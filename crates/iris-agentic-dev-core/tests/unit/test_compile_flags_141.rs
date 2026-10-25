//! Spec 141 C1: `compile --flags` and `iris_compile flags` accept only the IRIS qualifier charset.
//!
//! The flags string goes into `$SYSTEM.OBJ.CompileAll("…")` and into the Atelier query string.
//! Escaping keeps a quote from closing the ObjectScript literal; the charset check also refuses a
//! value IRIS would read as a different qualifier list, with an error that names the rule.

use iris_agentic_dev_core::tools::compile_flags_problem;

#[test]
fn flags_outside_qualifier_charset_are_refused() {
    // A quote closes the literal, a newline starts a new source line, a space starts a new word.
    for bad in [
        "cuk\") Kill ^X //",
        "cuk\nSet x=1",
        "cuk\"",
        "cu k",
        "cuk;",
        "cuk)",
    ] {
        let msg = compile_flags_problem(bad).unwrap_or_else(|| panic!("{bad:?} must be refused"));
        assert!(
            msg.contains("flags"),
            "the error names the flags argument: {msg}"
        );
    }
}

#[test]
fn qualifier_lists_are_accepted() {
    // The default, and the flags and qualifiers that compile what was named and nothing more.
    for good in [
        "cuk",
        "",
        "ck",
        "CUK",
        "ck-d",
        "cukd/checkuptodate=all",
        "ck/checkuptodate=none",
        "cuk/displaylog=0/quiet",
        "c/nodisplay",
        "/checkuptodate=expandedonly",
    ] {
        assert!(
            compile_flags_problem(good).is_none(),
            "{good:?} compiles what was named and must pass"
        );
    }
}

/// Spec 141 T206. IRIS compiles more than the target when a flag or qualifier says so: `b` and
/// `/subclasses` add every subclass, `r` and `/predecessorclasses` the classes the target depends on,
/// `y` and `/relatedclasses` the classes it shares SQL with, `/expand` all three. The stored-source
/// check reads the target and its includes, so a class added this way runs its compile-time code
/// unread (measured on live IRIS in `test_compile_widen_141_live`). Qualifier names are
/// case-insensitive and IRIS accepts an unambiguous abbreviation, so the check is an allowlist of
/// full names, not a list of the bad ones.
#[test]
fn flags_and_qualifiers_that_widen_the_set_compiled_are_refused() {
    for widening in [
        "cukb",
        "ckB",
        "ck-bb",
        "cukr",
        "cuky",
        "cukp",
        "cukh",
        "cuk/subclasses",
        "ck/SUBCLASSES",
        "ck/sub",
        "ck/subclasses=1",
        "ck/subclasses=0",
        "ck/expand",
        "ck/expan",
        "ck/expand=none",
        "ck/expand=subclasses,relatedclasses",
        "ck/predecessorclasses",
        "ck/relatedclasses",
        "ck/checkuptodate=none/subclasses",
        "ck/recursive",
        "ck/allnamespaces",
        "ck/multicompile",
        "ck/percent",
        "ck/includesubpackages",
    ] {
        let msg = compile_flags_problem(widening)
            .unwrap_or_else(|| panic!("{widening:?} widens the set compiled and must be refused"));
        assert!(
            msg.contains("flags") && msg.contains("compile"),
            "{widening:?}: the message names the argument and the compile: {msg}"
        );
        assert!(
            msg.contains("allowed"),
            "{widening:?}: the message says what is allowed: {msg}"
        );
    }
}

#[test]
fn malformed_flag_and_qualifier_lists_are_refused() {
    for bad in [
        "ck/",
        "ck//checkuptodate=none",
        "ck/=1",
        "u=1,e:2",
        "ck1",
        "ck.d",
        "ck/checkuptodate=none/",
    ] {
        assert!(compile_flags_problem(bad).is_some(), "{bad:?}");
    }
}
