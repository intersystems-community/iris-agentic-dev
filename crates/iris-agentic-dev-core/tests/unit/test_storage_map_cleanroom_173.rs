//! Spec 173, T063 (FR-031): the repository is public and the work is clean-room. Nothing 173 adds
//! names a source-control system, a depot path, an internal collation call, a compiler routine or
//! a macro from the compiler's include files. The scan reads the spec folder, the fixtures, the new
//! source files, the doc and the skill.
//!
//! The words are written here in pieces, so that a search of the repository for one of them finds
//! only a real leak and not this list. The scan skips this file and `tasks.md`, which talks about
//! the rule and so names what it forbids.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A forbidden pattern: what it is called in a failure message, and its case-insensitive regex.
struct Forbidden {
    kind: &'static str,
    pattern: String,
}

fn forbidden() -> Vec<Forbidden> {
    let f = |kind, parts: &[&str]| Forbidden {
        kind,
        pattern: format!("(?i){}", parts.concat()),
    };
    vec![
        // A source-control system's name, its command and its words for a change.
        f("source-control word", &[r"\bper", "force\\b"]),
        f(
            "source-control word",
            &[
                r"\bp",
                "4(v|\\s+(sync|edit|submit|opened|files|filelog))\\b",
            ],
        ),
        f("source-control word", &[r"\bchange", "list\\b"]),
        // A depot path.
        f("depot path", &["//de", "pot/"]),
        f("depot path", &[r"\bde", r"pot\b"]),
        // The server's internal collation and conversion calls; the documented functions of
        // `%SYSTEM.SQL.Functions` are the way in.
        f("internal collation call", &[r"\$z", r"u\s*\("]),
        f("internal collation call", &[r"\$z", "cvt"]),
        f("internal collation call", &[r"\$z", "convert"]),
        // The names of compiler routines and classes.
        f("compiler routine", &["%o", "cc[A-Za-z]"]),
        f("compiler routine", &["%q", "aq[A-Za-z0-9]*"]),
        f("compiler routine", &["%q", "cr[A-Za-z0-9]*"]),
        f("compiler routine", &["%q", "rs[A-Za-z0-9]*"]),
    ]
}

/// The public status macros of the documented status include; every other `$$$` macro in these
/// files would come from the compiler's own include files.
const PUBLIC_MACROS: [&str; 4] = ["OK", "ISOK", "ISERR", "ERROR"];

/// What in `text` is forbidden: `(kind, matched text)`.
fn hits(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for fb in forbidden() {
        let re = regex::Regex::new(&fb.pattern).unwrap();
        for m in re.find_iter(text) {
            found.push((fb.kind.to_string(), m.as_str().to_string()));
        }
    }
    let macros = regex::Regex::new(r"\$\$\$([A-Za-z]+)").unwrap();
    for c in macros.captures_iter(text) {
        if !PUBLIC_MACROS.contains(&&c[1]) {
            found.push(("compiler macro".to_string(), c[0].to_string()));
        }
    }
    found
}

/// Every file under `dir`, recursively, that is not this test or `tasks.md`.
fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let p = e.unwrap().path();
        if p.is_dir() {
            files_under(&p, out);
        } else if skipped(&p) {
            continue;
        } else {
            out.push(p);
        }
    }
}

fn skipped(p: &Path) -> bool {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name == "tasks.md" || name == "test_storage_map_cleanroom_173.rs"
}

/// The 173 source files: the modules the linker, the candidates, the master, the typing, the
/// bitmaps and the usability line added.
const NEW_SOURCES: [&str; 14] = [
    "bitmap.rs",
    "bitmap_run.rs",
    "candidates.rs",
    "candidates_run.rs",
    "link.rs",
    "link_code.rs",
    "link_run.rs",
    "link_verify.rs",
    "master.rs",
    "propose_class.rs",
    "typing.rs",
    "typing_run.rs",
    "usability.rs",
    "usability_run.rs",
];

fn scanned_files() -> Vec<PathBuf> {
    let root = root();
    let mut files = Vec::new();
    files_under(&root.join("specs/173-propose-index-linking"), &mut files);
    files_under(
        &root.join("crates/iris-agentic-dev-core/tests/fixtures/storage173"),
        &mut files,
    );
    for name in NEW_SOURCES {
        let p = root
            .join("crates/iris-agentic-dev-core/src/tools/storage_map")
            .join(name);
        assert!(p.is_file(), "a 173 source file is missing: {}", p.display());
        files.push(p);
    }
    files.push(root.join("docs/storage-maps.md"));
    files.push(root.join("skills/skills/iris-storage-mapping/SKILL.md"));
    files
}

/// What the scan reports for `files`: one line per file and forbidden word.
fn leaks_in(files: &[PathBuf]) -> Vec<String> {
    let mut leaks = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        for (kind, word) in hits(&text) {
            leaks.push(format!("{}: {kind} {word:?}", f.display()));
        }
    }
    leaks
}

#[test]
fn test_storage_map_cleanroom_173_no_file_names_a_forbidden_word() {
    let files = scanned_files();
    assert!(files.len() >= 80, "only {} files scanned", files.len());
    for must in [
        "docs/storage-maps.md",
        "spec.md",
        "link.rs",
        "F01.json",
        "SKILL.md",
    ] {
        assert!(
            files.iter().any(|f| f.ends_with(must)),
            "{must} is not among the scanned files"
        );
    }
    assert!(
        !files.iter().any(|f| skipped(f)),
        "a skipped file is in the list"
    );
    let leaks = leaks_in(&files);
    assert!(
        leaks.is_empty(),
        "clean-room breaches:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn test_storage_map_cleanroom_173_a_planted_word_in_a_temp_copy_is_caught() {
    let dir = tempfile::tempdir().unwrap();
    // One planted line per kind of word, written from the same pieces the scan uses.
    let planted = [
        ("source-control word", ["per", "force sync"].concat()),
        ("source-control word", ["p", "4 edit"].concat()),
        ("depot path", ["//de", "pot/main/x.cls"].concat()),
        ("internal collation call", ["$z", "u(28,1)"].concat()),
        ("internal collation call", ["$z", "cvt(x,\"U\")"].concat()),
        ("compiler routine", ["%o", "ccInclude"].concat()),
        ("compiler routine", ["^%q", "aqplan"].concat()),
        ("compiler macro", ["$$", "$ClassIsAbstract(c)"].concat()),
    ];
    for (i, (kind, line)) in planted.iter().enumerate() {
        let p = dir.path().join(format!("planted{i}.md"));
        std::fs::write(&p, format!("A harmless sentence.\n{line}\nAnother one.\n")).unwrap();
        let leaks = leaks_in(&[p]);
        assert!(
            leaks.iter().any(|l| l.contains(kind)),
            "a planted {kind} ({line}) was not caught: {leaks:?}"
        );
    }
    // The copy without a planted word is clean, and so are the public status macros.
    let clean = "Set tSC = $$$OK\nIf $$$ISERR(tSC) { Quit }\nuses %SYSTEM.SQL.Functions.SQLUPPER\n";
    let p = dir.path().join("clean.md");
    std::fs::write(&p, clean).unwrap();
    assert_eq!(leaks_in(&[p]), Vec::<String>::new());
}
