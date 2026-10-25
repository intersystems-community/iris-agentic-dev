//! Files that hold a credential or a record of calls, and files iad writes into a checkout.
//!
//! Three rules, one place (spec 141, FR-023, FR-026, FR-028):
//!
//! - The file is created with the mode it will keep. A write-then-chmod leaves a window in which
//!   another user reads it.
//! - The bytes go to `<name>.<random>.tmp`, opened with `create_new`, and are renamed over the
//!   target. A symlink planted at the temp name makes the open fail, and `rename` replaces a
//!   symlinked target instead of writing through it.
//! - A directory iad creates for itself is `0700`.
//!
//! Windows has no mode bits. The profile directory's ACL applies and the helpers set none; the
//! create-new and rename steps are the same.

use std::io::{self, Write};
use std::path::Path;

/// Write `bytes` to `path` so only the owner can read it: file `0600`, parent directory `0700`.
/// An existing file or directory with group or other access is tightened.
pub fn write_private_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = non_empty_parent(path) {
        ensure_private_dir(parent)?;
    }
    write_atomic(path, bytes, 0o600)
}

/// Write `bytes` to `path` with `mode` (ignored off Unix) through a temp file and a rename. The
/// parent directory must exist or is created with the default mode; use [`write_private_atomic`]
/// for a directory that is iad's own.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let parent = non_empty_parent(path).unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy();
    let tmp = parent.join(format!("{name}.{}.tmp", uuid::Uuid::new_v4().simple()));

    let result = (|| {
        let mut file = create_new(&tmp, mode)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Append `line` and a newline to `path`: file `0600`, parent directory `0700`. A file that is a
/// symlink is refused, since `append` would follow it. An existing file with group or other access
/// is tightened.
pub fn append_private(path: &Path, line: &str) -> io::Result<()> {
    if let Some(parent) = non_empty_parent(path) {
        ensure_private_dir(parent)?;
    }
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is a symlink; not appending through it", path.display()),
            ));
        }
        tighten(path, 0o600)?;
    }
    let mut file = open_append(path)?;
    // One write, so two processes appending do not interleave inside a line.
    file.write_all(format!("{line}\n").as_bytes())?;
    file.flush()
}

fn non_empty_parent(path: &Path) -> Option<&Path> {
    path.parent().filter(|p| !p.as_os_str().is_empty())
}

#[cfg(unix)]
fn create_new(path: &Path, mode: u32) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
}

#[cfg(not(unix))]
fn create_new(path: &Path, _mode: u32) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(unix)]
fn open_append(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_append(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
}

#[cfg(unix)]
fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if dir.exists() {
        return tighten(dir, 0o700);
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)
}

/// Set `mode` on `path` when it allows group or other access. One warning names the path.
#[cfg(unix)]
fn tighten(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let current = std::fs::metadata(path)?.permissions().mode() & 0o777;
    if current & 0o077 != 0 {
        tracing::warn!(
            "{} allowed group or other access (mode {current:o}); setting {mode:o}",
            path.display()
        );
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn tighten(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}
