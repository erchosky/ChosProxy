//! `chosplugin`: signing tool for ChosProxy plugin publishers (ADR-0033).
//!
//! ```text
//! chosplugin keygen <key-file>                       new Ed25519 key (PKCS#8, private)
//! chosplugin sign <plugin-dir> <key-file> <publisher> write plugin.sig
//! chosplugin verify <plugin-dir>                      check plugin.sig
//! ```

use chosproxy_plugin::{Manifest, Verified, generate_key, key_fingerprint, sign_dir, verify_dir};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage:
  chosplugin keygen <key-file>
  chosplugin sign <plugin-dir> <key-file> <publisher>
  chosplugin verify <plugin-dir>";

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options
        .open(path)
        .map_err(|e| format!("{}: {e} (it must not exist yet)", path.display()))?;
    f.write_all(bytes).map_err(|e| e.to_string())
}

fn run(args: &[String]) -> Result<String, String> {
    match args {
        [cmd, key] if cmd == "keygen" => {
            let (pkcs8, public) = generate_key()?;
            write_private(Path::new(key), &pkcs8)?;
            Ok(format!(
                "Private key written to {key} (keep it secret).\nPublic key: {public}\nFingerprint: {}",
                key_fingerprint(&public)
            ))
        }
        [cmd, dir, key, publisher] if cmd == "sign" => {
            let pkcs8 = std::fs::read(key).map_err(|e| format!("{key}: {e}"))?;
            let sig = sign_dir(Path::new(dir), &pkcs8, publisher)?;
            Ok(format!(
                "Signed {dir} as {publisher}.\nPublic key: {}\nFingerprint: {}",
                sig.key,
                key_fingerprint(&sig.key)
            ))
        }
        [cmd, dir] if cmd == "verify" => {
            let manifest = Manifest::load(Path::new(dir))?;
            match verify_dir(Path::new(dir), &manifest) {
                Verified::Unsigned => Err("not signed".into()),
                Verified::Invalid(e) => Err(format!("invalid signature: {e}")),
                Verified::Valid { publisher, key } => Ok(format!(
                    "Valid signature by {publisher}, key fingerprint {}",
                    key_fingerprint(&key)
                )),
            }
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    use std::io::Write;
    match run(&args) {
        Ok(out) => {
            let _ = writeln!(std::io::stdout(), "{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "{e}");
            ExitCode::FAILURE
        }
    }
}
