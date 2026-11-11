//! T049: Global normalization for the `uses` action (spec 172 US2).
//!
//! Run: `RUST_TEST_THREADS=12 cargo test --features testing --test unit test_storage_map_gates_172`

use iris_agentic_dev_core::tools::storage_map::globals::normalize_global;

#[test]
fn normalize_global_accepts_caret_forms() {
    let r = normalize_global("^X").unwrap();
    assert_eq!(r.name, "^X");
    assert_eq!(r.extended, None);
    assert_eq!(r.subscripts_prefix, None);

    let r = normalize_global("X").unwrap();
    assert_eq!(r.name, "^X");
    assert_eq!(r.extended, None);
    assert_eq!(r.subscripts_prefix, None);
}

#[test]
fn normalize_global_accepts_extended_references() {
    let r = normalize_global("^|\"NS\"|X").unwrap();
    assert_eq!(r.name, "^X");
    assert_eq!(r.extended.as_deref(), Some("NS"));
    assert_eq!(r.subscripts_prefix, None);

    let r = normalize_global("^[\"NS\"]X").unwrap();
    assert_eq!(r.name, "^X");
    assert_eq!(r.extended.as_deref(), Some("NS"));
    assert_eq!(r.subscripts_prefix, None);

    let r = normalize_global("^|\"^^/db\"|X").unwrap();
    assert_eq!(r.name, "^X");
    assert_eq!(r.extended.as_deref(), Some("^^/db"));
    assert_eq!(r.subscripts_prefix, None);
}

#[test]
fn normalize_global_accepts_subscript_prefix() {
    let r = normalize_global("^X(1,2)").unwrap();
    assert_eq!(r.name, "^X");
    assert_eq!(r.extended, None);
    assert_eq!(r.subscripts_prefix.as_deref(), Some("(1,2)"));

    let r = normalize_global("X(\"a\",\"b\")").unwrap();
    assert_eq!(r.name, "^X");
    assert_eq!(r.extended, None);
    assert_eq!(r.subscripts_prefix.as_deref(), Some("(\"a\",\"b\")"));
}

#[test]
fn normalize_global_keeps_case() {
    let r = normalize_global("^MyGlobal").unwrap();
    assert_eq!(r.name, "^MyGlobal");

    let r = normalize_global("MyGlobal").unwrap();
    assert_eq!(r.name, "^MyGlobal");
}

#[test]
fn normalize_global_rejects_dynamic() {
    assert!(normalize_global("^@X").is_err());
    assert!(normalize_global("^X(@sub)").is_err());
    assert!(normalize_global("^$Name(x)").is_err());

    let err = normalize_global("^@dynamic").unwrap_err();
    assert_eq!(err, "dynamic");
}
