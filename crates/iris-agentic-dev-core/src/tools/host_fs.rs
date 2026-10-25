//! Confinement of the host's disk to the project directory (spec 141, T212 and T216).
//!
//! Three tools take a path from the agent and read it on the machine iad runs on: `iris_compile`
//! (a path-like `target` is uploaded), `kb index` (a directory goes into `^KBCHUNKS`) and
//! `iris_symbols_local` (a directory is crawled). Each is held to one rule.
//!
//! **The project directory** is the first of: `OBJECTSCRIPT_WORKSPACE`, `--workspace`, the
//! directory of the workspace file the walk up from the working directory finds, the working
//! directory. It is made canonical. When none of those is a usable place there is no project
//! directory and every host path is refused (the stricter reading): a directory that does not
//! exist, the filesystem root, and the home directory (it holds the operator file and every
//! other project) are not usable.
//!
//! **A path is allowed** when, with symlinks resolved, it is inside the project directory. A
//! path with a `..` segment is refused whether or not it lands inside, a link that leaves the
//! directory is refused through its target, and a relative path is taken from the working
//! directory. A file must also have an extension the tool accepts, and that is judged first, so
//! a refusal does not say whether a file outside the project exists.
//!
//! The refusals are one fixed-rule code, [`ERR_HOST_PATH_BLOCKED`], with a `reason`.

use crate::iris::config_sources::LoadInputs;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

pub const ERR_HOST_PATH_BLOCKED: &str = "HOST_PATH_BLOCKED";

/// What `iris_compile` uploads.
pub const COMPILE_EXTENSIONS: &[&str] = &["cls", "mac", "inc", "int"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathRefusal {
    /// No project directory is known.
    NoRoot,
    /// The path has a `..` segment.
    ParentReference,
    /// The path, with links resolved, is not inside the project directory.
    OutsideRoot,
    /// A file whose extension the tool does not take.
    Extension,
    /// Inside the project directory, and nothing is there.
    NotFound,
}

impl PathRefusal {
    /// The `reason` field of the refusal.
    pub fn reason(&self) -> &'static str {
        match self {
            PathRefusal::NoRoot => "no_workspace",
            PathRefusal::ParentReference => "parent_reference",
            PathRefusal::OutsideRoot => "outside_workspace",
            PathRefusal::Extension => "extension",
            PathRefusal::NotFound => "not_found",
        }
    }
}

/// The project directory a load works in, or `None` when there is none (see the module comment).
pub fn project_root(inputs: &LoadInputs) -> Option<PathBuf> {
    let root = std::fs::canonicalize(crate::iris::config_sources::workspace_root(inputs)).ok()?;
    if !root.is_dir() || root.parent().is_none() {
        return None;
    }
    let home = inputs
        .home
        .as_deref()
        .and_then(|h| std::fs::canonicalize(h).ok());
    if home.as_deref() == Some(root.as_path()) {
        return None;
    }
    Some(root)
}

fn has_parent_segment(requested: &str) -> bool {
    requested.split(['/', '\\']).any(|s| s == "..")
        || Path::new(requested)
            .components()
            .any(|c| c == Component::ParentDir)
}

fn absolute(cwd: &Path, requested: &str) -> PathBuf {
    let p = Path::new(requested);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

fn has_extension(p: &Path, allowed: &[&str]) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| allowed.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

/// The canonical form of `p` when it is inside `root`, else `None`. `root` is canonical. A path
/// that does not exist is judged by its nearest existing ancestor; `Ok(None)` inside means the
/// path is inside the project and absent.
fn resolve_inside(root: &Path, p: &Path) -> Result<Option<PathBuf>, PathRefusal> {
    if let Ok(canonical) = std::fs::canonicalize(p) {
        return if canonical.starts_with(root) {
            Ok(Some(canonical))
        } else {
            Err(PathRefusal::OutsideRoot)
        };
    }
    let mut ancestor = p.parent();
    while let Some(a) = ancestor {
        if let Ok(canonical) = std::fs::canonicalize(a) {
            return if canonical.starts_with(root) {
                Ok(None)
            } else {
                Err(PathRefusal::OutsideRoot)
            };
        }
        ancestor = a.parent();
    }
    Err(PathRefusal::OutsideRoot)
}

/// `p` as a canonical path when it is inside `root`. For a caller that has listed a directory and
/// must not follow an entry that leaves it.
pub fn within(root: &Path, p: &Path) -> Option<PathBuf> {
    resolve_inside(root, p).ok().flatten()
}

/// The file `requested` names, when it is a file inside `root` with one of `extensions`. Read the
/// returned path, not `requested`.
pub fn confine_file(
    root: Option<&Path>,
    cwd: &Path,
    requested: &str,
    extensions: &[&str],
) -> Result<PathBuf, PathRefusal> {
    let root = root.ok_or(PathRefusal::NoRoot)?;
    if has_parent_segment(requested) {
        return Err(PathRefusal::ParentReference);
    }
    let abs = absolute(cwd, requested);
    if !has_extension(&abs, extensions) {
        return Err(PathRefusal::Extension);
    }
    match resolve_inside(root, &abs)? {
        // A link inside the project to a file of another kind.
        Some(c) if !has_extension(&c, extensions) => Err(PathRefusal::Extension),
        Some(c) if c.is_file() => Ok(c),
        _ => Err(PathRefusal::NotFound),
    }
}

/// The directory `requested` names, when it is a directory inside `root`.
pub fn confine_dir(
    root: Option<&Path>,
    cwd: &Path,
    requested: &str,
) -> Result<PathBuf, PathRefusal> {
    let root = root.ok_or(PathRefusal::NoRoot)?;
    if has_parent_segment(requested) {
        return Err(PathRefusal::ParentReference);
    }
    match resolve_inside(root, &absolute(cwd, requested))? {
        Some(c) if c.is_dir() => Ok(c),
        _ => Err(PathRefusal::NotFound),
    }
}

/// The body of a refusal. `NotFound` is not a refusal and has no body here.
pub fn refusal_body(tool: &str, requested: &str, why: &PathRefusal, allowed: &[&str]) -> Value {
    let (what, fix) = match why {
        PathRefusal::NoRoot => (
            "no project directory is known, so no host path is allowed".to_string(),
            "start iad in the project directory, or set OBJECTSCRIPT_WORKSPACE to it",
        ),
        PathRefusal::ParentReference => (
            "the path has a `..` segment".to_string(),
            "give the path from the project directory without `..`",
        ),
        PathRefusal::OutsideRoot => (
            "the path is outside the project directory (links are followed)".to_string(),
            "use a path inside the project directory",
        ),
        PathRefusal::Extension => (
            format!("only {} files are accepted", allowed.join(", ")),
            "name a source file of one of those kinds",
        ),
        PathRefusal::NotFound => ("nothing is there".to_string(), "check the path"),
    };
    let mut body = json!({
        "success": false,
        "error_code": ERR_HOST_PATH_BLOCKED,
        "reason": why.reason(),
        "tool": tool,
        "path": requested,
        "error": format!("{tool} will not read {requested}: {what}. {fix}."),
    });
    if *why == PathRefusal::Extension {
        body["allowed_extensions"] = json!(allowed);
    }
    body
}
