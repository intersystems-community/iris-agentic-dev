//! Security fix S1 to S3: a workspace file's `mode` is a loosening setting, and the walk up from
//! the working directory and the project directory stop at places that are not a project.
//!
//! Every case passes `LoadInputs` with a temporary home; none reads the developer's own
//! `~/.iris-agentic-dev.toml` or the process environment.

use iris_agentic_dev_core::iris::config_sources::{
    is_broad_root, loosening_settings, ConfigSnapshot, LoadInputs, SetAsideKind, TrustState,
    CONFIG_FILE_NAME,
};
use iris_agentic_dev_core::iris::operator_hold::hold_loosening;
use iris_agentic_dev_core::tools::host_fs::project_root;
use std::path::{Path, PathBuf};

fn table(s: &str) -> toml::Table {
    s.parse().unwrap_or_else(|e| panic!("{e}: {s}"))
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap()
}

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

const OPERATOR: &str = r#"
mode = "operate"

[instance.prod]
host = "127.0.0.1"
web_port = 1
role = "subject"
"#;

struct Env {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    proj: PathBuf,
}

fn env(operator: Option<&str>, workspace: &str) -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let root = canon(tmp.path());
    let (home, proj) = (root.join("home"), root.join("proj"));
    std::fs::create_dir_all(&home).unwrap();
    if let Some(op) = operator {
        write(&home, CONFIG_FILE_NAME, op);
    }
    write(&proj, CONFIG_FILE_NAME, workspace);
    Env {
        _tmp: tmp,
        home,
        proj,
    }
}

impl Env {
    fn load(&self) -> ConfigSnapshot {
        ConfigSnapshot::load(&LoadInputs {
            cwd: self.proj.clone(),
            home: Some(self.home.clone()),
            ..Default::default()
        })
    }
}

// ── S1 ──────────────────────────────────────────────────────────────────────────────────────

#[test]
fn a_mode_other_than_operate_is_a_loosening_setting() {
    for mode in ["dev", "develop", "workbench", "Operate", ""] {
        let t = table(&format!("mode = {mode:?}\n"));
        assert!(
            loosening_settings(&t)
                .iter()
                .any(|l| l == &format!("mode = {mode:?}")),
            "mode {mode:?} is not listed: {:?}",
            loosening_settings(&t)
        );
    }
    assert!(
        loosening_settings(&table("mode = \"operate\"\n")).is_empty(),
        "operate tightens: it is not a loosening setting"
    );
    assert!(loosening_settings(&table("namespace = \"USER\"\n")).is_empty());
}

#[test]
fn an_untrusted_workspace_mode_cannot_move_an_operate_fleet_to_workbench() {
    for ws in [
        "mode = \"dev\"\n",
        "mode = \"develop\"\n",
        "mode = \"Operate\"\n",
        "mode = \"\"\n",
    ] {
        let e = env(Some(OPERATOR), ws);
        let snap = e.load();
        assert_eq!(
            snap.fleet.mode.as_deref(),
            Some("operate"),
            "workspace file {ws:?} changed the operator's mode"
        );
        assert!(snap.fleet.instance.contains_key("prod"));
        assert_eq!(snap.trust(), TrustState::Untrusted, "{ws:?}");
        let item = snap
            .set_aside
            .iter()
            .find(|i| i.id == "workspace:mode")
            .unwrap_or_else(|| panic!("{ws:?}: no workspace:mode in {:?}", snap.set_aside));
        assert_eq!(item.kind, SetAsideKind::UntrustedSetting);
        assert!(item.setting.starts_with("mode = "), "{}", item.setting);
        assert!(
            item.lift.contains("iris-agentic-dev trust"),
            "{}",
            item.lift
        );
    }
}

#[test]
fn a_trusted_workspace_mode_applies() {
    let e = env(None, "mode = \"dev\"\n");
    let ws_path = canon(&e.proj.join(CONFIG_FILE_NAME));
    let lines = loosening_settings(&table("mode = \"dev\"\n"));
    write(
        &e.home,
        CONFIG_FILE_NAME,
        &format!(
            "mode = \"operate\"\n[[trust]]\npath = {:?}\nformat = 1\nsettings = {:?}\n",
            ws_path.to_string_lossy(),
            lines
        ),
    );
    let snap = e.load();
    assert_eq!(snap.trust(), TrustState::Trusted, "{:?}", snap.trust());
    assert_eq!(snap.fleet.mode.as_deref(), Some("dev"));
    assert!(
        !snap.set_aside.iter().any(|i| i.id == "workspace:mode"),
        "{:?}",
        snap.set_aside
    );
}

#[test]
fn a_workspace_mode_that_tightens_still_applies_untrusted() {
    // The operator file says nothing, so the fleet is a workbench; the workspace file asks for
    // operate and a subject server. Tightening needs no trust.
    let e = env(
        None,
        "mode = \"operate\"\n[instance.prod]\nhost = \"127.0.0.1\"\nweb_port = 1\nrole = \"subject\"\n",
    );
    let snap = e.load();
    assert_eq!(snap.fleet.mode.as_deref(), Some("operate"));
    assert!(
        !snap.set_aside.iter().any(|i| i.id == "workspace:mode"),
        "{:?}",
        snap.set_aside
    );
    assert_eq!(snap.trust(), TrustState::NotNeeded);
}

#[test]
fn the_operator_file_keeps_its_own_mode_without_trust() {
    let e = env(Some("mode = \"dev\"\n"), "namespace = \"USER\"\n");
    let snap = e.load();
    assert_eq!(snap.fleet.mode.as_deref(), Some("dev"));
    assert!(snap.set_aside.is_empty(), "{:?}", snap.set_aside);
}

#[test]
fn an_edit_to_the_operator_mode_that_loosens_waits_for_a_restart() {
    let applied = table("mode = \"operate\"\n[instance.prod]\nrole = \"subject\"\n");
    for incoming in [
        "mode = \"dev\"\n[instance.prod]\nrole = \"subject\"\n",
        "[instance.prod]\nrole = \"subject\"\n",
    ] {
        let (out, held) = hold_loosening(&applied, table(incoming));
        assert_eq!(
            out.get("mode").and_then(|v| v.as_str()),
            Some("operate"),
            "{incoming:?} was applied"
        );
        let ids: Vec<_> = held.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, vec!["operator:mode"], "{incoming:?}");
    }
}

#[test]
fn an_edit_to_the_operator_mode_that_tightens_or_does_nothing_applies() {
    for (applied, incoming) in [
        ("namespace = \"USER\"\n", "mode = \"operate\"\n"),
        ("mode = \"dev\"\n", "mode = \"operate\"\n"),
        ("mode = \"operate\"\n", "mode = \"operate\"\n"),
        ("mode = \"dev\"\n", "mode = \"develop\"\n"),
        ("namespace = \"USER\"\n", "mode = \"dev\"\n"),
    ] {
        let (out, held) = hold_loosening(&table(applied), table(incoming));
        assert!(held.is_empty(), "{applied:?} -> {incoming:?} held {held:?}");
        assert_eq!(out, table(incoming));
    }
}

#[test]
fn a_deleted_operator_file_does_not_drop_the_fleet_mode_mid_session() {
    let tmp = tempfile::tempdir().unwrap();
    let root = canon(tmp.path());
    let (home, proj) = (root.join("home"), root.join("proj"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: proj,
        home: Some(home),
        operator_baseline: Some(table(OPERATOR)),
        ..Default::default()
    });
    assert_eq!(snap.fleet.mode.as_deref(), Some("operate"));
    assert!(snap.held.iter().any(|h| h.id == "operator:mode"));
}

// ── S3 ──────────────────────────────────────────────────────────────────────────────────────

#[test]
fn broad_roots_are_not_projects() {
    let home = Path::new("/Users/someone");
    for broad in [
        "/",
        "/Users",
        "/home",
        "/tmp",
        "/private",
        "/private/tmp",
        "/var/tmp",
        "/private/var/tmp",
    ] {
        assert!(is_broad_root(Path::new(broad), Some(home)), "{broad}");
        assert!(is_broad_root(Path::new(broad), None), "{broad} (no home)");
    }
    // Case does not matter on the platforms where the filesystem ignores it.
    assert!(is_broad_root(Path::new("/users"), Some(home)));
    // Whatever holds the user's home holds user homes.
    assert!(is_broad_root(
        Path::new("/srv/people"),
        Some(Path::new("/srv/people/ann"))
    ));
    assert!(is_broad_root(
        Path::new("/srv"),
        Some(Path::new("/srv/people/ann"))
    ));

    for ok in [
        "/Users/someone/proj",
        "/tmp/proj",
        "/home/someone/proj",
        "/srv/app",
        "/var/folders/ab/cd/T/proj",
        "/private/tmp/proj",
    ] {
        assert!(!is_broad_root(Path::new(ok), Some(home)), "{ok}");
    }
}

/// Shared and mounted places hold other people's files: the shared user directories, and the mount
/// roots with the mount points directly under them.
#[test]
fn shared_directories_and_mount_roots_are_not_projects() {
    let home = Path::new("/Users/someone");
    for broad in [
        "/Users/Shared",
        "/users/shared",
        "/Users/Public",
        "C:\\Users\\Public",
        "c:\\users\\public",
        "/Volumes",
        "/Volumes/Macintosh HD",
        "/Volumes/work",
        "/mnt",
        "/mnt/c",
        "/mnt/data",
        "/media",
        "/media/alice",
    ] {
        assert!(is_broad_root(Path::new(broad), Some(home)), "{broad}");
        assert!(is_broad_root(Path::new(broad), None), "{broad} (no home)");
    }
    // A real project on a mount, more than one level below the mount point, still works, and so do
    // directories that only share a prefix with a broad root.
    for ok in [
        "/Volumes/work/proj",
        "/Volumes/work/proj/sub",
        "/mnt/data/proj",
        "/media/alice/usb/proj",
        "/Users/Shared/proj",
        "/Users/Public/proj",
        "/Volumesx",
        "/mntx/a",
        "/srv/Volumes",
        "/Users/someone/Volumes",
        "C:\\Users\\Public\\proj",
        "D:\\work",
    ] {
        // `/Users/Shared/proj` and `/Users/Public/proj` are shared places too, but a directory
        // below them is a project the user made there; only the shared root itself is refused.
        assert!(!is_broad_root(Path::new(ok), Some(home)), "{ok}");
    }
}

#[test]
fn a_working_directory_that_is_a_broad_root_is_no_project() {
    let tmp = tempfile::tempdir().unwrap();
    let root = canon(tmp.path());
    let home = root.join("people").join("ann");
    std::fs::create_dir_all(&home).unwrap();
    // `/`, `/tmp` and the directory that holds the home all exist here.
    for cwd in [
        PathBuf::from("/"),
        canon(Path::new("/tmp")),
        root.join("people"),
        root.clone(),
    ] {
        let inputs = LoadInputs {
            cwd: cwd.clone(),
            home: Some(home.clone()),
            ..Default::default()
        };
        assert_eq!(
            project_root(&inputs),
            None,
            "{} became the project directory",
            cwd.display()
        );
    }
    // A directory of its own is one.
    let proj = root.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let inputs = LoadInputs {
        cwd: proj.clone(),
        home: Some(home),
        ..Default::default()
    };
    assert_eq!(project_root(&inputs), Some(proj));
}

#[test]
fn a_broad_root_named_by_the_workspace_flag_is_no_project() {
    let tmp = tempfile::tempdir().unwrap();
    let root = canon(tmp.path());
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    for ws in ["/", "/tmp"] {
        let inputs = LoadInputs {
            cwd: root.clone(),
            home: Some(home.clone()),
            workspace_flag: Some(ws.to_string()),
            ..Default::default()
        };
        assert_eq!(project_root(&inputs), None, "--workspace {ws}");
    }
}

// ── S2 ──────────────────────────────────────────────────────────────────────────────────────

fn load_from(home: &Path, cwd: &Path) -> ConfigSnapshot {
    ConfigSnapshot::load(&LoadInputs {
        cwd: canon(cwd),
        home: Some(canon(home)),
        ..Default::default()
    })
}

#[test]
fn the_walk_stops_at_the_repository_root() {
    let tmp = tempfile::tempdir().unwrap();
    let base = canon(tmp.path());
    let home = base.join("home");
    std::fs::create_dir_all(&home).unwrap();
    // A file in the directory that holds the repository, as a stranger could leave it.
    write(
        &base.join("outer"),
        CONFIG_FILE_NAME,
        "namespace = \"ABOVE\"\n",
    );
    let repo = base.join("outer").join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let deep = repo.join("src").join("x");
    std::fs::create_dir_all(&deep).unwrap();

    let snap = load_from(&home, &deep);
    assert!(
        snap.workspace_layer.is_none(),
        "the walk passed the repository root: {:?}",
        snap.workspace_layer.as_ref().map(|f| &f.path)
    );

    // The repository root's own file is read, from anywhere inside.
    write(&repo, CONFIG_FILE_NAME, "namespace = \"REPO\"\n");
    let snap = load_from(&home, &deep);
    let ws = snap.workspace_layer.as_ref().expect("the repo's file");
    assert_eq!(ws.path, canon(&repo.join(CONFIG_FILE_NAME)));
}

#[test]
fn a_git_file_marks_the_repository_root_too() {
    // A worktree or submodule has a `.git` file, not a directory.
    let tmp = tempfile::tempdir().unwrap();
    let base = canon(tmp.path());
    let home = base.join("home");
    std::fs::create_dir_all(&home).unwrap();
    write(
        &base.join("outer"),
        CONFIG_FILE_NAME,
        "namespace = \"ABOVE\"\n",
    );
    let repo = base.join("outer").join("wt");
    write(&repo, ".git", "gitdir: /elsewhere\n");
    let snap = load_from(&home, &repo);
    assert!(snap.workspace_layer.is_none(), "{:?}", snap.workspace_layer);
}

#[test]
fn the_walk_never_reads_above_the_users_home() {
    let tmp = tempfile::tempdir().unwrap();
    let base = canon(tmp.path());
    // `base` holds the home directory, so it is the stand-in for `/Users`; no repository.
    let home = base.join("people").join("ann");
    std::fs::create_dir_all(&home).unwrap();
    write(&base, CONFIG_FILE_NAME, "namespace = \"ROOT\"\n");
    write(
        &base.join("people"),
        CONFIG_FILE_NAME,
        "namespace = \"PEOPLE\"\n",
    );
    let proj = base.join("elsewhere").join("proj").join("src");
    std::fs::create_dir_all(&proj).unwrap();
    write(
        &base.join("elsewhere"),
        CONFIG_FILE_NAME,
        "namespace = \"SIBLING\"\n",
    );

    // The nearest file that is not above home: the sibling directory's.
    let snap = load_from(&home, &proj);
    let ws = snap
        .workspace_layer
        .as_ref()
        .expect("a file below the roots");
    assert_eq!(
        ws.path,
        canon(&base.join("elsewhere").join(CONFIG_FILE_NAME))
    );

    // With that gone the walk reaches the directory that holds the homes and stops.
    std::fs::remove_file(base.join("elsewhere").join(CONFIG_FILE_NAME)).unwrap();
    let snap = load_from(&home, &proj);
    assert!(
        snap.workspace_layer.is_none(),
        "read a file above home: {:?}",
        snap.workspace_layer.as_ref().map(|f| &f.path)
    );
}
