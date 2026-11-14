//! Product branding.
//!
//! The product name is defined once, in `branding.json` at the repository
//! root, and compiled into the binary. Rename the product by editing that
//! file (and `productName` in `tauri.conf.json`, which a test keeps in sync).

use serde::Deserialize;
use std::sync::OnceLock;

const BRANDING_JSON: &str = include_str!("../../../branding.json");

/// User-visible identity of the product.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Brand {
    /// Human-readable product name, e.g. "ChosProxy".
    pub product_name: String,
    /// Executable / package name, e.g. "chosproxy".
    pub binary_name: String,
    /// Reverse-DNS application identifier.
    pub identifier: String,
    /// Directory name used under the platform config/data dirs.
    pub config_dir_name: String,
    /// One-line description.
    pub tagline: String,
}

/// Returns the compiled-in branding.
pub fn brand() -> &'static Brand {
    static BRAND: OnceLock<Brand> = OnceLock::new();
    BRAND.get_or_init(|| {
        parse(BRANDING_JSON).expect("branding.json is validated by the brand tests in CI")
    })
}

fn parse(json: &str) -> Result<Brand, String> {
    let brand: Brand = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let valid_ident = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    };
    if brand.product_name.trim().is_empty() {
        return Err("productName must not be empty".into());
    }
    if !valid_ident(&brand.binary_name) || !valid_ident(&brand.config_dir_name) {
        return Err("binaryName and configDirName must be [A-Za-z0-9._-]+".into());
    }
    Ok(brand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_branding_is_valid() {
        assert!(parse(BRANDING_JSON).is_ok());
        assert!(!brand().product_name.is_empty());
    }

    #[test]
    fn rejects_path_like_dir_names() {
        let json = r#"{"productName":"X","binaryName":"x","identifier":"a.b",
            "configDirName":"../evil","tagline":""}"#;
        assert!(parse(json).is_err());
    }

    #[test]
    fn tauri_product_name_matches_branding() {
        let conf = include_str!("../../../apps/desktop/src-tauri/tauri.conf.json");
        let v: serde_json::Value = serde_json::from_str(conf).expect("valid tauri.conf.json");
        assert_eq!(v["productName"], brand().product_name);
        assert_eq!(v["identifier"], brand().identifier);
    }
}
