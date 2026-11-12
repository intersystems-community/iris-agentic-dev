//! T107: the writer's canonical form holds for trees nobody wrote by hand. 200 random Storage
//! trees, valid by the grammar, from a fixed seed: each is emitted, PUT (no compile), read back,
//! and compared with what was sent after IRIS's drop rules (`write::readback_diff`). Any
//! difference fails with the seed index and the tree.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_random_172 -- --test-threads=1 --include-ignored`

use iris_agentic_dev_core::tools::storage_map::tree::{emit_body, parse_body, split_class, Node};
use iris_agentic_dev_core::tools::storage_map::write::readback_diff;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len() as u64) as usize]
    }
}

fn leaf(tag: &str, value: &str) -> Node {
    Node {
        tag: tag.into(),
        name: None,
        value: Some(value.into()),
        children: None,
    }
}

fn elem(tag: &str, name: &str, children: Vec<Node>) -> Node {
    Node {
        tag: tag.into(),
        name: Some(name.into()),
        value: None,
        children: Some(children),
    }
}

/// A code-like value: plain, with characters that need CDATA, a `]]`, or several lines and tabs.
fn code(r: &mut Rng) -> String {
    match r.below(5) {
        0 => " s {L1}=$o(^IadSm172Rnd({L1}))".into(),
        1 => "{L1}>3&&({L1}<9)".into(),
        2 => "$S({L1}<\"]]\":1,1:0)".into(),
        3 => " s {L1}=$o(^IadSm172Rnd({L1}))\n f  {\n\tq:{L1}=\"\"\n\tq:{L1}'=\"x\"\n }".into(),
        _ => "$Get(^IadSm172Rnd({L1},\"a&b\"))".into(),
    }
}

fn subscript(r: &mut Rng, level: u32) -> Node {
    let mut c = vec![leaf(
        "Expression",
        r.pick(&["{ID}", "{Field1}", "\"node\"", "1", "{L1}"]),
    )];
    if r.chance(30) {
        c.push(leaf(
            "AccessType",
            r.pick(&["Sub", "Piece", "Other", "sub", "List"]),
        ));
    }
    if r.chance(25) {
        c.push(leaf("NextCode", &code(r)));
    }
    if r.chance(15) {
        c.push(leaf("Delimiter", "\"^\""));
    }
    if r.chance(15) {
        c.push(elem(
            "Accessvar",
            "1",
            vec![leaf("Code", &code(r)), leaf("Variable", "{1D1}")],
        ));
    }
    if r.chance(15) {
        c.push(elem(
            "Invalidcondition",
            "1",
            vec![leaf("Expression", &code(r))],
        ));
    }
    if r.chance(10) {
        c.push(leaf("StartValue", "0"));
    }
    elem("Subscript", &level.to_string(), c)
}

fn map(r: &mut Rng, name: &str, i: usize) -> Node {
    let mut c = vec![
        leaf("Type", r.pick(&["data", "index", "bitmap"])),
        leaf("Global", &format!("^IadSm172Rnd{i}")),
    ];
    for level in 1..=(1 + r.below(3) as u32) {
        c.push(subscript(r, level));
    }
    for f in 1..=r.below(5) {
        let mut d = Vec::new();
        if r.chance(70) {
            d.push(leaf("Delimiter", r.pick(&["\"^\"", "\"|\"", "$C(1)"])));
        }
        if r.chance(80) {
            d.push(leaf("Piece", &(1 + r.below(6)).to_string()));
        }
        if r.chance(25) {
            d.push(leaf("Node", r.pick(&["\"contact\"", "1", "\"a b\""])));
        }
        if r.chance(10) {
            d.push(leaf("RetrievalCode", &code(r)));
        }
        if d.is_empty() {
            d.push(leaf("Piece", "1"));
        }
        c.push(elem("Data", &format!("Field{f}"), d));
    }
    if r.chance(20) {
        c.push(elem(
            "RowIdSpec",
            "1",
            vec![leaf("Field", "Field1"), leaf("Expression", "{L1}")],
        ));
    }
    for (tag, value) in [
        ("Condition", "{Field1}=\"x\""),
        ("PopulationType", "nonnull"),
        ("BlockCount", "-4"),
        ("Structure", "delimited"),
        ("RowReference", "$name(^IadSm172Rnd({L1}))"),
        ("PopulationPct", "50"),
    ] {
        if r.chance(12) {
            c.push(leaf(tag, value));
        }
    }
    elem("SQLMap", name, c)
}

fn tree(r: &mut Rng) -> Vec<Node> {
    let mut nodes = vec![
        leaf("Type", "%Storage.SQL"),
        leaf("StreamLocation", "^IadSm172RndS"),
    ];
    if r.chance(60) {
        nodes.push(leaf("SqlIdExpression", "$i(^IadSm172Rnd)"));
    }
    if r.chance(20) {
        nodes.push(leaf("ExtentSize", &r.below(1000).to_string()));
    }
    for i in 0..(1 + r.below(3) as usize) {
        let name = if i == 0 {
            "Master".to_string()
        } else {
            format!("Idx{i}")
        };
        nodes.push(map(r, &name, i));
    }
    nodes
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_random_172_trees_round_trip() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    let t = iris_agentic_dev_core::testing::LiveTarget::from_env();
    t.assert_running();
    let mut conn = IrisConnection::new(
        t.base_url(),
        &t.namespace,
        &t.username,
        &t.password,
        DiscoverySource::ExplicitFlag,
    );
    conn.probe().await;
    let client = conn
        .client(iris_agentic_dev_core::iris::http::ClientKind::Call)
        .expect("client");
    let doc = "IadSm172Rnd.T.cls";
    let url = conn.versioned_ns_url("USER", &format!("/doc/{doc}"));
    let mut r = Rng(0x172_5EED);
    let mut failures = Vec::new();
    let (mut compared, mut cdata, mut multiline) = (0, 0, 0);
    for i in 0..200 {
        let sent = tree(&mut r);
        let text = format!(
            "Class IadSm172Rnd.T Extends %Persistent [ StorageStrategy = S ]\n{{\n\nStorage S\n{{\n{}\n}}\n\n}}\n",
            emit_body(&sent)
        );
        cdata += usize::from(text.contains("<![CDATA["));
        multiline += usize::from(text.contains("\n\tq:"));
        let lines: Vec<&str> = text.lines().collect();
        let put = client
            .put(format!("{url}?ignoreConflict=1"))
            .basic_auth(&conn.username, Some(&conn.password))
            .json(&serde_json::json!({"enc": false, "content": lines}))
            .send()
            .await
            .expect("PUT");
        if !put.status().is_success() {
            failures.push(format!("#{i}: PUT HTTP {}\n{text}", put.status()));
            continue;
        }
        let got: serde_json::Value = client
            .get(&url)
            .basic_auth(&conn.username, Some(&conn.password))
            .send()
            .await
            .expect("GET")
            .json()
            .await
            .unwrap_or_default();
        let stored: String = got["result"]["content"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let Some(span) = split_class(&stored).ok().and_then(|s| s.into_iter().next()) else {
            failures.push(format!("#{i}: no Storage block read back\n{stored}"));
            continue;
        };
        let back = match parse_body(&stored[span.body_start..span.body_end]) {
            Ok(n) => n,
            Err(e) => {
                failures.push(format!(
                    "#{i}: read-back does not parse: {}\n{stored}",
                    e.reason
                ));
                continue;
            }
        };
        compared += 1;
        let d = readback_diff(&sent, &back);
        if !d.dropped.is_empty() || !d.unexplained.is_empty() {
            failures.push(format!(
                "#{i}: dropped {:?}, unexplained {:?}\nsent:\n{}\nstored:\n{}",
                d.dropped,
                d.unexplained,
                emit_body(&sent),
                &stored[span.body_start..span.body_end]
            ));
        }
        if failures.len() >= 3 {
            break;
        }
    }
    let _ = client
        .delete(&url)
        .basic_auth(&conn.username, Some(&conn.password))
        .send()
        .await;
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    assert_eq!(compared, 200, "not every tree was compared");
    assert!(
        cdata > 20 && multiline > 10,
        "the trees missed the hard cases: {cdata} CDATA, {multiline} multi-line"
    );
}
