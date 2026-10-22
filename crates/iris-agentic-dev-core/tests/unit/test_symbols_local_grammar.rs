//! `iris_symbols_local` reads class members out of tree-sitter-objectscript's parse tree, so the
//! grammar version decides what it can see. On 5,485 real documents from a USER namespace,
//! 1.9.13 broke 32.5% of documents and missed 22% of class members, mostly at member headers;
//! 1.9.21 broke 2.8% (specs/139-objectscript-refactor/research.md, F34), and 1.10.3 broke 1.1%
//! (0.14% of methods). These pin the header forms 1.9.13 failed on, a JavaScript `ClientMethod`
//! (its own `clientmethod` node from 1.9.21 on), and the version floor that keeps them parsing.

use iris_agentic_dev_core::tools::symbols_local::extract_cls_symbols;

const HEADERS: &str = r#"Class Gram.Headers Extends %Persistent
{

Parameter VERSION As %String = "1";

Property Count As %Integer [ InitialExpression = 0 ];

Property Name As %String;

ForeignKey OwnerFK(Name) References Gram.Owner();

Index NameIdx On Name;

Method Zen() [ ZenMethod ]
{
    Quit
}

ClassMethod Gen() [ CodeMode = generator ]
{
    Quit $$$OK
}

ClientMethod Js() [ Language = javascript ]
{
    var a = this.b.c();
}

Method After() As %Status
{
    Quit $$$OK
}

}
"#;

#[test]
fn member_headers_that_broke_1_9_13_parse_and_every_member_is_listed() {
    let (symbols, warnings) = extract_cls_symbols(HEADERS.as_bytes(), "Gram/Headers.cls", "*");
    let errors: Vec<_> = warnings
        .iter()
        .filter(|w| w.warning_type == "PARSE_ERROR")
        .collect();
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    for member in [
        "Gram.Headers",
        "Gram.Headers.VERSION",
        "Gram.Headers.Count",
        "Gram.Headers.Name",
        "Gram.Headers.OwnerFK",
        "Gram.Headers.NameIdx",
        "Gram.Headers.Zen",
        "Gram.Headers.Gen",
        "Gram.Headers.Js",
        "Gram.Headers.After",
    ] {
        assert!(names.contains(&member), "{member} missing; got {names:?}");
    }
}

/// `Cargo.toml` asked for `1.9`, which 1.9.13 satisfied, so nothing stopped the lockfile resting
/// on the broken grammar. It now asks for `1.10`; this checks the lockfile honours it.
#[test]
fn the_lockfile_holds_grammar_1_10_3_or_later() {
    let lock = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock"),
    )
    .expect("Cargo.lock");
    for name in [
        "tree-sitter-objectscript",
        "tree-sitter-objectscript-routine",
    ] {
        let at = lock
            .find(&format!("name = \"{name}\"\nversion = \""))
            .unwrap_or_else(|| panic!("{name} not in Cargo.lock"));
        let rest = &lock[at..];
        let v = rest
            .split("version = \"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let parts: Vec<u32> = v.split('.').map(|p| p.parse().unwrap()).collect();
        assert!(
            parts.as_slice() >= [1, 10, 3].as_slice(),
            "{name} is {v} in Cargo.lock; 1.10.3 or later is needed"
        );
    }
}
