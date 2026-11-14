//! `plugin.toml`: who a plugin is, how to start it and what it needs.

use crate::permissions::Permission;
use ring::digest::{Context, SHA256};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// The plugin API version this host speaks.
pub const API_VERSION: u32 = 2;
/// Every API version this ChosProxy runs (v1 plugins keep working).
pub const SUPPORTED_APIS: [u32; 2] = [1, 2];
/// Name of the manifest file in a plugin directory.
pub const MANIFEST_FILE: &str = "plugin.toml";
/// Largest plugin directory hashed (and accepted).
const MAX_PLUGIN_BYTES: u64 = 64 * 1024 * 1024;

/// A plugin manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, rename = "PluginManifest")]
pub struct Manifest {
    /// Stable id: lower-case letters, digits and dashes.
    pub id: String,
    /// Name shown.
    pub name: String,
    /// Version.
    pub version: String,
    /// What it does.
    #[serde(default)]
    pub description: String,
    /// Who made it.
    #[serde(default)]
    pub author: String,
    /// Plugin API version it was written for.
    pub api: u32,
    /// Program and arguments, run in the plugin directory. A program with a
    /// `/` is relative to that directory; otherwise it is looked up in PATH.
    pub command: Vec<String>,
    /// Permissions it asks for.
    #[serde(default)]
    pub permissions: Vec<String>,
}

impl Manifest {
    /// Reads and checks a manifest.
    /// Reads and validates `plugin.toml` of a plugin directory.
    pub fn load(dir: &Path) -> Result<Manifest, String> {
        let text = std::fs::read_to_string(dir.join(MANIFEST_FILE))
            .map_err(|e| format!("cannot read {MANIFEST_FILE}: {e}"))?;
        Manifest::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Manifest, String> {
        let m: Manifest =
            toml::from_str(text).map_err(|e| format!("invalid {MANIFEST_FILE}: {e}"))?;
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<(), String> {
        let id_ok = (2..=64).contains(&self.id.len())
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !self.id.starts_with('-');
        if !id_ok {
            return Err("the id must be 2–64 lower-case letters, digits or dashes".into());
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 100 {
            return Err("the name must be 1–100 characters".into());
        }
        if self.version.trim().is_empty() || self.version.len() > 40 {
            return Err("the version must be 1–40 characters".into());
        }
        if !SUPPORTED_APIS.contains(&self.api) {
            return Err(format!(
                "written for plugin API {}; this ChosProxy supports APIs 1 and {API_VERSION}",
                self.api
            ));
        }
        let Some(program) = self.command.first() else {
            return Err("the command is empty".into());
        };
        if program.trim().is_empty() || self.command.len() > 50 {
            return Err("invalid command".into());
        }
        if program.contains("..") || Path::new(program).is_absolute() {
            return Err("the program must be in PATH or inside the plugin directory".into());
        }
        for p in &self.permissions {
            match Permission::parse(p) {
                None => return Err(format!("unknown permission {p:?}")),
                Some(perm) if !perm.supported_in(self.api) => {
                    return Err(format!(
                        "permission {p} is not available in plugin API {}; use api = 2",
                        self.api
                    ));
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    /// The permissions asked for, parsed (the manifest is validated).
    pub fn requested(&self) -> Vec<Permission> {
        let mut out: Vec<Permission> = self
            .permissions
            .iter()
            .filter_map(|p| Permission::parse(p))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The program to run: inside `dir` when it contains a path separator.
    pub fn program(&self, dir: &Path) -> PathBuf {
        let program = self.command.first().map_or("", String::as_str);
        if program.contains('/') || program.contains('\\') {
            dir.join(program)
        } else {
            PathBuf::from(program)
        }
    }
}

/// SHA-256 over every file of a plugin directory (paths and contents, in
/// sorted order). Approvals are tied to it: a changed plugin needs to be
/// approved again.
pub fn hash_dir(dir: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    collect(dir, dir, &mut files, 0)?;
    files.sort();
    let mut ctx = Context::new(&SHA256);
    let mut total: u64 = 0;
    for rel in files {
        let bytes = std::fs::read(dir.join(&rel)).map_err(|e| format!("cannot read {rel}: {e}"))?;
        total += bytes.len() as u64;
        if total > MAX_PLUGIN_BYTES {
            return Err("the plugin directory is larger than 64 MiB".into());
        }
        ctx.update(rel.as_bytes());
        ctx.update(&[0]);
        ctx.update(&(bytes.len() as u64).to_le_bytes());
        ctx.update(&bytes);
    }
    Ok(ctx
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>, depth: u32) -> Result<(), String> {
    if depth > 8 {
        return Err("the plugin directory is nested too deeply".into());
    }
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot list {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if kind.is_symlink() {
            return Err(format!(
                "{} is a symbolic link; plugins cannot contain links",
                path.display()
            ));
        }
        if kind.is_dir() {
            // Caches written by interpreters change on every run.
            if entry.file_name() == "__pycache__" {
                continue;
            }
            collect(root, &path, out, depth + 1)?;
        } else if kind.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            // The signature signs the fingerprint, so it is not part of it.
            if rel == crate::signing::SIG_FILE {
                continue;
            }
            out.push(rel);
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    const GOOD: &str = r#"
id = "hello-plugin"
name = "HelloPlugin"
version = "1.0.0"
api = 1
command = ["python3", "hello.py"]
permissions = ["network.read", "scanner.passive", "network.read"]
"#;

    #[test]
    fn parses_and_checks_manifests() {
        let m = Manifest::parse(GOOD).unwrap();
        assert_eq!(
            m.requested(),
            [Permission::NetworkRead, Permission::ScannerPassive]
        );
        assert_eq!(m.program(Path::new("/p")), PathBuf::from("python3"));

        let bad = |from: &str, to: &str| Manifest::parse(&GOOD.replace(from, to)).unwrap_err();
        assert!(bad("hello-plugin", "Hello Plugin").contains("id"));
        assert!(bad("api = 1", "api = 3").contains("API 3"));
        assert!(
            bad("\"network.read\", \"scanner", "\"network.send\", \"scanner")
                .contains("not available")
        );
        assert!(bad("\"scanner.passive\"", "\"disk.write\"").contains("unknown permission"));
        assert!(
            bad("[\"python3\", \"hello.py\"]", "[\"../../bin/sh\"]").contains("PATH or inside")
        );
        assert!(bad("[\"python3\", \"hello.py\"]", "[]").contains("empty"));
        assert!(Manifest::parse("id = \"x\"\nsurprise = 1").is_err());

        let local =
            Manifest::parse(&GOOD.replace("[\"python3\", \"hello.py\"]", "[\"bin/run\"]")).unwrap();
        assert_eq!(local.program(Path::new("/p")), PathBuf::from("/p/bin/run"));
    }

    #[test]
    fn hashes_change_with_contents() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MANIFEST_FILE), GOOD).unwrap();
        std::fs::write(dir.path().join("hello.py"), "print(1)").unwrap();
        std::fs::create_dir(dir.path().join("__pycache__")).unwrap();
        let first = hash_dir(dir.path()).unwrap();
        std::fs::write(dir.path().join("__pycache__/x.pyc"), "cache").unwrap();
        assert_eq!(hash_dir(dir.path()).unwrap(), first, "caches are ignored");
        std::fs::write(dir.path().join("hello.py"), "print(2)").unwrap();
        assert_ne!(hash_dir(dir.path()).unwrap(), first);
        assert_eq!(first.len(), 64);
    }
}
