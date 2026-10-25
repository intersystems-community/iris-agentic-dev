pub mod resolve;
mod schema;

pub use resolve::Resolve;
pub use schema::{DependencySpec, Manifest, PackageInfo, Provides};

use anyhow::{Context, Result};
use std::path::Path;

pub fn parse_manifest(path: impl AsRef<Path>) -> Result<Manifest> {
    let content = std::fs::read_to_string(path.as_ref())
        .with_context(|| format!("reading {}", path.as_ref().display()))?;
    let manifest: Manifest =
        toml::from_str(&content).with_context(|| format!("parsing {}", path.as_ref().display()))?;
    check_names(&manifest, path.as_ref())?;
    Ok(manifest)
}

/// A name that holds a control character (a newline, an escape, a NUL) is refused before it reaches
/// resolution, a lock file or a terminal (FR-026).
fn check_names(manifest: &Manifest, path: &Path) -> Result<()> {
    let names = std::iter::once(("package name", &manifest.package.name))
        .chain(manifest.dependencies.keys().map(|k| ("dependency name", k)));
    for (what, name) in names {
        if name.chars().any(char::is_control) {
            anyhow::bail!(
                "INVALID_MANIFEST: {what} {name:?} in {} contains a control character",
                path.display()
            );
        }
    }
    Ok(())
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
