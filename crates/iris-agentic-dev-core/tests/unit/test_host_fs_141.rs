//! Spec 141 T212, T216: the path rule behind `iris_compile`, `kb index` and `iris_symbols_local`,
//! and the crawl budgets of `iris_symbols_local`.
//!
//! The tools are exercised through the pipeline in `test_host_paths_141`; these tests pin the rule
//! on its own, with the directories and the working directory passed in.

use iris_agentic_dev_core::iris::config_sources::{LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::tools::host_fs::{
    confine_dir, confine_file, project_root, refusal_body, within, PathRefusal, COMPILE_EXTENSIONS,
};
use iris_agentic_dev_core::tools::symbols_local::{scan_workspace_budgeted, ScanBudget};
use std::path::{Path, PathBuf};

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap()
}

struct Dirs {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    proj: PathBuf,
    outside: PathBuf,
}

fn dirs() -> Dirs {
    let tmp = tempfile::tempdir().unwrap();
    let base = canon(tmp.path());
    let (home, proj, outside) = (base.join("home"), base.join("proj"), base.join("outside"));
    for d in [&home, &proj, &outside] {
        std::fs::create_dir_all(d).unwrap();
    }
    Dirs {
        _tmp: tmp,
        home,
        proj,
        outside,
    }
}

fn inputs(d: &Dirs, cwd: &Path) -> LoadInputs {
    LoadInputs {
        cwd: cwd.to_path_buf(),
        home: Some(d.home.clone()),
        ..Default::default()
    }
}

// ── the project directory ─────────────────────────────────────────────────────

#[test]
fn the_working_directory_is_the_project_when_nothing_else_names_one() {
    let d = dirs();
    assert_eq!(project_root(&inputs(&d, &d.proj)), Some(d.proj.clone()));
}

#[test]
fn a_workspace_file_above_the_working_directory_names_the_project() {
    let d = dirs();
    std::fs::write(d.proj.join(CONFIG_FILE_NAME), "").unwrap();
    let deep = d.proj.join("src").join("deep");
    std::fs::create_dir_all(&deep).unwrap();
    assert_eq!(project_root(&inputs(&d, &deep)), Some(d.proj.clone()));
}

#[test]
fn the_environment_and_the_flag_name_the_project_before_the_working_directory() {
    let d = dirs();
    let mut i = inputs(&d, &d.outside);
    i.env_workspace = Some(d.proj.to_string_lossy().to_string());
    assert_eq!(project_root(&i), Some(d.proj.clone()));
    let mut i = inputs(&d, &d.outside);
    i.workspace_flag = Some(d.proj.to_string_lossy().to_string());
    assert_eq!(project_root(&i), Some(d.proj.clone()));
}

#[test]
fn a_symlinked_project_directory_is_made_canonical() {
    let d = dirs();
    let link = d.outside.join("link");
    std::os::unix::fs::symlink(&d.proj, &link).unwrap();
    let mut i = inputs(&d, &d.outside);
    i.env_workspace = Some(link.to_string_lossy().to_string());
    assert_eq!(project_root(&i), Some(d.proj.clone()));
}

#[test]
fn there_is_no_project_in_the_home_directory_the_filesystem_root_or_a_missing_one() {
    let d = dirs();
    assert_eq!(project_root(&inputs(&d, &d.home)), None, "home");
    assert_eq!(project_root(&inputs(&d, Path::new("/"))), None, "root");
    let mut i = inputs(&d, &d.proj);
    i.env_workspace = Some(d.proj.join("nope").to_string_lossy().to_string());
    assert_eq!(project_root(&i), None, "missing");
    let mut i = inputs(&d, &d.proj);
    i.env_workspace = Some(d.proj.join("file").to_string_lossy().to_string());
    std::fs::write(d.proj.join("file"), "x").unwrap();
    assert_eq!(project_root(&i), None, "a file");
}

// ── files ─────────────────────────────────────────────────────────────────────

fn file(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, "Class A.B {}\n").unwrap();
    p
}

fn check(d: &Dirs, requested: &str) -> Result<PathBuf, PathRefusal> {
    confine_file(Some(&d.proj), &d.proj, requested, COMPILE_EXTENSIONS)
}

#[test]
fn a_source_file_inside_the_project_is_allowed_and_comes_back_canonical() {
    let d = dirs();
    for name in ["A.cls", "A.mac", "A.inc", "A.int", "UPPER.CLS", "Mixed.Mac"] {
        let f = file(&d.proj, name);
        assert_eq!(check(&d, f.to_str().unwrap()), Ok(f.clone()), "{name}");
    }
}

#[test]
fn a_relative_path_is_taken_from_the_working_directory() {
    let d = dirs();
    std::fs::create_dir_all(d.proj.join("src")).unwrap();
    let f = file(&d.proj.join("src"), "A.cls");
    assert_eq!(check(&d, "src/A.cls"), Ok(f.clone()));
    assert_eq!(
        confine_file(
            Some(&d.proj),
            &d.proj.join("src"),
            "A.cls",
            COMPILE_EXTENSIONS
        ),
        Ok(f)
    );
    // From another working directory the same text names another place.
    assert_eq!(
        confine_file(Some(&d.proj), &d.outside, "A.cls", COMPILE_EXTENSIONS),
        Err(PathRefusal::OutsideRoot)
    );
}

#[test]
fn an_absolute_path_outside_the_project_is_refused_whether_or_not_it_exists() {
    let d = dirs();
    let there = file(&d.outside, "A.cls");
    assert_eq!(
        check(&d, there.to_str().unwrap()),
        Err(PathRefusal::OutsideRoot)
    );
    let ghost = d.outside.join("Ghost.cls");
    assert_eq!(
        check(&d, ghost.to_str().unwrap()),
        Err(PathRefusal::OutsideRoot)
    );
    assert_eq!(
        check(&d, "/nonexistent_141/A.cls"),
        Err(PathRefusal::OutsideRoot)
    );
}

#[test]
fn a_parent_segment_is_refused_even_when_the_path_lands_inside() {
    let d = dirs();
    file(&d.proj, "A.cls");
    std::fs::create_dir_all(d.proj.join("sub")).unwrap();
    for text in [
        format!("{}/sub/../A.cls", d.proj.display()),
        "../proj/A.cls".to_string(),
        "..\\proj\\A.cls".to_string(),
        "sub/../A.cls".to_string(),
    ] {
        assert_eq!(
            check(&d, &text),
            Err(PathRefusal::ParentReference),
            "{text}"
        );
    }
    // `..` inside a name is not a segment.
    file(&d.proj, "a..b.cls");
    assert!(check(&d, "a..b.cls").is_ok());
}

#[test]
fn only_source_extensions_are_accepted_and_the_extension_is_judged_first() {
    let d = dirs();
    for name in ["notes.txt", "A.cls.bak", "A", "A.xml", ".cls"] {
        let f = file(&d.proj, name);
        assert_eq!(
            check(&d, f.to_str().unwrap()),
            Err(PathRefusal::Extension),
            "{name}"
        );
    }
    // Outside and not source: the extension answers, so the reply never says whether it exists.
    assert_eq!(check(&d, "/etc/hosts"), Err(PathRefusal::Extension));
    assert_eq!(check(&d, "/etc/nope_141"), Err(PathRefusal::Extension));
}

#[test]
fn a_missing_file_or_a_directory_inside_the_project_is_not_found() {
    let d = dirs();
    assert_eq!(
        check(&d, d.proj.join("Missing.cls").to_str().unwrap()),
        Err(PathRefusal::NotFound)
    );
    std::fs::create_dir_all(d.proj.join("Dir.cls")).unwrap();
    assert_eq!(
        check(&d, d.proj.join("Dir.cls").to_str().unwrap()),
        Err(PathRefusal::NotFound)
    );
    assert_eq!(
        check(&d, d.proj.join("nodir/Missing.cls").to_str().unwrap()),
        Err(PathRefusal::NotFound)
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_is_judged_by_where_it_points() {
    let d = dirs();
    let secret = file(&d.outside, "Secret.cls");
    let out = d.proj.join("Out.cls");
    std::os::unix::fs::symlink(&secret, &out).unwrap();
    assert_eq!(
        check(&d, out.to_str().unwrap()),
        Err(PathRefusal::OutsideRoot)
    );

    let real = file(&d.proj, "Real.cls");
    let inside = d.proj.join("Inside.cls");
    std::os::unix::fs::symlink(&real, &inside).unwrap();
    assert_eq!(check(&d, inside.to_str().unwrap()), Ok(real));

    // A link to a file that is not source, inside the project.
    let text = file(&d.proj, "plain.txt");
    let disguised = d.proj.join("Disguised.cls");
    std::os::unix::fs::symlink(&text, &disguised).unwrap();
    assert_eq!(
        check(&d, disguised.to_str().unwrap()),
        Err(PathRefusal::Extension)
    );

    // A link to a directory outside, and a dangling link.
    std::os::unix::fs::symlink(&d.outside, d.proj.join("dir")).unwrap();
    file(&d.outside, "Deep.cls");
    assert_eq!(
        check(&d, d.proj.join("dir/Deep.cls").to_str().unwrap()),
        Err(PathRefusal::OutsideRoot)
    );
    let dangling = d.proj.join("Dangling.cls");
    std::os::unix::fs::symlink(d.outside.join("gone.cls"), &dangling).unwrap();
    // Nothing is behind it, so nothing can be read: it is "not found", not a leak.
    assert_eq!(
        check(&d, dangling.to_str().unwrap()),
        Err(PathRefusal::NotFound)
    );
}

#[test]
fn with_no_project_every_path_is_refused() {
    let d = dirs();
    let f = file(&d.proj, "A.cls");
    assert_eq!(
        confine_file(None, &d.proj, f.to_str().unwrap(), COMPILE_EXTENSIONS),
        Err(PathRefusal::NoRoot)
    );
    assert_eq!(
        confine_dir(None, &d.proj, d.proj.to_str().unwrap()),
        Err(PathRefusal::NoRoot)
    );
}

// ── directories ───────────────────────────────────────────────────────────────

#[test]
fn a_directory_inside_the_project_is_allowed_and_so_is_the_project() {
    let d = dirs();
    std::fs::create_dir_all(d.proj.join("docs")).unwrap();
    for text in [d.proj.to_str().unwrap(), "docs", "./docs", "."] {
        let want = if text.contains("docs") {
            d.proj.join("docs")
        } else {
            d.proj.clone()
        };
        assert_eq!(
            confine_dir(Some(&d.proj), &d.proj, text),
            Ok(want),
            "{text}"
        );
    }
}

#[test]
fn a_directory_outside_with_a_parent_segment_or_a_link_out_is_refused() {
    let d = dirs();
    let r = Some(d.proj.as_path());
    assert_eq!(
        confine_dir(r, &d.proj, d.outside.to_str().unwrap()),
        Err(PathRefusal::OutsideRoot)
    );
    assert_eq!(confine_dir(r, &d.proj, "/"), Err(PathRefusal::OutsideRoot));
    assert_eq!(
        confine_dir(r, &d.proj, d.home.to_str().unwrap()),
        Err(PathRefusal::OutsideRoot)
    );
    assert_eq!(
        confine_dir(r, &d.proj, ".."),
        Err(PathRefusal::ParentReference)
    );
    assert_eq!(
        confine_dir(r, &d.proj, "docs/../.."),
        Err(PathRefusal::ParentReference)
    );
    std::os::unix::fs::symlink(&d.outside, d.proj.join("out")).unwrap();
    assert_eq!(
        confine_dir(r, &d.proj, "out"),
        Err(PathRefusal::OutsideRoot)
    );
    assert_eq!(
        confine_dir(r, &d.proj, "missing"),
        Err(PathRefusal::NotFound)
    );
    file(&d.proj, "f.cls");
    assert_eq!(confine_dir(r, &d.proj, "f.cls"), Err(PathRefusal::NotFound));
}

#[test]
fn within_follows_a_listed_entry_and_drops_one_that_leaves() {
    let d = dirs();
    let real = file(&d.proj, "a.md");
    let secret = file(&d.outside, "s.md");
    std::os::unix::fs::symlink(&secret, d.proj.join("b.md")).unwrap();
    assert_eq!(within(&d.proj, &real), Some(real));
    assert_eq!(within(&d.proj, &d.proj.join("b.md")), None);
    assert_eq!(within(&d.proj, &d.proj.join("missing.md")), None);
}

// ── the refusal ───────────────────────────────────────────────────────────────

#[test]
fn the_refusal_body_names_the_code_the_reason_and_the_way_out() {
    for (why, reason) in [
        (PathRefusal::NoRoot, "no_workspace"),
        (PathRefusal::ParentReference, "parent_reference"),
        (PathRefusal::OutsideRoot, "outside_workspace"),
        (PathRefusal::Extension, "extension"),
    ] {
        let b = refusal_body("iris_compile", "/x/y.cls", &why, COMPILE_EXTENSIONS);
        assert_eq!(b["success"], false);
        assert_eq!(b["error_code"], "HOST_PATH_BLOCKED");
        assert_eq!(b["reason"], reason);
        assert!(b["error"].as_str().unwrap().contains("/x/y.cls"), "{b}");
    }
    let b = refusal_body(
        "iris_compile",
        "a.txt",
        &PathRefusal::Extension,
        COMPILE_EXTENSIONS,
    );
    assert_eq!(
        b["allowed_extensions"],
        serde_json::json!(["cls", "mac", "inc", "int"])
    );
}

// ── iris_symbols_local budgets ────────────────────────────────────────────────

fn tree(d: &Dirs, files: usize, depth: usize) {
    for i in 0..files {
        std::fs::write(
            d.proj.join(format!("C{i}.cls")),
            format!("Class Budget{i} {{\n}}\n"),
        )
        .unwrap();
    }
    let mut dir = d.proj.clone();
    for level in 0..depth {
        dir = dir.join(format!("d{level}"));
    }
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Deep.cls"), "Class Budget.Deep {\n}\n").unwrap();
}

fn budget(entries: usize, depth: usize, secs: u64) -> ScanBudget {
    ScanBudget {
        max_entries: entries,
        max_depth: depth,
        max_time: std::time::Duration::from_secs(secs),
    }
}

#[test]
fn a_crawl_inside_its_budget_is_not_truncated() {
    let d = dirs();
    tree(&d, 5, 2);
    let r = scan_workspace_budgeted(&d.proj, "Budget*", 500, None, ScanBudget::default());
    assert_eq!(r.truncated, None);
    assert_eq!(r.symbols.len(), 6, "{:?}", r.symbols);
}

#[test]
fn a_crawl_stops_at_the_entry_budget_and_says_so() {
    let d = dirs();
    tree(&d, 30, 1);
    let r = scan_workspace_budgeted(&d.proj, "Budget*", 500, None, budget(10, 32, 60));
    assert_eq!(r.truncated, Some("file_budget"));
    assert!(r.symbols.len() <= 10, "{}", r.symbols.len());
}

#[test]
fn a_crawl_stops_descending_at_the_depth_budget_and_says_so() {
    let d = dirs();
    tree(&d, 1, 6);
    let r = scan_workspace_budgeted(&d.proj, "Budget*", 500, None, budget(1000, 3, 60));
    assert_eq!(r.truncated, Some("depth_budget"));
    assert!(
        r.symbols.iter().all(|s| s.name != "Budget.Deep"),
        "the class six levels down is not read: {:?}",
        r.symbols
    );
    assert!(r.symbols.iter().any(|s| s.name == "Budget0"));
}

#[test]
fn a_crawl_stops_at_the_time_budget_and_says_so() {
    let d = dirs();
    tree(&d, 20, 1);
    let r = scan_workspace_budgeted(&d.proj, "Budget*", 500, None, budget(1000, 32, 0));
    // A zero budget has always run out by the second entry.
    assert_eq!(r.truncated, Some("time_budget"));
}

#[test]
fn stopping_at_the_symbol_limit_is_not_truncation() {
    let d = dirs();
    tree(&d, 10, 1);
    let r = scan_workspace_budgeted(&d.proj, "Budget*", 3, None, ScanBudget::default());
    assert_eq!(r.symbols.len(), 3);
    assert_eq!(r.truncated, None);
}
