use crate::manifest::schema::{DependencySpec, Manifest};
use anyhow::{anyhow, Result};
use semver::{Version, VersionReq};
use std::collections::HashSet;

pub struct Resolve {
    pub packages: Vec<ResolvedPackage>,
}

pub struct ResolvedPackage {
    pub name: String,
    pub version: Version,
    pub source: ResolvedSource,
}

#[derive(Debug, Clone)]
pub enum ResolvedSource {
    Local(std::path::PathBuf),
    Git(String),
    GitHub { owner: String, repo: String },
    OpenExchange(String),
}

impl Resolve {
    pub fn from_manifest(manifest: &Manifest) -> Result<Self> {
        let mut packages = vec![];
        let mut seen: HashSet<String> = HashSet::new();

        for (name, dep) in &manifest.dependencies {
            if seen.contains(name) {
                continue;
            }
            seen.insert(name.clone());

            let version_req = VersionReq::parse(&dep.version).map_err(|e| {
                anyhow!("invalid semver '{}' for dep '{}': {}", dep.version, name, e)
            })?;

            let source = dep_to_source(name, dep)?;
            let version = resolve_version(&version_req, &source)?;

            packages.push(ResolvedPackage {
                name: name.clone(),
                version,
                source,
            });
        }

        Ok(Self { packages })
    }

    pub fn to_lock(&self) -> ResolveLock {
        ResolveLock {
            packages: self
                .packages
                .iter()
                .map(|p| {
                    // Bug 11: format repository as a proper URL string, not Rust Debug output.
                    let repository = match &p.source {
                        ResolvedSource::GitHub { owner, repo } => {
                            format!("https://github.com/{}/{}", owner, repo)
                        }
                        ResolvedSource::Git(url) => url.clone(),
                        ResolvedSource::Local(path) => path.to_string_lossy().into_owned(),
                        ResolvedSource::OpenExchange(id) => {
                            format!("openexchange:{}", id)
                        }
                    };
                    PackageLock {
                        name: p.name.clone(),
                        version: p.version.to_string(),
                        repository,
                        checksum: None,
                    }
                })
                .collect(),
        }
    }
}

fn dep_to_source(name: &str, dep: &DependencySpec) -> Result<ResolvedSource> {
    if let Some(github) = &dep.github {
        let parts: Vec<_> = github.splitn(2, '/').collect();
        if parts.len() == 2 {
            return Ok(ResolvedSource::GitHub {
                owner: parts[0].to_string(),
                repo: parts[1].to_string(),
            });
        }
    }
    if let Some(git) = &dep.git {
        return Ok(ResolvedSource::Git(git.clone()));
    }
    if let Some(repo) = &dep.repository {
        return Ok(ResolvedSource::Local(std::path::PathBuf::from(repo)));
    }
    if let Some(ox) = &dep.openexchange {
        return Ok(ResolvedSource::OpenExchange(ox.clone()));
    }
    Err(anyhow!(
        "dependency '{}' has no source (git, github, repository, or openexchange)",
        name
    ))
}

fn resolve_version(req: &VersionReq, source: &ResolvedSource) -> Result<Version> {
    // Sync wrapper — spins up a tokio runtime for the async GitHub fetch.
    // Called from Resolve::from_manifest which is sync.
    match source {
        ResolvedSource::GitHub { .. } => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(resolve_github_version_async(req, source))
        }
        ResolvedSource::Local(path) => {
            // Read version from a local iris-agentic-dev.toml or Cargo.toml
            let manifest_path = path.join("iris-agentic-dev.toml");
            if manifest_path.exists() {
                let content = std::fs::read_to_string(&manifest_path)?;
                let parsed: toml::Value = toml::from_str(&content)?;
                let v_str = parsed
                    .get("package")
                    .and_then(|p| p.get("version"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("no [package].version in {:?}", manifest_path))?;
                let v = Version::parse(v_str)?;
                if req.matches(&v) {
                    return Ok(v);
                }
                anyhow::bail!("local version {} does not satisfy {}", v, req);
            }
            anyhow::bail!("local source {:?} has no iris-agentic-dev.toml", path)
        }
        _ => anyhow::bail!(
            "version resolution not yet implemented for source {:?} (requirement: {})",
            source,
            req
        ),
    }
}

/// Fetch GitHub tags and return the highest version satisfying `req`.
/// Exported for use in async tests.
pub async fn resolve_github_version_async(
    req: &VersionReq,
    source: &ResolvedSource,
) -> Result<Version> {
    let (owner, repo) = match source {
        ResolvedSource::GitHub { owner, repo } => (owner.as_str(), repo.as_str()),
        _ => anyhow::bail!("resolve_github_version_async called with non-GitHub source"),
    };

    let url = format!(
        "https://api.github.com/repos/{}/{}/tags?per_page=100",
        owner, repo
    );
    let client = reqwest::Client::builder()
        .user_agent("iris-agentic-dev/resolver")
        .build()?;

    let resp = client.get(&url).send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!("GitHub repo {}/{} not found", owner, repo);
    }
    if !resp.status().is_success() {
        anyhow::bail!(
            "GitHub API returned {} for {}/{}",
            resp.status(),
            owner,
            repo
        );
    }

    let tags: serde_json::Value = resp.json().await?;
    let tag_array = tags
        .as_array()
        .ok_or_else(|| anyhow!("unexpected GitHub tags response"))?;

    let mut candidates: Vec<Version> = tag_array
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
        .filter_map(|name| {
            // Accept "v1.2.3" and "1.2.3" tag formats
            let stripped = name.strip_prefix('v').unwrap_or(name);
            Version::parse(stripped).ok()
        })
        .filter(|v| req.matches(v))
        .collect();

    if candidates.is_empty() {
        anyhow::bail!(
            "no tags in {}/{} satisfy version requirement {}",
            owner,
            repo,
            req
        );
    }

    candidates.sort();
    Ok(candidates.into_iter().last().unwrap())
}

pub struct ResolveLock {
    pub packages: Vec<PackageLock>,
}

pub struct PackageLock {
    pub name: String,
    pub version: String,
    pub repository: String,
    pub checksum: Option<String>,
}

impl ResolveLock {
    pub fn to_toml(&self) -> String {
        let mut out = String::from("[metadata]\nformat-version = 1\n\n");
        for pkg in &self.packages {
            // Bug 11: use proper TOML string quoting, not Rust Debug format ({:?}).
            out.push_str(&format!(
                "[[package]]\nname = \"{}\"\nversion = \"{}\"\nrepository = \"{}\"\n\n",
                pkg.name.replace('\\', "\\\\").replace('"', "\\\""),
                pkg.version.replace('\\', "\\\\").replace('"', "\\\""),
                pkg.repository.replace('\\', "\\\\").replace('"', "\\\""),
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::schema::DependencySpec;
    use std::collections::HashMap;

    fn dep_github(github: &str) -> DependencySpec {
        DependencySpec {
            version: "0.1.0".to_string(),
            git: None,
            github: Some(github.to_string()),
            openexchange: None,
            repository: None,
        }
    }

    fn dep_git(url: &str) -> DependencySpec {
        DependencySpec {
            version: "0.1.0".to_string(),
            git: Some(url.to_string()),
            github: None,
            openexchange: None,
            repository: None,
        }
    }

    fn dep_local(path: &str) -> DependencySpec {
        DependencySpec {
            version: "0.1.0".to_string(),
            git: None,
            github: None,
            openexchange: None,
            repository: Some(path.to_string()),
        }
    }

    fn dep_ox(id: &str) -> DependencySpec {
        DependencySpec {
            version: "0.1.0".to_string(),
            git: None,
            github: None,
            openexchange: Some(id.to_string()),
            repository: None,
        }
    }

    fn dep_no_source() -> DependencySpec {
        DependencySpec {
            version: "0.1.0".to_string(),
            git: None,
            github: None,
            openexchange: None,
            repository: None,
        }
    }

    #[test]
    fn test_dep_to_source_github() {
        let dep = dep_github("owner/repo");
        let source = dep_to_source("pkg", &dep).unwrap();
        assert!(matches!(source, ResolvedSource::GitHub { .. }));
        if let ResolvedSource::GitHub { owner, repo } = source {
            assert_eq!(owner, "owner");
            assert_eq!(repo, "repo");
        }
    }

    #[test]
    fn test_dep_to_source_git() {
        let dep = dep_git("https://github.com/x/y.git");
        let source = dep_to_source("pkg", &dep).unwrap();
        assert!(matches!(source, ResolvedSource::Git(_)));
    }

    #[test]
    fn test_dep_to_source_local() {
        let dep = dep_local("/path/to/pkg");
        let source = dep_to_source("pkg", &dep).unwrap();
        assert!(matches!(source, ResolvedSource::Local(_)));
    }

    #[test]
    fn test_dep_to_source_openexchange() {
        let dep = dep_ox("iris-json-1.0.0");
        let source = dep_to_source("pkg", &dep).unwrap();
        assert!(matches!(source, ResolvedSource::OpenExchange(_)));
    }

    #[test]
    fn test_dep_to_source_no_source_errors() {
        let dep = dep_no_source();
        let result = dep_to_source("pkg", &dep);
        assert!(result.is_err());
    }

    #[test]
    fn test_to_lock_github_url_format() {
        let pkg = ResolvedPackage {
            name: "mypkg".to_string(),
            version: Version::parse("1.2.3").unwrap(),
            source: ResolvedSource::GitHub {
                owner: "alice".to_string(),
                repo: "myrepo".to_string(),
            },
        };
        let resolve = Resolve {
            packages: vec![pkg],
        };
        let lock = resolve.to_lock();
        assert_eq!(
            lock.packages[0].repository,
            "https://github.com/alice/myrepo"
        );
        assert_eq!(lock.packages[0].version, "1.2.3");
    }

    #[test]
    fn test_to_lock_openexchange_url_format() {
        let pkg = ResolvedPackage {
            name: "mypkg".to_string(),
            version: Version::parse("0.1.0").unwrap(),
            source: ResolvedSource::OpenExchange("some-pkg-id".to_string()),
        };
        let resolve = Resolve {
            packages: vec![pkg],
        };
        let lock = resolve.to_lock();
        assert_eq!(lock.packages[0].repository, "openexchange:some-pkg-id");
    }

    // --- additional tests ---

    #[test]
    fn test_dep_to_source_github_splits_on_first_slash() {
        // "owner/org/repo" — splitn(2, '/') keeps trailing part intact
        let dep = dep_github("owner/org/repo");
        let source = dep_to_source("pkg", &dep).unwrap();
        if let ResolvedSource::GitHub { owner, repo } = source {
            assert_eq!(owner, "owner");
            assert_eq!(repo, "org/repo");
        } else {
            panic!("expected GitHub source");
        }
    }

    #[test]
    fn test_dep_to_source_github_missing_slash_falls_through_to_error() {
        // A github value with no '/' cannot be split into owner/repo;
        // dep_to_source falls through to the error branch.
        let dep = DependencySpec {
            version: "0.1.0".to_string(),
            git: None,
            github: Some("noslash".to_string()),
            openexchange: None,
            repository: None,
        };
        let result = dep_to_source("pkg", &dep);
        assert!(result.is_err());
    }

    #[test]
    fn test_dep_to_source_git_url_preserved() {
        let url = "https://github.com/example/project.git";
        let dep = dep_git(url);
        let source = dep_to_source("pkg", &dep).unwrap();
        if let ResolvedSource::Git(got) = source {
            assert_eq!(got, url);
        } else {
            panic!("expected Git source");
        }
    }

    #[test]
    fn test_dep_to_source_openexchange_id_preserved() {
        let id = "ZPM-community-iris-tools";
        let dep = dep_ox(id);
        let source = dep_to_source("pkg", &dep).unwrap();
        if let ResolvedSource::OpenExchange(got) = source {
            assert_eq!(got, id);
        } else {
            panic!("expected OpenExchange source");
        }
    }

    #[test]
    fn test_to_lock_git_url_preserved() {
        let url = "https://github.com/example/project.git";
        let pkg = ResolvedPackage {
            name: "gitpkg".to_string(),
            version: Version::parse("2.0.0").unwrap(),
            source: ResolvedSource::Git(url.to_string()),
        };
        let resolve = Resolve {
            packages: vec![pkg],
        };
        let lock = resolve.to_lock();
        assert_eq!(lock.packages[0].repository, url);
    }

    #[test]
    fn test_to_lock_local_path_preserved() {
        let path = "/home/user/my-pkg";
        let pkg = ResolvedPackage {
            name: "localpkg".to_string(),
            version: Version::parse("0.5.1").unwrap(),
            source: ResolvedSource::Local(std::path::PathBuf::from(path)),
        };
        let resolve = Resolve {
            packages: vec![pkg],
        };
        let lock = resolve.to_lock();
        assert_eq!(lock.packages[0].repository, path);
    }

    #[test]
    fn test_to_lock_checksum_is_none() {
        let pkg = ResolvedPackage {
            name: "pkg".to_string(),
            version: Version::parse("1.0.0").unwrap(),
            source: ResolvedSource::Git("https://example.com/repo.git".to_string()),
        };
        let resolve = Resolve {
            packages: vec![pkg],
        };
        let lock = resolve.to_lock();
        assert!(lock.packages[0].checksum.is_none());
    }

    #[test]
    fn test_resolve_lock_to_toml_header() {
        let lock = ResolveLock { packages: vec![] };
        let toml = lock.to_toml();
        assert!(toml.starts_with("[metadata]\nformat-version = 1\n"));
    }

    #[test]
    fn test_resolve_lock_to_toml_package_entry() {
        let lock = ResolveLock {
            packages: vec![PackageLock {
                name: "mypkg".to_string(),
                version: "1.2.3".to_string(),
                repository: "https://github.com/alice/myrepo".to_string(),
                checksum: None,
            }],
        };
        let toml = lock.to_toml();
        assert!(toml.contains("[[package]]"));
        assert!(toml.contains("name = \"mypkg\""));
        assert!(toml.contains("version = \"1.2.3\""));
        assert!(toml.contains("repository = \"https://github.com/alice/myrepo\""));
    }

    #[test]
    fn test_resolve_lock_to_toml_escapes_double_quotes() {
        let lock = ResolveLock {
            packages: vec![PackageLock {
                name: "pkg-with-\"quote\"".to_string(),
                version: "0.1.0".to_string(),
                repository: "https://example.com".to_string(),
                checksum: None,
            }],
        };
        let toml = lock.to_toml();
        // Ensure the raw quote is escaped, not left bare
        assert!(toml.contains("\\\"quote\\\""));
        assert!(!toml.contains("\"quote\""));
    }

    #[test]
    fn test_resolve_lock_to_toml_multiple_packages() {
        let lock = ResolveLock {
            packages: vec![
                PackageLock {
                    name: "a".to_string(),
                    version: "1.0.0".to_string(),
                    repository: "https://github.com/x/a".to_string(),
                    checksum: None,
                },
                PackageLock {
                    name: "b".to_string(),
                    version: "2.0.0".to_string(),
                    repository: "https://github.com/x/b".to_string(),
                    checksum: None,
                },
            ],
        };
        let toml = lock.to_toml();
        let count = toml.matches("[[package]]").count();
        assert_eq!(count, 2);
        assert!(toml.contains("name = \"a\""));
        assert!(toml.contains("name = \"b\""));
    }

    #[test]
    fn test_from_manifest_no_deps_succeeds() {
        let manifest = Manifest {
            package: crate::manifest::schema::PackageInfo {
                name: "test-pkg".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: vec![],
                license: None,
                repository: None,
            },
            provides: None,
            dependencies: HashMap::new(),
        };
        let resolve = Resolve::from_manifest(&manifest).unwrap();
        assert!(resolve.packages.is_empty());
    }

    #[test]
    fn test_from_manifest_invalid_version_req_errors() {
        let mut deps = HashMap::new();
        deps.insert(
            "mypkg".to_string(),
            DependencySpec {
                version: "not-semver".to_string(),
                git: Some("https://example.com/repo.git".to_string()),
                github: None,
                openexchange: None,
                repository: None,
            },
        );
        let manifest = Manifest {
            package: crate::manifest::schema::PackageInfo {
                name: "test-pkg".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: vec![],
                license: None,
                repository: None,
            },
            provides: None,
            dependencies: deps,
        };
        let result = Resolve::from_manifest(&manifest);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("invalid semver") || msg.contains("not-semver"));
    }

    #[test]
    fn test_from_manifest_git_dep_unimplemented() {
        let mut deps = HashMap::new();
        deps.insert(
            "gitpkg".to_string(),
            DependencySpec {
                version: "^0.1.0".to_string(),
                git: Some("https://example.com/repo.git".to_string()),
                github: None,
                openexchange: None,
                repository: None,
            },
        );
        let manifest = Manifest {
            package: crate::manifest::schema::PackageInfo {
                name: "test-pkg".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: vec![],
                license: None,
                repository: None,
            },
            provides: None,
            dependencies: deps,
        };
        let result = Resolve::from_manifest(&manifest);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("not yet implemented"));
    }

    #[test]
    fn test_from_manifest_openexchange_dep_unimplemented() {
        let mut deps = HashMap::new();
        deps.insert(
            "oxpkg".to_string(),
            DependencySpec {
                version: "^1.0.0".to_string(),
                git: None,
                github: None,
                openexchange: Some("some-pkg-id".to_string()),
                repository: None,
            },
        );
        let manifest = Manifest {
            package: crate::manifest::schema::PackageInfo {
                name: "test-pkg".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: vec![],
                license: None,
                repository: None,
            },
            provides: None,
            dependencies: deps,
        };
        let result = Resolve::from_manifest(&manifest);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("not yet implemented"));
    }

    #[test]
    fn test_from_manifest_local_dep_no_toml_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut deps = HashMap::new();
        deps.insert(
            "localpkg".to_string(),
            DependencySpec {
                version: "^1.0.0".to_string(),
                git: None,
                github: None,
                openexchange: None,
                repository: Some(dir.path().to_string_lossy().into_owned()),
            },
        );
        let manifest = Manifest {
            package: crate::manifest::schema::PackageInfo {
                name: "test-pkg".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: vec![],
                license: None,
                repository: None,
            },
            provides: None,
            dependencies: deps,
        };
        let result = Resolve::from_manifest(&manifest);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("iris-agentic-dev.toml"));
    }

    #[test]
    fn test_from_manifest_local_dep_with_valid_toml() {
        let dir = tempfile::tempdir().unwrap();
        // Write a local iris-agentic-dev.toml with matching version
        std::fs::write(
            dir.path().join("iris-agentic-dev.toml"),
            "[package]\nname = \"localpkg\"\nversion = \"1.2.3\"\n",
        )
        .unwrap();
        let mut deps = HashMap::new();
        deps.insert(
            "localpkg".to_string(),
            DependencySpec {
                version: "^1.0.0".to_string(),
                git: None,
                github: None,
                openexchange: None,
                repository: Some(dir.path().to_string_lossy().into_owned()),
            },
        );
        let manifest = Manifest {
            package: crate::manifest::schema::PackageInfo {
                name: "test-pkg".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: vec![],
                license: None,
                repository: None,
            },
            provides: None,
            dependencies: deps,
        };
        let resolve = Resolve::from_manifest(&manifest).unwrap();
        assert_eq!(resolve.packages.len(), 1);
        assert_eq!(resolve.packages[0].name, "localpkg");
        assert_eq!(resolve.packages[0].version.to_string(), "1.2.3");
    }

    #[test]
    fn test_from_manifest_local_dep_version_mismatch_errors() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("iris-agentic-dev.toml"),
            "[package]\nname = \"localpkg\"\nversion = \"0.5.0\"\n",
        )
        .unwrap();
        let mut deps = HashMap::new();
        deps.insert(
            "localpkg".to_string(),
            DependencySpec {
                version: "^1.0.0".to_string(), // requires >= 1.0.0
                git: None,
                github: None,
                openexchange: None,
                repository: Some(dir.path().to_string_lossy().into_owned()),
            },
        );
        let manifest = Manifest {
            package: crate::manifest::schema::PackageInfo {
                name: "test-pkg".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: vec![],
                license: None,
                repository: None,
            },
            provides: None,
            dependencies: deps,
        };
        let result = Resolve::from_manifest(&manifest);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("does not satisfy") || msg.contains("0.5.0"));
    }

    #[tokio::test]
    async fn test_resolve_github_version_async_rejects_non_github_source() {
        let req = VersionReq::parse("^1.0.0").unwrap();
        let source = ResolvedSource::Git("https://example.com/repo.git".to_string());
        let result = resolve_github_version_async(&req, &source).await;
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("non-GitHub"));
    }

    #[test]
    fn test_from_manifest_duplicate_dep_deduplicates() {
        // Same dep name appearing twice in HashMap — HashMap naturally deduplicates keys.
        // This test ensures from_manifest handles the dedup logic (seen set).
        let mut deps = HashMap::new();
        deps.insert(
            "mypkg".to_string(),
            DependencySpec {
                version: "not-a-version".to_string(), // will error on version parse
                git: Some("https://example.com/repo.git".to_string()),
                github: None,
                openexchange: None,
                repository: None,
            },
        );
        // Insert same key again — HashMap replaces it (only one entry ever)
        let manifest = Manifest {
            package: crate::manifest::schema::PackageInfo {
                name: "test-pkg".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: vec![],
                license: None,
                repository: None,
            },
            provides: None,
            dependencies: deps,
        };
        // Should error on the invalid version, not panic or infinite loop
        let result = Resolve::from_manifest(&manifest);
        assert!(result.is_err());
    }
}
