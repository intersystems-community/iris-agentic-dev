//! Spec 127: wording guards for skill claims that IRIS contradicted.
//!
//! Each test pins one correction from `specs/127-skill-fact-fixes/research.md`. It fails if the old
//! wrong wording comes back. The matching live test, showing IRIS doing what the corrected text
//! says, is in `tests/integration/test_skill_facts_127_live.rs`.

use std::path::PathBuf;

fn skills_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills/skills")
        .canonicalize()
        .expect("skills/skills must exist in the repo")
}

fn skill(name: &str) -> String {
    let path = skills_dir().join(name).join("SKILL.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// A file under `skills/` that is not a bundled skill (`compile.md`, `AGENTS.md`).
fn skills_file(rel: &str) -> String {
    let path = skills_dir().join("..").join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// US1. `e` is "Delete extent" in `ShowFlags` on 2026.2. The skill called it "display errors only"
/// and put it in a CI recipe, where `Delete` with it wipes a table.
#[test]
fn eval_skill_does_not_call_e_harmless() {
    let text = skill("iris-objectscript-eval");
    assert!(
        !text.contains("display errors only"),
        "iris-objectscript-eval still describes compile flag `e` as \"display errors only\""
    );
    assert!(
        text.contains("delete extent") || text.contains("Delete extent"),
        "iris-objectscript-eval must say what `e` does: delete the extent"
    );
}

/// US2.1. A postfix `Quit:key=""` sharing a line compiles. The space form fails with #1054, not
/// #5559.
#[test]
fn postconditional_claims_match_iris() {
    let loops = skill("objectscript-loop-patterns");
    assert!(
        !loops.contains("#5559"),
        "objectscript-loop-patterns still blames the postconditional forms on #5559"
    );
    assert!(
        !loops.contains("postfix Quit on same line as anything else"),
        "objectscript-loop-patterns still says a postfix Quit cannot share a line"
    );
    assert!(loops.contains("#1054"), "loop-patterns must name #1054");

    let fewshot = skill("objectscript-fewshot-fixes");
    assert!(
        !fewshot.contains("parser error #5559") && !fewshot.contains("causes #5559 parse error"),
        "objectscript-fewshot-fixes still says the spaced postconditional is #5559"
    );
    assert!(fewshot.contains("#1054"), "fewshot-fixes must name #1054");

    assert!(
        !skill("objectscript-guardrails").contains("alone on its own line"),
        "objectscript-guardrails still says a postfix Quit must be alone on its line"
    );
}

/// US2.2. `Quit <value>` in a loop compiles and fails at runtime with `<COMMAND>`; only inside
/// `Try` is it the compile error #1043.
#[test]
fn quit_value_in_loop_claims_match_iris() {
    let tdd = skill("objectscript-tdd");
    assert!(
        !tdd.contains("inside TRY/CATCH or loop "),
        "objectscript-tdd still lists a loop as a cause of the compile error"
    );
    assert!(
        tdd.contains("<COMMAND>"),
        "objectscript-tdd must name the runtime <COMMAND>"
    );

    let loops = skill("objectscript-loop-patterns");
    assert!(
        !loops.contains("method returns \"\"!") && !loops.contains("actually \"\""),
        "objectscript-loop-patterns still says Quit in a loop returns \"\""
    );
    assert!(
        loops.contains("<COMMAND>"),
        "loop-patterns must name <COMMAND>"
    );
    assert!(
        loops.contains("#1043"),
        "loop-patterns must name #1043 for Try"
    );

    let compile = skills_file("compile.md");
    assert!(
        !compile.contains("#5563") && !compile.contains("inside TRY/CATCH or loop"),
        "skills/compile.md still gives #5563 and blames loops for the Quit compile error"
    );
}

/// US2.3. Routines can use `Try/Catch` and `Return`; the checklist listed both as wrong.
#[test]
fn mac_routines_do_not_forbid_try_or_return() {
    let mac = skill("objectscript-mac-routines");
    assert!(
        !mac.contains("| Error trap             | `Try { } Catch e { }`  |"),
        "objectscript-mac-routines still lists Try/Catch as wrong in a routine"
    );
    assert!(
        !mac.contains("| Return from subroutine | `Return`               |"),
        "objectscript-mac-routines still lists Return as wrong in a routine"
    );
}

/// US2.4. `lst_$LB(x)` is the linear form; `$LIST(lst,*+1)` is the one that is slow in a loop.
#[test]
fn list_patterns_label_the_fast_form_correctly() {
    let list = skill("objectscript-list-patterns");
    assert!(
        !list.contains("WRONG — $LISTBUILD creates a new list each iteration"),
        "objectscript-list-patterns still labels concatenation as the O(n²) form"
    );
    assert!(
        list.contains("$LIST(lst, *+1)"),
        "list-patterns must show $LIST(lst, *+1) as the slow form"
    );
}

/// US2.5. The SQL skill follows its own last-dot rule: an underscore form of a two-level class
/// appears only on a line marked wrong, or right under one.
#[test]
fn sql_patterns_follow_their_own_naming_rule() {
    let sql = skill("objectscript-sql-patterns");
    let lines: Vec<&str> = sql.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        for bad in ["Catalog_Item", "Config_Setting", "Bench_Patient"] {
            if line.contains(bad) {
                let marked = |l: &str| l.to_lowercase().contains("wrong");
                assert!(
                    marked(line) || (i > 0 && marked(lines[i - 1])),
                    "objectscript-sql-patterns line {} uses {bad} as if it worked: {line}",
                    i + 1
                );
            }
        }
    }
}

/// US2.6. `%Execute(args...)` returns rows. The skill said `<STACK>` and told agents to
/// concatenate values into the SQL string, which is SQL injection.
#[test]
fn iris_sql_does_not_teach_in_list_concatenation() {
    let sql = skill("iris-sql");
    assert!(
        !sql.contains("<STACK> error!"),
        "iris-sql still says %Execute(args...) raises <STACK>"
    );
    assert!(
        !sql.contains("build the quoted values directly into the SQL string"),
        "iris-sql still recommends concatenating values into SQL"
    );
    assert!(
        !sql.contains("%Execute does NOT accept arrays"),
        "iris-sql still says %Execute does not accept arrays"
    );
    assert!(
        sql.contains("%Execute(args...)"),
        "iris-sql must show %Execute(args...)"
    );
}

/// US2.7. `New $Namespace` is the idiom; only `New` on a plain variable in a procedure block fails.
#[test]
fn review_new_rule_is_narrowed_to_plain_variables() {
    let review = skill("objectscript-review");
    assert!(
        !review.contains("No `New` command inside method/procedure blocks"),
        "objectscript-review still forbids every New"
    );
    assert!(
        review.contains("New $Namespace"),
        "review must allow New $Namespace"
    );
    assert!(review.contains("#1038"), "review must name #1038");
}

/// US2.8. A bare `TROLLBACK` rolls back every level, the caller's included.
#[test]
fn trollback_advice_rolls_back_one_level() {
    for name in ["objectscript-guardrails", "objectscript-review"] {
        let text = skill(name);
        assert!(
            !text.contains("`If $TLevel > 0 { TROLLBACK }`")
                && !text.contains("TRollback checks `$TLevel > 0` first"),
            "{name} still recommends a bare TROLLBACK"
        );
        assert!(
            text.contains("TROLLBACK:$TLevel>entry 1"),
            "{name} must show the one-level form"
        );
    }
    assert!(
        !skills_file("AGENTS.md").contains("If $TLevel > 0 { TRollback }"),
        "skills/AGENTS.md still recommends a bare TRollback"
    );
}

/// US2.9. An unquoted Python name with `_` is ObjectScript concatenation.
#[test]
fn embedded_python_quotes_underscore_names() {
    let py = skill("iris-embedded-python");
    assert!(
        !py.contains("pyobj.my_function(42)"),
        "iris-embedded-python still calls pyobj.my_function unquoted"
    );
    assert!(
        py.contains("pyobj.\"my_function\"(42)"),
        "must show the quoted form"
    );
}

/// US2.10. On 2026.2 a graceful stop leaves queued messages queued and a force stop requeues the
/// one in hand; neither loses a message. The skill said restarting drops in-flight messages.
#[test]
fn production_stop_is_not_described_as_losing_messages() {
    let text = skill("ensemble-production");
    for old in [
        "drops in-flight messages",
        "Restart loses in-flight messages",
        "Force-stop drops",
    ] {
        assert!(
            !text.contains(old),
            "ensemble-production still says {old:?}"
        );
    }
    assert!(
        text.contains("requeue"),
        "ensemble-production must say a force stop requeues the interrupted message"
    );
}

/// US3.2. #5477 is a compile error, a hand-written Storage block contradicts guardrails, and the
/// 31-character global limit does not exist (IRIS hashes long names).
#[test]
fn ensemble_production_storage_and_name_claims_match_iris() {
    let text = skill("ensemble-production");
    assert!(
        !text.contains("errors at runtime") && !text.contains("at runtime\n"),
        "ensemble-production still places #5477 at runtime"
    );
    assert!(
        !text.contains("<IdLocation>^Ens.MessageBodyD</IdLocation>"),
        "ensemble-production still shows a hand-written Storage block as the right form"
    );
    assert!(
        !text.contains("31-character limit"),
        "ensemble-production still claims a 31-character global name limit"
    );
}

/// US3.2. The skill routed around an `iris_production` refusal with `iris_execute`.
#[test]
fn ensemble_production_does_not_bypass_tool_refusal() {
    let text = skill("ensemble-production");
    assert!(
        !text.contains("Use `iris_execute` as the\nreliable workaround")
            && !text.contains("reliable workaround"),
        "ensemble-production still routes around iris_production through iris_execute"
    );
}

/// US3.3. The skill forbade curl on DocBook, then curled DocBook in its own re-scrape recipe.
#[test]
fn iris_docs_has_one_docbook_rule() {
    let text = skill("iris-docs");
    assert!(
        !text.contains("DO NOT use WebFetch or curl on DocBook URLs"),
        "iris-docs still forbids curl on DocBook beside a curl recipe"
    );
    assert!(
        !text.contains("often a 504"),
        "iris-docs still claims a 504 that was not reproduced"
    );
    assert!(
        text.contains("ALG-"),
        "iris-docs must keep the ALG-* meta tag recipe"
    );
}

/// FR-005. `iris_compile` compiles what the server holds; a path works only on the HTTP path, and
/// `*.cls` is a namespace wildcard, not the workspace. Skills push with `iris_doc(mode="put")`.
#[test]
fn no_skill_passes_a_local_path_to_iris_compile() {
    let mut files: Vec<PathBuf> = std::fs::read_dir(skills_dir())
        .unwrap()
        .filter_map(|e| {
            let p = e.ok()?.path().join("SKILL.md");
            p.is_file().then_some(p)
        })
        .collect();
    for e in std::fs::read_dir(skills_dir().join("..")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "md") {
            files.push(p);
        }
    }
    let mut bad = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        for (i, line) in text.lines().enumerate() {
            let Some(at) = line.find("iris_compile(target=\"") else {
                continue;
            };
            let target: String = line[at + 21..].chars().take_while(|c| *c != '"').collect();
            if target.contains('/') || target.contains('\\') || line.contains("in workspace") {
                bad.push(format!("{}:{}: {line}", f.display(), i + 1));
            }
        }
        assert!(
            !text.contains("always pass the `.cls` file path"),
            "{} tells the reader to pass a file path to iris_compile",
            f.display()
        );
    }
    assert!(
        bad.is_empty(),
        "iris_compile given a local path:\n{}",
        bad.join("\n")
    );
}

/// The text of one `## N.` section of a skill, heading included.
fn section(text: &str, number: u32) -> String {
    let head = format!("\n## {number}. ");
    let start = text
        .find(&head)
        .unwrap_or_else(|| panic!("no section {number}"));
    let rest = &text[start + 1..];
    let end = rest[1..].find("\n## ").map_or(rest.len(), |i| i + 1);
    rest[..end].to_string()
}

/// sql-3 (130 FR-019). `If SQLCODE` is false on 0, so a found row is never reported as missing.
/// The defect is that it fires on 100 and on every negative code alike, so an SQL error reads as
/// "not found".
#[test]
fn sql_patterns_section_3_blames_the_lumping_not_the_zero() {
    let s3 = section(&skill("objectscript-sql-patterns"), 3);
    assert!(
        !s3.contains("NOT FOUND when row EXISTS"),
        "sql-patterns §3 still says `If SQLCODE` reports a found row as missing"
    );
    assert!(
        s3.contains("fires on 100 and on every negative code"),
        "sql-patterns §3 must say what `If SQLCODE` gets wrong: 100 and errors in one branch"
    );
}

/// sql-5 (130 FR-019). `If SQLCODE < 0 { Return "" }` hands an SQL error back as an empty
/// answer. The section must surface it.
#[test]
fn sql_patterns_section_5_does_not_swallow_a_negative_sqlcode() {
    let s5 = section(&skill("objectscript-sql-patterns"), 5);
    for line in s5.lines() {
        let squashed: String = line.split_whitespace().collect();
        assert!(
            !squashed.contains("SQLCODE<0{Return\"\"}"),
            "sql-patterns §5 still returns \"\" on a negative SQLCODE: {line}"
        );
    }
    assert!(
        s5.contains("CreateFromSQLCODE"),
        "sql-patterns §5 must surface the error, e.g. %Exception.SQL.CreateFromSQLCODE"
    );
}

/// sql-114 (130 FR-019). A row lock that times out is -114, and the INTO variable is not a
/// sign of success. On 2026.2 it held the locked row in runs on their own and came back empty in
/// three full-suite runs, so the section may not promise either value.
#[test]
fn sql_patterns_section_5_names_the_lock_timeout() {
    let s5 = section(&skill("objectscript-sql-patterns"), 5);
    assert!(s5.contains("-114"), "sql-patterns §5 must name -114");
    assert!(
        s5.contains("READ COMMITTED"),
        "sql-patterns §5 must say -114 comes from READ COMMITTED's row lock"
    );
    assert!(
        !s5.contains("still holds the row"),
        "sql-patterns §5 must not promise the INTO variable holds the row on -114"
    );
    assert!(
        s5.contains("empty"),
        "sql-patterns §5 must say the INTO variable can be empty or hold the row on -114"
    );
}

/// sql-9 (130 FR-019). `COUNT(*) INTO :n` with `n` undefined leaves `n` defined and 0. On
/// SQLCODE 100 a plain SELECT sets the INTO variable to "", which the section keeps.
#[test]
fn sql_patterns_section_9_says_count_into_sets_zero() {
    let s9 = section(&skill("objectscript-sql-patterns"), 9);
    assert!(
        !s9.contains("IRIS leaves it empty string"),
        "sql-patterns §9 still says COUNT(*) INTO leaves an undefined variable empty"
    );
    assert!(
        !s9.contains("outputs \"count=\" (empty)"),
        "sql-patterns §9 still shows COUNT(*) INTO printing an empty count"
    );
    assert!(
        s9.contains("defined and 0"),
        "sql-patterns §9 must say COUNT(*) INTO leaves the variable defined and 0"
    );
    assert!(
        s9.contains("SQLCODE 100") && s9.contains("\"\""),
        "sql-patterns §9 must keep the true fact: SQLCODE 100 sets the INTO variable to \"\""
    );
}

// ── Spec 141 T-R4b-U16: skill recipes keep the password and the scheme out of argv ────────────

/// Every `*.md` under `skills/`, with its repository-relative path.
fn skill_markdown_files() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e == "md") {
                out.push(p);
            }
        }
    }
    let root = skills_dir().join("..");
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    assert!(paths.len() > 30, "found only {} skill files", paths.len());
    paths
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            (p.strip_prefix(&root).unwrap().display().to_string(), text)
        })
        .collect()
}

/// The logical command lines of a file that run `curl`: a line ending in `\` joins the next.
fn curl_commands(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let mut cmd = line.to_string();
        while cmd.trim_end().ends_with('\\') {
            cmd.pop();
            match lines.next() {
                Some(next) => cmd.push_str(next),
                None => break,
            }
        }
        // A command, not prose: `curl` starts the line, follows a pipe or an `$(` / `=`.
        let t = cmd.trim_start();
        let runs_curl = t.starts_with("curl ")
            || t.contains("| curl ")
            || t.contains("$(curl ")
            || t.contains("&& curl ")
            || t.contains("|| curl ");
        if runs_curl {
            out.push(cmd);
        }
    }
    out
}

#[test]
fn no_skill_recipe_puts_a_password_on_the_curl_command_line() {
    let mut bad = Vec::new();
    let mut seen = 0;
    for (name, text) in skill_markdown_files() {
        for cmd in curl_commands(&text) {
            seen += 1;
            let has_user_flag = cmd.split_whitespace().any(|w| w == "-u" || w == "--user")
                || cmd.contains(" -su ")
                || cmd.contains("-u \"") && cmd.contains("PASS");
            if has_user_flag || cmd.contains("_SYSTEM:SYS") || cmd.contains(":-SYS}") {
                bad.push(format!("{name}: {}", cmd.trim()));
            }
        }
    }
    assert!(
        seen > 5,
        "found only {seen} curl commands; the scan is broken"
    );
    assert!(
        bad.is_empty(),
        "curl commands with a password in argv (use `curl -K -` or the `tool` CLI):\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn skill_recipes_that_call_atelier_keep_the_scheme_configurable() {
    let mut bad = Vec::new();
    for (name, text) in skill_markdown_files() {
        for cmd in curl_commands(&text) {
            let calls_iris = cmd.contains("/api/atelier") || cmd.contains("${BASE_URL}");
            if !calls_iris {
                continue;
            }
            let ok = cmd.contains("${IRIS_SCHEME") || cmd.contains("localhost");
            let via_base_url =
                cmd.contains("${BASE_URL}") && text.contains("BASE_URL=\"${IRIS_SCHEME");
            if !(ok || via_base_url) {
                bad.push(format!("{name}: {}", cmd.trim()));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "curl commands against Atelier with a fixed scheme and a non-local host:\n  {}",
        bad.join("\n  ")
    );
}
