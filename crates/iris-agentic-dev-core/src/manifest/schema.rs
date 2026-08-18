use serde::Deserialize;
use std::collections::HashMap;

/// Root iris-dev.toml manifest.
/// Designed to be extensible: [provides] covers developer tooling now;
/// [iris_app] is reserved for future IRIS application deployment.
#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub package: PackageInfo,
    pub provides: Option<Provides>,
    #[serde(default)]
    pub dependencies: HashMap<String, DependencySpec>,
    // pub iris_app: Option<IrisApp>,  // Future: IRIS application deployment
}

#[derive(Debug, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
}

/// Developer tooling package contents.
#[derive(Debug, Deserialize, Default)]
pub struct Provides {
    /// Relative paths to SKILL.md files
    #[serde(default)]
    pub skills: Vec<String>,
    /// Relative paths to KB markdown files
    #[serde(default)]
    pub kb_items: Vec<String>,
    /// iris-dev-* binary names this package provides
    #[serde(default)]
    pub plugins: Vec<String>,
    /// Tool allowlist: when non-empty, only these named tools are exposed after install.
    /// Written into the workspace config `enabled_tools` field by the resolve/install
    /// command (075-modular-tool-install, FR-004). All names are validated against the
    /// live tool registry at resolve time; an unknown name is an error (FR-005).
    #[serde(default)]
    pub tools: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct DependencySpec {
    pub version: String,
    pub git: Option<String>,
    pub github: Option<String>,
    pub openexchange: Option<String>,
    pub repository: Option<String>,
}
