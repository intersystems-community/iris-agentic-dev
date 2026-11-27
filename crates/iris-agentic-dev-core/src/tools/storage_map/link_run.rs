//! Running the linker for `propose` (spec 173): for each index global the caller names, gate it,
//! run the generated code on the server as the call's execute identity, read the lines it wrote,
//! judge them, and draft the index map. The pure halves are `link.rs` (judging, drafting) and
//! `link_code.rs` (the code); this is the part that talks to IRIS.

use super::candidates::{cap_reason, prefix_levels, Candidate, Status, MAX_CANDIDATES};
use super::candidates_run::Discovery;
use super::globals::check_index_global;
use super::link::{
    apply_condition, assemble, condition_for, draft_index_map, field_label, link_json, map_name,
    orphan_nodes, parse_link_output, rounded, Assembled, Condition, LinkReport, MasterView,
    MissingRule, Stopped, Verdict, MAX_EXAMINED,
};
use super::link_code::{build_code, field_access, Caps, FieldAccess, LinkRequest};
use super::link_verify::{verify, verify_with, KeptCheck};
use super::master::MasterSpec;
use super::typing_run::{collation_picks, FieldCollation};
use crate::iris::connection::IrisConnection;
use crate::policy::effective::EffectivePolicy;
use serde_json::{json, Value};

/// A test forces the time cap of one candidate here, as `(global with its caret, seconds)`, so a
/// stop on the clock is certain without a global too big to build. Compiled only into test builds
/// (`testing`), so a release binary has no way to change a cap.
#[cfg(feature = "testing")]
pub static CANDIDATE_TIME_CAP_FOR_TESTS: std::sync::Mutex<Option<(String, u32)>> =
    std::sync::Mutex::new(None);

/// What `propose` adds to its answer for the index globals it examined.
pub struct Linked {
    pub index_candidates: Value,
    pub indexes: Vec<Value>,
    /// One item per candidate whose nodes hold bit strings (`bitmap_run`).
    pub bitmaps: Vec<Value>,
    pub limits: Value,
    /// `{what, why}`: what was not examined, not explained or not drafted, for
    /// `not_inferred_detail`.
    pub not_inferred_detail: Vec<Value>,
    /// What each verified map's link says about the collation of the field it reads, for the
    /// class draft's `COLLATION` parameters.
    pub collations: Vec<FieldCollation>,
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
    /// `include_values` under the data policy: a discovered null marker's literal may be returned.
    pub with_values: bool,
    /// Names of maps the class already has besides the master (class mode), so a drafted map
    /// takes a name none of them has.
    pub existing_maps: &'a [String],
}

/// The master as a draft names it, for spelling the map.
pub fn master_view(spec: &MasterSpec) -> MasterView {
    MasterView {
        global: spec.global.clone(),
        field_names: spec.fields.iter().map(|f| f.name.clone()).collect(),
        key_names: spec.key_names.clone(),
    }
}

/// A `{what, why}` item for `not_inferred_detail`.
pub(super) fn detail(what: impl Into<String>, why: impl Into<String>) -> Value {
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

/// Link the candidates of `found`: the first [`MAX_CANDIDATES`] open to examination, each through
/// the global gates first; the rest are named `not_examined` (FR-003). A candidate a class already
/// maps (`already_mapped`) is neither linked nor counted against the cap.
pub async fn link_candidates(run: &Run<'_>, found: Discovery) -> Linked {
    let mut candidates = found.candidates.clone();
    let mut open = 0;
    for c in candidates
        .iter_mut()
        .filter(|c| c.status == Status::Examined)
    {
        open += 1;
        if open > MAX_CANDIDATES {
            c.status = Status::NotExamined;
            c.reason = Some(cap_reason(MAX_CANDIDATES));
        }
    }
    let view = master_view(run.master);
    let mut details = found.details.clone();
    let mut indexes = Vec::new();
    let mut bitmaps = Vec::new();
    let mut stopped = Vec::new();
    let mut collations = Vec::new();
    let mut taken = vec![run.master.map_name.clone()];
    taken.extend(run.existing_maps.iter().cloned());
    let fields: Result<Vec<FieldAccess>, String> =
        run.master.fields.iter().map(field_access).collect();
    for c in &mut candidates {
        match c.status {
            Status::Examined => {}
            Status::NotExamined => {
                details.push(detail(
                    format!("{} was not examined", c.label()),
                    c.reason.clone().unwrap_or_default(),
                ));
                continue;
            }
            Status::AlreadyMapped | Status::Refused => continue,
        }
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
        // A sub-tree's map is named for its literal (`idx` is `Idx`), not for the master.
        let name_from = c
            .sub_prefix
            .last()
            .map_or_else(|| c.global.clone(), |literal| format!("^{literal}"));
        let name = map_name(&name_from, &taken);
        taken.push(name.clone());
        if !report.bitmaps.is_empty() {
            if let Some(items) =
                super::bitmap_run::bitmap_items(run, c, &name, &report, &view, &mut details).await
            {
                indexes.push(items.index);
                bitmaps.push(items.bitmap);
                continue;
            }
        }
        let assembled = assemble(&c.global, &name, &report);
        let item = index_item(run, c, &assembled, &view, &report, &mut details).await;
        if item.get("map").is_some() {
            collations.extend(collation_picks(&assembled.link));
        }
        indexes.push(item);
    }
    let not_examined: Vec<String> = candidates
        .iter()
        .filter(|c| !matches!(c.status, Status::Examined | Status::AlreadyMapped))
        .map(Candidate::label)
        .collect();
    let mut index_candidates = found.listing_json();
    index_candidates["candidates"] = json!(candidates);
    index_candidates["not_examined"] = json!(not_examined);
    Linked {
        index_candidates,
        indexes,
        bitmaps,
        limits: json!({
            "node_cap": run.node_cap,
            "time_cap_seconds": Caps::for_node_cap(run.node_cap).time_cap_secs,
            "stopped": stopped,
        }),
        not_inferred_detail: details,
        collations,
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
    #[allow(unused_mut)]
    let mut caps = Caps::for_node_cap(run.node_cap);
    #[cfg(feature = "testing")]
    if let Some((global, secs)) = CANDIDATE_TIME_CAP_FOR_TESTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    {
        if global == c.global {
            caps.time_cap_secs = secs;
        }
    }
    let code = build_code(&LinkRequest {
        master: &run.master.global,
        candidate: &c.global,
        sub_prefix: &c.sub_prefix,
        key_parts,
        fields,
        caps,
        with_values: run.with_values,
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
    for j in a
        .judgements
        .iter()
        .filter(|j| j.judgement.verdict == Verdict::NotExplained)
    {
        let best = j.judgement.best.as_ref().map_or(String::new(), |b| {
            format!(
                "; the closest is {} by {}",
                field_label(view, b.field),
                b.transform.name()
            )
        });
        details.push(detail(
            format!("{} position {}", c.label(), j.position),
            format!(
                "no transform of the closed set reaches 0.95 here (best {}{best}): a computed \
                 subscript, or a form this tool does not draft",
                rounded(j.judgement.agreement)
            ),
        ));
    }
    if let Some((count, share)) = orphan_nodes(a, report) {
        details.push(detail(
            format!("{} has index nodes with no master row", c.label()),
            format!(
                "{count} of the nodes read ({:.0}%) find no master row, as rows deleted from the \
                 master would leave them; the link rests on the rest",
                share * 100.0
            ),
        ));
    }
    for p in &a.link.unexamined {
        details.push(detail(
            format!("{} position {p} was not examined", c.label()),
            format!(
                "an index is examined up to {} subscripts besides the row ID, and this one has \
                 more (the first six are scored, the rest are left to you)",
                MAX_EXAMINED
            ),
        ));
    }
    for sub in &a.link.subscripts {
        if let Some(m) = &sub.null_marker {
            if m.expression().is_err() {
                details.push(detail(
                    format!("{} position {} NullMarker", c.label(), sub.position),
                    "empty fields file a marker that is none of the engine's four, and its literal is \
                     held back (pass include_values under the data policy to draft it); the draft \
                     needs a NullMarker on this subscript",
                ));
            }
        }
    }
    let condition = sparse_condition(c, view, report, details);
    let drafted = if a.link.verdict.is_explained() {
        draft_verified(
            run,
            a,
            view,
            &c.sub_prefix,
            &mut item,
            condition.as_ref(),
            details,
        )
        .await
    } else {
        Err(format!("the link is {}", a.link.verdict.as_str()))
    };
    if let Err(why) = drafted {
        details.push(detail(format!("{} has no drafted map", c.label()), why));
    }
    item
}

/// The condition and the numbers it must check against, when the index lacks master rows (spec 173
/// FR-019): the rule the linker found, spelt as a `Condition`, the rows examined and how many of
/// them the master keeps under it. What was not drafted, and why, goes to `details`.
fn sparse_condition(
    c: &Candidate,
    view: &MasterView,
    report: &LinkReport,
    details: &mut Vec<Value>,
) -> Option<(Condition, u32, u64)> {
    let m = report.missing.as_ref()?;
    let what = format!("{} lacks master rows", c.label());
    match condition_for(m, view) {
        Ok(cond) => Some((cond, m.rows, u64::from(m.rows.saturating_sub(m.count)))),
        Err(why) => {
            let why = if m.rule == MissingRule::None {
                format!(
                    "no rule fit: no one condition on a master field explains every missing row \
                     and no present row, so the drafted map covers fewer rows than the master \
                     ({} of {} examined rows are missing)",
                    m.count, m.rows
                )
            } else {
                format!("{why}; the drafted map covers fewer rows than the master")
            };
            details.push(detail(what, why));
            None
        }
    }
}

/// Draft the index map, verify it in a scratch copy, and put the map and its guesses on `item`
/// only when it verified; `verified` goes on `item` whenever the check ran. With a `condition`, the
/// map carries it and the scratch copy must also show the master keeps the rows the index holds; a
/// condition that does not check out is left off, and the plain map is checked instead.
async fn draft_verified(
    run: &Run<'_>,
    a: &Assembled,
    view: &MasterView,
    sub_prefix: &[String],
    item: &mut Value,
    condition: Option<&(Condition, u32, u64)>,
    details: &mut Vec<Value>,
) -> Result<(), String> {
    let (mut plain, mut guesses) = draft_index_map(&a.link, view)?;
    // A sub-tree's positions count after its literal first subscripts; the saved map starts with them.
    prefix_levels(&mut plain, &mut guesses, sub_prefix);
    if let Some((cond, rows, expect)) = condition {
        let mut map = plain.clone();
        apply_condition(&mut map, cond);
        let kept = KeptCheck {
            condition: cond,
            rows: *rows,
            expect: *expect,
        };
        let (v, n) = verify_with(
            run.conn,
            run.client,
            run.ns,
            run.master,
            run.master_map,
            &map,
            &a.link,
            view,
            Some(&kept),
        )
        .await;
        if v.compiled && v.row_found {
            item["verified"] = v.to_json();
            item["verified"]["rows_kept"] = json!(n);
            item["map"] = map;
            item["guesses"] = json!(guesses);
            return Ok(());
        }
        details.push(detail(
            format!("{} condition {}", a.link.map_name, cond.expression),
            format!(
                "the drafted condition did not verify in a scratch copy, so the map has none: {}",
                v.detail.as_deref().unwrap_or("no reason given")
            ),
        ));
    }
    let v = verify(
        run.conn,
        run.client,
        run.ns,
        run.master,
        run.master_map,
        &plain,
        &a.link,
        view,
    )
    .await;
    item["verified"] = v.to_json();
    if !v.verified() {
        return Err(format!(
            "the draft did not verify in a scratch copy: {}",
            v.not_verified_because()
        ));
    }
    item["map"] = plain;
    item["guesses"] = json!(guesses);
    Ok(())
}
