//! A release build compiles no test hooks (security fix).
//!
//! The core crate's `testing` feature adds a module of test support, a fault hook in the refusal
//! builder and a `mock` answer in the LLM client. None is reachable from tool input, but none
//! belongs in a shipped binary either. The binary crate used to name the feature on its core
//! dependency, so every build of it, release included, carried them. It now forwards its own
//! `testing` feature to core's and names nothing else.

fn manifest() -> toml::Table {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .parse::<toml::Table>()
        .expect("Cargo.toml parses")
}

#[test]
fn the_core_dependency_does_not_enable_testing() {
    let manifest = manifest();
    let dep = &manifest["dependencies"]["iris-agentic-dev-core"];
    let features: Vec<&str> = dep
        .get("features")
        .and_then(|f| f.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    assert!(
        features.is_empty(),
        "the release dependency on the core crate enables {features:?}; features belong behind this \
         crate's own `testing` feature"
    );
    assert!(
        dep.get("default-features").is_none(),
        "default-features changes what the release build compiles: {dep}"
    );
}

#[test]
fn only_the_testing_feature_reaches_core_testing() {
    let manifest = manifest();
    let features = manifest["features"].as_table().expect("[features]");
    let forwards = |name: &str| -> Vec<&str> {
        features
            .get(name)
            .and_then(|f| f.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default()
    };
    assert_eq!(
        forwards("testing"),
        ["iris-agentic-dev-core/testing"],
        "`--features testing` must reach the core crate's test support, or the test targets that \
         need it do not compile"
    );
    for (name, list) in features {
        if name == "testing" {
            continue;
        }
        let reaches = list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
            .any(|f| f == "testing" || f == "iris-agentic-dev-core/testing");
        assert!(
            !reaches,
            "feature `{name}` turns on test support; a default or release feature must not"
        );
    }
    let default = forwards("default");
    assert!(
        !default.contains(&"testing"),
        "`testing` is a default feature of the binary crate"
    );
}

#[test]
fn the_core_crate_does_not_default_to_testing() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../iris-agentic-dev-core/Cargo.toml");
    let manifest = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .parse::<toml::Table>()
        .expect("core Cargo.toml parses");
    let default: Vec<&str> = manifest["features"]
        .get("default")
        .and_then(|d| d.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    assert!(
        !default.contains(&"testing"),
        "the core crate turns `testing` on by default: {default:?}"
    );
}
