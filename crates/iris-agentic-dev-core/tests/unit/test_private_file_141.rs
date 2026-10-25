//! Spec 141 FR-023, FR-026, FR-028 (T-R4b-U11): files that hold a credential or a log of calls are
//! owner-only, and a write never goes through a symlink planted at a name the writer picks.

use iris_agentic_dev_core::iris::private_file::{
    append_private, write_atomic, write_private_atomic,
};
use std::path::Path;
use tempfile::TempDir;

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
#[test]
fn private_write_creates_a_0600_file_in_a_0700_directory() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cfg").join("servers.json");
    write_private_atomic(&path, b"{}").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"{}");
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(path.parent().unwrap()), 0o700);
}

#[cfg(unix)]
#[test]
fn private_write_tightens_an_existing_loose_file_and_directory() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("cfg");
    std::fs::create_dir(&sub).unwrap();
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = sub.join("servers.json");
    std::fs::write(&path, "old").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    write_private_atomic(&path, b"new").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&sub), 0o700);
}

#[cfg(unix)]
#[test]
fn write_replaces_a_symlinked_target_instead_of_writing_through_it() {
    let dir = TempDir::new().unwrap();
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "keep").unwrap();
    let target = dir.path().join("iris-agentic-dev.lock");
    std::os::unix::fs::symlink(&outside, &target).unwrap();

    write_atomic(&target, b"lock", 0o644).unwrap();

    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "lock");
    assert!(
        !std::fs::symlink_metadata(&target)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the target must be a regular file afterwards"
    );
    assert_eq!(mode(&target), 0o644);
}

#[cfg(unix)]
#[test]
fn a_planted_symlink_at_the_temp_name_is_not_followed() {
    // The temp name carries a random part, so a planted link cannot match it. Plant links at
    // every plausible fixed name and assert none of them is written through.
    let dir = TempDir::new().unwrap();
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "keep").unwrap();
    for name in ["servers.json.tmp", "servers.tmp", ".servers.json.tmp"] {
        std::os::unix::fs::symlink(&outside, dir.path().join(name)).unwrap();
    }
    write_private_atomic(&dir.path().join("servers.json"), b"data").unwrap();
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
}

#[cfg(unix)]
#[test]
fn write_leaves_no_temp_file_behind() {
    let dir = TempDir::new().unwrap();
    write_private_atomic(&dir.path().join("a.json"), b"x").unwrap();
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["a.json"]);
}

#[cfg(unix)]
#[test]
fn append_creates_0600_in_0700_and_tightens_an_existing_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("log").join("audit.jsonl");
    append_private(&path, "one").unwrap();
    append_private(&path, "two").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(path.parent().unwrap()), 0o700);

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    append_private(&path, "three").unwrap();
    assert_eq!(mode(&path), 0o600);
}

#[cfg(unix)]
#[test]
fn append_refuses_a_symlinked_log() {
    let dir = TempDir::new().unwrap();
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "keep").unwrap();
    let log = dir.path().join("audit.jsonl");
    std::os::unix::fs::symlink(&outside, &log).unwrap();

    let err = append_private(&log, "entry").unwrap_err();
    assert!(
        err.to_string().contains("symlink"),
        "the refusal must say why: {err}"
    );
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
}

#[test]
fn write_atomic_overwrites_a_regular_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("f.txt");
    write_atomic(&path, b"one", 0o644).unwrap();
    write_atomic(&path, b"two", 0o644).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"two");
}

#[cfg(unix)]
#[test]
fn servers_json_is_saved_owner_only() {
    use iris_agentic_dev_core::iris::servers_config::{save_to_path, ServersConfig};
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("iris-agentic-dev").join("servers.json");
    save_to_path(&ServersConfig::default(), &path).unwrap();
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(path.parent().unwrap()), 0o700);
}
