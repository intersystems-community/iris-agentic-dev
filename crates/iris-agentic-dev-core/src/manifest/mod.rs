pub mod resolve;
mod schema;

pub use resolve::Resolve;
pub use schema::{DependencySpec, Manifest, PackageInfo, Provides};

use anyhow::{Context, Result};
use std::path::Path;

pub fn parse_manifest(path: impl AsRef<Path>) -> Result<Manifest> {
    let content = std::fs::read_to_string(path.as_ref())
        .with_context(|| format!("reading {}", path.as_ref().display()))?;
    toml::from_str(&content).with_context(|| format!("parsing {}", path.as_ref().display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_manifest_valid_file() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"[package]
name = "test-pkg"
version = "1.0.0"
"#
        )
        .unwrap();
        let manifest = parse_manifest(f.path()).unwrap();
        assert_eq!(manifest.package.name, "test-pkg");
        assert_eq!(manifest.package.version, "1.0.0");
    }

    #[test]
    fn parse_manifest_missing_file_errors() {
        let result = parse_manifest("/tmp/nonexistent-iris-dev-manifest-xyz.toml");
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("reading") || msg.contains("No such file"));
    }

    #[test]
    fn parse_manifest_invalid_toml_errors() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "this is not valid toml ={{{{").unwrap();
        let result = parse_manifest(f.path());
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("parsing") || msg.contains("expected"));
    }
}
