//! Spec 141 US3 (FR-013, verify G6): the compile-time gate's lexer must stay in step through text
//! that is not ObjectScript. A lone `"` in a Python body, an apostrophe in `&html<>`, a `/*` inside
//! `&js<>` and an XData block with a quote all open a "string" or "comment" the gate's lexer
//! believes in and IRIS does not. The bogus run swallowed the member header that followed, with the
//! `CodeMode` keyword in it.
//!
//! Each case here is valid UDL. A document is built from a "desyncing" section followed by a real
//! generator or a real directive, and both must be found. The twin tests show the same sections
//! with no construct pass, so the fix is not "refuse anything odd".

use iris_agentic_dev_core::policy::code_edit_gate::check_compile_time_code_mode;

/// (label, section). Each leaves a quote or comment opener the gate's lexer must not trust.
const DESYNCERS: &[(&str, &str)] = &[
    (
        "lone quote in a python method",
        "ClassMethod P() [ Language = python ]\n{\n    s = '\"'\n}\n",
    ),
    (
        "lone quote in an html block",
        "ClassMethod H()\n{\n    &html<\">\n}\n",
    ),
    (
        "quote in a js block",
        "ClassMethod J()\n{\n    &js<var q='\"';>\n}\n",
    ),
    (
        "quote in an sql block",
        "ClassMethod S()\n{\n    &sql(SELECT Name FROM T WHERE Name = '\"')\n}\n",
    ),
    (
        "lone quote in xdata",
        "XData Notes\n{\n<note>say \" to me</note>\n}\n",
    ),
    (
        "block comment opener in a js block closed in a doc comment",
        "ClassMethod J2()\n{\n    &js</*>\n}\n\n/// closes the opener */\n",
    ),
    (
        "block comment opener in an html block",
        "ClassMethod H2()\n{\n    &html</*>\n}\n",
    ),
    (
        "semicolon and slashes in a python method",
        "ClassMethod P2() [ Language = python ]\n{\n    s = \"a;b\" # // /*\n}\n",
    ),
];

const GENERATOR: &str =
    "ClassMethod G() As %Status [ CodeMode = objectgenerator ]\n{\n    Quit $$$OK\n}\n";
const DIRECTIVE_METHOD: &str = "ClassMethod D() As %Status\n{\n    #execute Do ##class(My.Probe).Mark(\"d\")\n    Quit $$$OK\n}\n";

fn class(sections: &[&str]) -> String {
    format!("Class My.X\n{{\n\n{}\n}}\n", sections.join("\n"))
}

#[test]
fn a_generator_after_each_desyncing_section_is_still_found() {
    for (label, section) in DESYNCERS {
        let cls = class(&[section, GENERATOR]);
        let r = check_compile_time_code_mode(&cls, "My.X.cls")
            .unwrap_or_else(|| panic!("generator hidden after {label}:\n{cls}"));
        assert_eq!(r["matched"], "CodeMode = objectgenerator", "{label}: {r}");
    }
}

#[test]
fn a_directive_after_each_desyncing_section_is_still_found() {
    for (label, section) in DESYNCERS {
        let cls = class(&[section, DIRECTIVE_METHOD]);
        let r = check_compile_time_code_mode(&cls, "My.X.cls")
            .unwrap_or_else(|| panic!("directive hidden after {label}:\n{cls}"));
        assert!(
            r["matched"].as_str().unwrap().contains("#execute"),
            "{label}: {r}"
        );
    }
}

#[test]
fn the_same_sections_with_no_construct_pass() {
    for (label, section) in DESYNCERS {
        let cls = class(&[
            section,
            "ClassMethod Ok() As %Status\n{\n    Quit $$$OK\n}\n",
        ]);
        let r = check_compile_time_code_mode(&cls, "My.X.cls");
        assert!(r.is_none(), "{label} alone must pass, got {r:?}\n{cls}");
    }
}

#[test]
fn the_refusal_after_a_desync_names_the_right_line() {
    let cls = class(&[DESYNCERS[0].1, GENERATOR]);
    let r = check_compile_time_code_mode(&cls, "My.X.cls").expect("refused");
    let line = r["line"].as_u64().unwrap() as usize;
    assert_eq!(
        cls.lines().nth(line - 1).unwrap().trim(),
        "ClassMethod G() As %Status [ CodeMode = objectgenerator ]",
        "line {line} of:\n{cls}"
    );
}

#[test]
fn a_construct_inside_a_real_comment_or_string_after_a_desync_still_passes() {
    for (label, section) in DESYNCERS {
        let documenting = "/// Never write [ CodeMode = objectgenerator ] or #execute here.\n\
                           ClassMethod Doc() As %String\n{\n    Return \"#execute and CodeMode = objectgenerator\"\n}\n";
        let cls = class(&[section, documenting]);
        let r = check_compile_time_code_mode(&cls, "My.X.cls");
        assert!(
            r.is_none(),
            "documentation after {label} must pass, got {r:?}\n{cls}"
        );
    }
}

#[test]
fn a_header_split_over_two_lines_is_still_found() {
    let cls = class(&[
        DESYNCERS[0].1,
        "ClassMethod G()\n    [ CodeMode = objectgenerator ]\n{\n    Quit $$$OK\n}\n",
    ]);
    assert!(
        check_compile_time_code_mode(&cls, "My.X.cls").is_some(),
        "{cls}"
    );
}
