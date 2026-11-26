//! Running the bitmap parts of `propose` (spec 173 FR-016 to FR-018): the `bitmaps` item for a
//! candidate whose nodes hold bit strings, and the `chunk_map_draft` for a master of the shape
//! `^G(const, chunk) = $BIT`. The pure halves are in `bitmap.rs` (decode, drafts, verdicts) and
//! `link_code.rs` (the code); this is the part that talks to IRIS.
//!
//! Nothing a row holds leaves IRIS: the answers carry counts, positions, IDs' count and sum, the
//! index name and the constant first subscript (a name or a number).

use super::bitmap::{
    assemble_bitmap, bitmap_verdict, chunk_map_draft, constant_literal, draft_bitmap_map,
    looks_like_bit_master, parse_master_bits, MasterBits,
};
use super::candidates::Candidate;
use super::link::{link_json, Assembled, LinkReport, MasterView, Verdict};
use super::link_code::{master_bit_probe, Caps};
use super::link_run::Run;
use super::link_verify::verify;
use super::model::map_node;
use super::shape::ShapeReport;
use super::tree::{emit_body, normalize};
use super::typing::{class_source, PropertySpec, PropertyType};
use super::write::{scratch_compile, scratch_probe};
use crate::iris::connection::IrisConnection;
use serde_json::{json, Value};

fn detail(what: impl Into<String>, why: impl Into<String>) -> Value {
    json!({"what": what.into(), "why": why.into()})
}

/// The row-ID name the chunk draft uses for the key: the scratch class carries it as a property.
const KEY: &str = "Key1";

// ── the candidate side ─────────────────────────────────────────────────────

/// What a bitmap candidate adds to the answer: its `indexes` item, its `bitmaps` item, and what
/// was not drafted.
pub struct BitmapItems {
    pub index: Value,
    pub bitmap: Value,
}

/// The items for the candidate `c` whose linker run wrote a `bitmap` line, named `name`. The map is
/// drafted when the link is `inferred` or `ambiguous` and put on the items only when it verified
/// in a scratch copy.
pub async fn bitmap_items(
    run: &Run<'_>,
    c: &Candidate,
    name: &str,
    report: &LinkReport,
    view: &MasterView,
    details: &mut Vec<Value>,
) -> Option<BitmapItems> {
    let a: Assembled = assemble_bitmap(&c.global, name, report)?;
    let line = report.bitmaps.first()?;
    let (verdict, assumed) = bitmap_verdict(line, a.link.verdict);
    let mut index = link_json(&a, view, report);
    index["found_by"] = json!(c.found_by);
    index["guesses"] = json!([]);
    index["verdict"] = json!(verdict.as_str());
    index["id"] = json!({
        "form": "bitmap",
        "positions": [line.chunk_position],
        "agreement": index["id"]["agreement"],
    });
    let mut bitmap = json!({
        "global": c.global,
        "chunk_position": line.chunk_position,
        "index_name": line.lead,
        "ids_checked": line.ids_checked,
        "ids_found_in_master": line.ids_found,
        "verdict": verdict.as_str(),
        "assumed": assumed,
    });
    index["bitmap"] = json!({
        "chunk_position": line.chunk_position,
        "ids_checked": line.ids_checked,
        "ids_found_in_master": line.ids_found,
    });
    if line.ids_checked == 0 {
        details.push(detail(
            format!("{} holds bit strings that decoded to no row ID", c.label()),
            "no ID could be decoded (a composite row ID, or nodes of a depth the decode does not \
             read), so the link is not_linked",
        ));
    } else if verdict == Verdict::NotLinked {
        details.push(detail(
            format!("{} is not linked to the master", c.label()),
            format!(
                "its bit strings name {} row IDs and only {} of them are in the master",
                line.ids_checked, line.ids_found
            ),
        ));
    }
    for why in &assumed {
        details.push(detail(format!("{} decode", c.label()), why.clone()));
    }
    if line.later_checked == 0 && line.ids_checked > 0 {
        details.push(detail(
            format!("{} chunk size", c.label()),
            "only chunk 1 was sampled, so the chunk size of 64000 was assumed, not confirmed",
        ));
    }
    if verdict.is_explained() {
        match draft_and_verify(run, &a, view, line.lead.as_deref(), &assumed).await {
            Ok((map, guesses, verified)) => {
                index["map"] = map.clone();
                index["verified"] = verified.clone();
                index["guesses"] = json!(guesses);
                bitmap["map"] = map;
                bitmap["verified"] = verified;
            }
            Err((why, verified)) => {
                if let Some(v) = verified {
                    index["verified"] = v.clone();
                    bitmap["verified"] = v;
                }
                details.push(detail(format!("{} has no drafted map", c.label()), why));
            }
        }
    } else if verdict != Verdict::NotLinked {
        details.push(detail(
            format!("{} has no drafted map", c.label()),
            format!("the link is {}", verdict.as_str()),
        ));
    }
    Some(BitmapItems { index, bitmap })
}

type Drafted = (Value, Vec<String>, Value);

async fn draft_and_verify(
    run: &Run<'_>,
    a: &Assembled,
    view: &MasterView,
    lead: Option<&str>,
    assumed: &[String],
) -> Result<Drafted, (String, Option<Value>)> {
    let (map, mut guesses) = draft_bitmap_map(a, view, lead).map_err(|e| (e, None))?;
    if !assumed.is_empty() {
        guesses.push(format!("/maps/{}/type", a.link.map_name));
    }
    let v = verify(
        run.conn,
        run.client,
        run.ns,
        run.master,
        run.master_map,
        &map,
        &a.link,
        view,
    )
    .await;
    let verified = v.to_json();
    if !(v.compiled && v.row_found) {
        return Err((
            format!(
                "the draft did not verify in a scratch copy: {}",
                v.detail.as_deref().unwrap_or("no reason given")
            ),
            Some(verified),
        ));
    }
    Ok((map, guesses, verified))
}

// ── the master side ────────────────────────────────────────────────────────

/// What the master probe and the chunk draft add to the answer.
#[derive(Default)]
pub struct ChunkOutcome {
    pub draft: Option<Value>,
    pub details: Vec<Value>,
}

/// The `chunk_map_draft` for the sampled master `global`, when its shape suggests bit strings, the
/// probe confirms it and the draft verified in a scratch copy: the scratch table returned as many
/// IDs, with the same sum, as the bits name.
pub async fn chunk_map(
    conn: &IrisConnection,
    client: &reqwest::Client,
    ns: &str,
    global: &str,
    report: &ShapeReport,
    node_cap: u32,
) -> ChunkOutcome {
    let mut out = ChunkOutcome::default();
    if !looks_like_bit_master(report) {
        return out;
    }
    let what = format!("{global} as a bit-string master");
    let code = match master_bit_probe(global, Caps::for_node_cap(node_cap)) {
        Ok(c) => c,
        Err(why) => {
            out.details.push(detail(what, why));
            return out;
        }
    };
    let probed = match conn
        .execute_via_generator_prefixed(&code, ns, client, "IrisDevSm")
        .await
    {
        Ok(text) => parse_master_bits(&text),
        Err(e) => Err(format!("the probe could not run: {e}")),
    };
    let bits = match probed {
        Ok(Some(b)) => b,
        Ok(None) => return out,
        Err(why) => {
            out.details.push(detail(what, why));
            return out;
        }
    };
    match draft_and_check(conn, client, ns, global, &bits).await {
        Ok(draft) => out.draft = Some(draft),
        Err(why) => out
            .details
            .push(detail(format!("{global} chunk data map"), why)),
    }
    out
}

async fn draft_and_check(
    conn: &IrisConnection,
    client: &reqwest::Client,
    ns: &str,
    global: &str,
    bits: &MasterBits,
) -> Result<Value, String> {
    let constant = constant_literal(&bits.constant)?;
    let caret = format!("^{}", global.trim_start_matches('^'));
    let (map, guesses) = chunk_map_draft(&caret, &constant, KEY)?;
    let verified = verify_chunk_map(conn, client, ns, &map, bits).await?;
    Ok(json!({
        "map": map,
        "guesses": guesses,
        "bits": {
            "chunks_read": bits.chunks,
            "ids_read": bits.ids,
            "first_id": bits.first,
            "last_id": bits.last,
        },
        "verified": verified,
    }))
}

/// The scratch class of the chunk draft: one `%Integer` property `Key1` that is the whole ID, and
/// the draft as its only map.
fn chunk_class_text(map: &Value) -> Result<(String, Vec<super::tree::Node>), String> {
    let property = PropertySpec {
        name: KEY.to_string(),
        ty: PropertyType {
            name: "%Integer".into(),
            parameters: Vec::new(),
            date_guess: None,
            evidence: json!({}),
            note: "The row ID of a bit-string master, for the scratch copy that verifies its data map."
                .into(),
        },
        collation: None,
    };
    let draft = class_source("IrisDevTmp.SmDraft", &[property])?;
    let nodes = vec![super::link::storage_type_node(), map_node(map)?];
    let body = draft
        .source
        .trim_end()
        .strip_suffix('}')
        .ok_or("the class draft does not end in a brace")?;
    let text = format!(
        "{body}Index IdKeyIdx On {KEY} [ IdKey, Unique ];\n\nStorage SQLStorage\n{{\n{}\n}}\n\n}}\n",
        emit_body(&normalize(nodes.clone()))
    );
    Ok((text, nodes))
}

/// The count and the sum of the first 1,000 IDs the scratch table returns: the probe read the same
/// IDs in the same order, so the two pairs must be equal.
fn ids_query(table: &str) -> String {
    format!(
        "SELECT COUNT(*) || ':' || SUM({KEY}) FROM (SELECT TOP 1000 {KEY} FROM {table} ORDER BY {KEY}) AS q"
    )
}

/// The `count:sum` text of a query answer.
fn answer_pair(answer: &Value) -> Option<(u64, u64)> {
    let first = answer["result"]["content"][0]
        .as_object()
        .and_then(|o| o.values().next())?;
    let text = first
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| first.to_string());
    let (n, s) = text.split_once(':')?;
    Some((n.trim().parse().ok()?, s.trim().parse().ok()?))
}

async fn verify_chunk_map(
    conn: &IrisConnection,
    client: &reqwest::Client,
    ns: &str,
    map: &Value,
    bits: &MasterBits,
) -> Result<Value, String> {
    let (text, nodes) = chunk_class_text(map)?;
    scratch_probe(&text, &nodes).map_err(|refusal| {
        refusal["error"]
            .as_str()
            .unwrap_or("the scratch copy was refused")
            .to_string()
    })?;
    let query = |table: &str| ids_query(table);
    let (_, answer) = scratch_compile(
        conn,
        client,
        &text,
        &nodes,
        ns,
        Some(&query),
        "no real class was touched",
    )
    .await
    .map_err(|a| {
        let mut why = a["error"].as_str().unwrap_or("refused").to_string();
        if let Some(errors) = a["iris_errors"].as_array() {
            let shown: Vec<String> = errors
                .iter()
                .take(3)
                .map(|e| e.as_str().map_or_else(|| e.to_string(), String::from))
                .collect();
            why.push_str(&format!(": {}", shown.join("; ")));
        }
        format!("the draft did not compile in a scratch copy: {why}")
    })?;
    let answer = answer
        .ok_or("the scratch copy compiled but no query ran")?
        .map_err(|e| format!("the query on the scratch copy failed: {e}"))?;
    let (rows, sum) = answer_pair(&answer)
        .ok_or("the query on the scratch copy answered with no count and sum")?;
    if rows != u64::from(bits.ids) || sum != bits.id_sum {
        return Err(format!(
            "the scratch table returned {rows} IDs (sum {sum}) where the bits name {} (sum {}): \
             the computed level does not read this global",
            bits.ids, bits.id_sum
        ));
    }
    Ok(json!({"compiled": true, "row_found": true, "rows": rows, "id_sum": sum}))
}
