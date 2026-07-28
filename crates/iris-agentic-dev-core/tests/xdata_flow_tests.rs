#![allow(clippy::all)]
use iris_agentic_dev_core::tools::xdata_flow::*;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xdata")
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(fixtures_dir().join(name))
        .unwrap_or_else(|e| panic!("read fixture {name}: {e}"))
}

// ── T070-40: bpl_simple.xml → Code + Call steps ──────────────────────────────

#[test]
fn t070_40_parse_bpl_simple() {
    let xml = read_fixture("bpl_simple.xml");
    let flow = parse_bpl(&xml).expect("parse_bpl failed");
    assert_eq!(
        flow.steps.len(),
        2,
        "expected 2 steps, got: {:?}",
        flow.steps
    );

    let code_step = flow
        .steps
        .iter()
        .find(|s| matches!(s, BplStep::Code { .. }));
    assert!(code_step.is_some(), "expected a Code step");

    let call_step = flow
        .steps
        .iter()
        .find(|s| matches!(s, BplStep::Call { .. }));
    let call = call_step.expect("expected a Call step");
    match call {
        BplStep::Call { target, async_, .. } => {
            assert_eq!(target, "RiskService", "call target wrong");
            assert!(*async_, "call should be async=true");
        }
        _ => panic!("expected BplStep::Call"),
    }
}

// ── T070-41: bpl_dynamic.xml → has_dynamic_dispatch = true ──────────────────

#[test]
fn t070_41_parse_bpl_dynamic_dispatch() {
    let xml = read_fixture("bpl_dynamic.xml");
    let flow = parse_bpl(&xml).expect("parse_bpl failed");
    assert!(
        flow.has_dynamic_dispatch,
        "expected has_dynamic_dispatch=true for $classmethod usage"
    );
}

// ── T070-42: bpl_nested.xml → If step with inner Call ────────────────────────

#[test]
fn t070_42_parse_bpl_nested_if_with_call() {
    let xml = read_fixture("bpl_nested.xml");
    let flow = parse_bpl(&xml).expect("parse_bpl failed");

    let if_step = flow.steps.iter().find(|s| matches!(s, BplStep::If { .. }));
    let if_s = if_step.expect("expected an If step");
    match if_s {
        BplStep::If { steps, .. } => {
            let has_inner_call = steps.iter().any(|s| matches!(s, BplStep::Call { .. }));
            assert!(
                has_inner_call,
                "expected inner Call inside If; steps: {:?}",
                steps
            );
        }
        _ => panic!("expected BplStep::If"),
    }
}

// ── T070-43: dtl_simple.xml → two subtransforms + assign_count = 3 ──────────

#[test]
fn t070_43_parse_dtl_simple() {
    let xml = read_fixture("dtl_simple.xml");
    let flow = parse_dtl(&xml).expect("parse_dtl failed");

    assert_eq!(
        flow.subtransforms.len(),
        2,
        "expected 2 subtransforms, got: {:?}",
        flow.subtransforms
    );
    assert_eq!(
        flow.assign_count, 3,
        "expected assign_count=3, got {}",
        flow.assign_count
    );
    assert_eq!(flow.source_class, "Demo.Source");
    assert_eq!(flow.target_class, "Demo.Target");
}

// ── T070-44: empty BPL process → no panic, steps empty ───────────────────────

#[test]
fn t070_44_parse_bpl_empty_process_no_panic() {
    let xml = "<process language='objectscript' request='X' response='Y' />";
    let flow = parse_bpl(xml).expect("parse_bpl on empty process should not fail");
    assert!(
        flow.steps.is_empty(),
        "expected empty steps for empty process, got: {:?}",
        flow.steps
    );
}
