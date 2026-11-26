//! Running the linker for `propose` (spec 173): for each index global the caller names, gate it,
//! run the generated code on the server as the call's execute identity, read the lines it wrote,
//! judge them, and draft the index map. The pure halves are `link.rs` (judging, drafting) and
//! `link_code.rs` (the code); this is the part that talks to IRIS.

use super::candidates::{Candidate, Status};
use super::globals::check_index_global;
use super::link::{
    assemble, draft_index_map, link_json, map_name, parse_link_output, Assembled, LinkReport,
    MasterView, Stopped,
};
use super::link_code::{build_code, field_access, Caps, FieldAccess, LinkRequest};
use super::link_verify::verify;
use super::master::MasterSpec;
use crate::iris::connection::IrisConnection;
use crate::policy::effective::EffectivePolicy;
use serde_json::{json, Value};

/// What `propose` adds to its answer for the index globals it examined.
pub struct Linked {
    pub index_candidates: Value,
    pub indexes: Vec<Value>,
    pub limits: Value,
    /// `{what, why}`: what was not examined, not explained or not drafted, for
    /// `not_inferred_detail`.
    pub not_inferred_detail: Vec<Value>,
}

/// Everything a run needs from the call.
pub struct Run<'a> {
    pub conn: &'a IrisConnection,
    pub client: &'a reqwest::Client,
    pub ns: &'a str,
    pub master: &'a MasterSpec,
    /// The master draft in model JSON, for the scratch copy that verifies an index map.
    pub master_map: &'a Value,
    pub node_cap: u32,
    pub policy: &'a EffectivePolicy,
    pub server: &'a str,
    pub acknowledge_phi: bool,
}

/// The master as a draft names it, for spelling the map.
pub fn master_view(spec: &MasterSpec) -> MasterView {
    MasterView {
        global: spec.global.clone(),
        field_names: spec.fields.iter().map(|f| f.name.clone()).collect(),
        key_names: spec.key_names.clone(),
    }
}

fn detail(what: impl Into<String>, why: impl Into<String>) -> Value {
    json!({"what": what.into(), "why": why.into()})
}

/// The text of a gate's refusal body.
fn refusal_text(body: &Value) -> String {
    ["error", "message"]
        .iter()
        .find_map(|k| body[*k].as_str())
        .or_else(|| body["error_code"].as_str())
        .unwrap_or("refused by a global gate")
        .to_string()
}

fn cap_name(s: Stopped) -> Option<&'static str> {
    match s {
        Stopped::None => None,
        Stopped::Nodes => Some("nodes"),
        Stopped::Time => Some("time"),
        Stopped::CountCap => Some("count_cap"),
    }
}

/// Link each of `names` (carets on, checked by `validate_index_globals`) to the master.
pub async fn link_all(run: &Run<'_>, names: &[String]) -> Linked {
    let mut candidates = super::candidates::dedupe(super::candidates::from_parameter(names));
    let view = master_view(run.master);
    let mut details = Vec::new();
    let mut indexes = Vec::new();
    let mut stopped = Vec::new();
    let mut taken = vec![run.master.map_name.clone()];
    let fields: Result<Vec<FieldAccess>, String> =
        run.master.fields.iter().map(field_access).collect();
    for c in &mut candidates {
        let examined =
            match check_index_global(&c.global, run.policy, run.server, run.acknowledge_phi) {
                Some(body) => Err((Status::Refused, refusal_text(&body))),
                None => match &fields {
                    Ok(f) => run_one(run, c, f).await,
                    Err(why) => Err(why.clone()),
                }
                .map_err(|why| (Status::NotExamined, why)),
            };
        let report = match examined {
            Ok(r) => r,
            Err((status, why)) => {
                details.push(detail(format!("{} was not examined", c.label()), &why));
                c.status = status;
                c.reason = Some(why);
                continue;
            }
        };
        if let Some(cap) = cap_name(report.sample.stopped) {
            stopped.push(json!({"candidate": c.label(), "cap": cap}));
        }
        let name = map_name(&c.global, &taken);
        taken.push(name.clone());
        let assembled = assemble(&c.global, &name, &report);
        indexes.push(index_item(run, c, &assembled, &view, &report, &mut details).await);
    }
    let not_examined: Vec<String> = candidates
        .iter()
        .filter(|c| c.status != Status::Examined)
        .map(Candidate::label)
        .collect();
    Linked {
        index_candidates: json!({"candidates": candidates, "not_examined": not_examined}),
        indexes,
        limits: json!({
            "node_cap": run.node_cap,
            "time_cap_seconds": Caps::for_node_cap(run.node_cap).time_cap_secs,
            "stopped": stopped,
        }),
        not_inferred_detail: details,
    }
}

/// Build the code for one candidate, run it, and read what it wrote.
async fn run_one(
    run: &Run<'_>,
    c: &Candidate,
    fields: &[FieldAccess],
) -> Result<LinkReport, String> {
    let key_parts =
        u32::try_from(run.master.key_count()).map_err(|_| "too many key parts".to_string())?;
    let code = build_code(&LinkRequest {
        master: &run.master.global,
        candidate: &c.global,
        sub_prefix: &c.sub_prefix,
        key_parts,
        fields,
        caps: Caps::for_node_cap(run.node_cap),
        with_values: false,
    })?;
    let out = run
        .conn
        .execute_via_generator_prefixed(&code, run.ns, run.client, "IrisDevSm")
        .await
        .map_err(|e| format!("the linker could not run: {e}"))?;
    parse_link_output(&out)
}

/// The `indexes` item: the link's keys, its drafted map when it has one, and the guesses.
async fn index_item(
    run: &Run<'_>,
    c: &Candidate,
    a: &Assembled,
    view: &MasterView,
    report: &LinkReport,
    details: &mut Vec<Value>,
) -> Value {
    let mut item = link_json(a, view, report);
    item["found_by"] = json!(c.found_by);
    item["guesses"] = json!([]);
    for n in item["not_explained"].as_array().into_iter().flatten() {
        details.push(detail(
            format!("{} position {}", c.label(), n["position"]),
            format!(
                "no transform of the closed set reaches 0.95 here (best {})",
                n["best_agreement"]
            ),
        ));
    }
    let drafted = if a.link.verdict.is_explained() {
        draft_verified(run, a, view, &mut item).await
    } else {
        Err(format!("the link is {}", a.link.verdict.as_str()))
    };
    if let Err(why) = drafted {
        details.push(detail(format!("{} has no drafted map", c.label()), why));
    }
    item
}

/// Draft the index map, verify it in a scratch copy, and put the map and its guesses on `item`
/// only when it verified; `verified` goes on `item` whenever the check ran.
async fn draft_verified(
    run: &Run<'_>,
    a: &Assembled,
    view: &MasterView,
    item: &mut Value,
) -> Result<(), String> {
    let (map, guesses) = draft_index_map(&a.link, view)?;
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
    item["verified"] = v.to_json();
    if !(v.compiled && v.row_found) {
        return Err(format!(
            "the draft did not verify in a scratch copy: {}",
            v.detail.as_deref().unwrap_or("no reason given")
        ));
    }
    item["map"] = map;
    item["guesses"] = json!(guesses);
    Ok(())
}
