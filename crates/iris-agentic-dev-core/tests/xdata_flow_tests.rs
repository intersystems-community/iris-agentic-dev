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

// ── T070-45: BPL with self-closing <call/> (Event::Empty path) ────────────────

#[test]
fn t070_45_parse_bpl_self_closing_call() {
    let xml = r#"<process language="objectscript" request="Req" response="Resp">
  <sequence>
    <call name="Step1" target="MyService" async="1"/>
    <call name="Step2" target="OtherService" async="0"/>
  </sequence>
</process>"#;
    let flow = parse_bpl(xml).expect("parse_bpl failed");
    assert_eq!(flow.steps.len(), 2, "expected 2 Call steps");
    match &flow.steps[0] {
        BplStep::Call { target, async_, .. } => {
            assert_eq!(target, "MyService");
            assert!(*async_);
        }
        s => panic!("expected Call, got {:?}", s),
    }
    match &flow.steps[1] {
        BplStep::Call { target, async_, .. } => {
            assert_eq!(target, "OtherService");
            assert!(!*async_);
        }
        s => panic!("expected Call, got {:?}", s),
    }
}

// ── T070-46: BPL with Other steps (assign, break, trace via Event::Empty) ─────

#[test]
fn t070_46_parse_bpl_other_empty_steps() {
    let xml = r#"<process language="objectscript" request="Req" response="Resp">
  <sequence>
    <assign name="SetVar" property="x" value="1"/>
    <trace name="LogIt" value="'done'"/>
    <break name="ExitLoop"/>
  </sequence>
</process>"#;
    let flow = parse_bpl(xml).expect("parse_bpl failed");
    assert_eq!(
        flow.steps.len(),
        3,
        "expected 3 Other steps: {:?}",
        flow.steps
    );
    for step in &flow.steps {
        assert!(
            matches!(step, BplStep::Other { .. }),
            "expected Other, got {:?}",
            step
        );
    }
}

// ── T070-47: BPL with self-closing <code/> (Event::Empty code path) ───────────

#[test]
fn t070_47_parse_bpl_self_closing_code() {
    let xml = r#"<process language="objectscript" request="Req" response="Resp">
  <sequence>
    <code name="EmptyCode"/>
  </sequence>
</process>"#;
    let flow = parse_bpl(xml).expect("parse_bpl failed");
    assert_eq!(flow.steps.len(), 1);
    assert!(matches!(&flow.steps[0], BplStep::Code { .. }));
}

// ── T070-48: is_bpl_class / is_dtl_class ──────────────────────────────────────

#[test]
fn t070_48_is_bpl_class() {
    assert!(is_bpl_class("Ens.BusinessProcessBPL"));
    assert!(is_bpl_class("Demo.MyBase, Ens.BusinessProcessBPL"));
    assert!(!is_bpl_class("Ens.DataTransformDTL"));
    assert!(!is_bpl_class(""));
    assert!(!is_bpl_class("Ens.BusinessProcess")); // not BPL suffix
}

#[test]
fn t070_49_is_dtl_class() {
    assert!(is_dtl_class("Ens.DataTransformDTL"));
    assert!(is_dtl_class("Demo.MyBase, Ens.DataTransformDTL"));
    assert!(!is_dtl_class("Ens.BusinessProcessBPL"));
    assert!(!is_dtl_class(""));
}

// ── T070-50: extract_xdata_content ────────────────────────────────────────────

#[test]
fn t070_50_extract_xdata_content_found() {
    let xml = r#"<Export>
  <Class name="Demo.MyBPL">
    <XData name="BPL"><Data><![CDATA[<process/>]]></Data></XData>
  </Class>
</Export>"#;
    let content = extract_xdata_content(xml, "BPL");
    assert!(content.is_some(), "expected Some for BPL xdata block");
    assert!(content.unwrap().contains("<process/>"));
}

#[test]
fn t070_51_extract_xdata_content_not_found() {
    let xml = r#"<Export><Class name="Demo.Foo"><XData name="Other"><Data><![CDATA[x]]></Data></XData></Class></Export>"#;
    assert_eq!(extract_xdata_content(xml, "BPL"), None);
}

#[test]
fn t070_52_extract_xdata_content_case_insensitive() {
    let xml = r#"<Export>
  <Class name="Demo.Foo">
    <XData name="bpl"><Data><![CDATA[inner]]></Data></XData>
  </Class>
</Export>"#;
    let content = extract_xdata_content(xml, "BPL");
    assert!(
        content.is_some(),
        "xdata name match should be case-insensitive"
    );
}

// ── T070-53: DTL with no subtransforms, zero assigns ──────────────────────────

#[test]
fn t070_53_parse_dtl_empty() {
    let xml = r#"<transform language="objectscript" sourceClass="A" targetClass="B"/>"#;
    let flow = parse_dtl(xml).expect("parse_dtl failed on minimal transform");
    assert_eq!(flow.source_class, "A");
    assert_eq!(flow.target_class, "B");
    assert_eq!(flow.subtransforms.len(), 0);
    assert_eq!(flow.assign_count, 0);
}

// ── T070-54: BPL <code> with plain text body (read_cdata_content Text branch) ─

#[test]
fn t070_54_parse_bpl_code_plain_text_body() {
    // <code> body is unescaped text, not CDATA — exercises read_cdata_content Text branch
    let xml = r#"<process language="objectscript" request="Req" response="Resp">
  <sequence>
    <code name="TextBody">set x = 1</code>
  </sequence>
</process>"#;
    let flow = parse_bpl(xml).expect("parse_bpl failed");
    assert_eq!(flow.steps.len(), 1);
    assert!(matches!(&flow.steps[0], BplStep::Code { .. }));
}

// ── T070-55: BPL <call> with nested children (skip_element depth tracking) ────

#[test]
fn t070_55_parse_bpl_call_with_nested_children() {
    // <call> has child elements — skip_element must track depth > 1
    let xml = r#"<process language="objectscript" request="Req" response="Resp">
  <sequence>
    <call name="Step1" target="Service">
      <request messageclass="Req"/>
      <response messageclass="Resp"/>
    </call>
  </sequence>
</process>"#;
    let flow = parse_bpl(xml).expect("parse_bpl failed");
    assert_eq!(flow.steps.len(), 1);
    match &flow.steps[0] {
        BplStep::Call { name, target, .. } => {
            assert_eq!(name, "Step1");
            assert_eq!(target, "Service");
        }
        s => panic!("expected Call, got {:?}", s),
    }
}

// ── T070-56: BPL unknown empty tag (Event::Empty _ wildcard arm) ──────────────

#[test]
fn t070_56_parse_bpl_unknown_empty_tag() {
    // <unknown/> is a self-closing tag not in the known list — exercises the _ arm
    let xml = r#"<process language="objectscript" request="Req" response="Resp">
  <sequence>
    <unknown name="X"/>
    <call name="After" target="Svc" async="0"/>
  </sequence>
</process>"#;
    let flow = parse_bpl(xml).expect("parse_bpl failed");
    // unknown self-closing tag is silently ignored; only Call survives
    assert_eq!(flow.steps.len(), 1);
    assert!(matches!(&flow.steps[0], BplStep::Call { .. }));
}

// ── T070-57: extract_xdata_content with plain Text inside <Data> ──────────────

#[test]
fn t070_57_extract_xdata_content_plain_text_inside_data() {
    // <Data> contains unescaped text (no CDATA) — exercises Text branch in extract_xdata_content
    let xml = r#"<Export>
  <Class name="Demo.Foo">
    <XData name="BPL"><Data>plain text content</Data></XData>
  </Class>
</Export>"#;
    let result = extract_xdata_content(xml, "BPL");
    assert!(result.is_some());
    assert!(result.unwrap().contains("plain text content"));
}

// ── T070-58: extract_xdata_content with empty <Data> block ────────────────────

#[test]
fn t070_58_extract_xdata_content_empty_data_block() {
    // <Data></Data> contains only whitespace — accumulated.trim().is_empty() guard returns None
    let xml = r#"<Export>
  <Class name="Demo.Foo">
    <XData name="BPL"><Data>   </Data></XData>
  </Class>
</Export>"#;
    let result = extract_xdata_content(xml, "BPL");
    assert!(result.is_none(), "empty data block should return None");
}

// ── T070-59: BPL End handler at depth==0 (non-sequence close tag) ─────────────

#[test]
fn t070_59_parse_bpl_end_at_depth_zero() {
    // The End handler at depth==0 returns Ok(()) regardless of tag.
    // A bare <sequence> without a process wrapper exercises this path.
    let xml = r#"<sequence>
  <call name="Step1" target="Svc" async="0"/>
</sequence>"#;
    let flow = parse_bpl(xml).expect("parse_bpl failed");
    assert_eq!(flow.steps.len(), 1);
}
