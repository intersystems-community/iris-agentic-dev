//! `propose` with `class` (spec 173, US1a, FR-001a): the master is the class's own master map,
//! so the master is not sampled and no master draft is returned. The candidates are found as in
//! global mode, the ones an index map of the class already uses are `already_mapped`, and each
//! other is linked and drafted in the class's property names and row-ID parts.

use super::{
    candidates, candidates_run, fail, globals, link_run, load_storage, master, model,
    model_with_facts, shape, values_refusal, Action, CallShape,
};
use crate::iris::connection::IrisConnection;
use crate::tools::{err_result, CallToolResult, McpError};
use serde_json::{json, Value};

pub(super) async fn propose_class(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &Value,
    tools: &crate::tools::IrisTools,
    shape: &CallShape,
) -> Result<CallToolResult, McpError> {
    let t = tools.target()?;
    if let Some(refusal) = values_refusal(shape, &t.primary) {
        return err_result(refusal);
    }
    // Checked before any IRIS call.
    let index_names = match candidates::index_names_param(p.get("index_globals")) {
        Ok(names) => names,
        Err(why) => return fail("INVALID_PARAMS", why, json!({})),
    };
    let stored = match load_storage(iris, client, p, "propose").await {
        Ok(stored) => stored,
        Err(reply) => return reply,
    };
    // Every global the class's maps name goes through the gates the master goes through.
    if let Some(body) = globals::check_map_globals(
        &stored.nodes,
        &t.primary.effective,
        &t.primary.key,
        Action::Propose,
        shape.acknowledge_phi,
    ) {
        return err_result(body);
    }
    let class_model = model_with_facts(stored.class, &stored.storage_name, &stored.nodes);
    let spec = match master::MasterSpec::from_class(&class_model) {
        Ok(spec) => spec,
        Err(refusal) => return fail(refusal.code, refusal.message, json!({})),
    };
    let Some(master_map) = class_model
        .storage
        .maps
        .iter()
        .find(|m| m.name == spec.map_name)
        .and_then(|m| serde_json::to_value(m).ok())
    else {
        return fail(
            "INVALID_PARAMS",
            format!("the master map {} could not be read", spec.map_name),
            json!({}),
        );
    };
    let cx = match tools.exec_context() {
        Ok(cx) => cx,
        Err(e) => return err_result(e.to_json()),
    };
    let node_cap = super::sample::caps(p.get("max_nodes").and_then(Value::as_u64)).nodes;
    let master_global = format!("^{}", spec.global.trim_start_matches('^'));
    let mut found = candidates_run::discover(
        &cx.conn,
        cx.client.as_ref(),
        &stored.ns,
        &master_global,
        spec.subscripts.len() == 1,
        &index_names,
    )
    .await;
    candidates::mark_already_mapped(&mut found.candidates, &class_model.storage.maps);
    let existing: Vec<String> = class_model
        .storage
        .maps
        .iter()
        .map(|m| m.name.clone())
        .collect();
    let run = link_run::Run {
        conn: &cx.conn,
        client: cx.client.as_ref(),
        ns: &stored.ns,
        master: &spec,
        master_map: &master_map,
        node_cap,
        policy: &t.primary.effective,
        server: &t.primary.key,
        acknowledge_phi: shape.acknowledge_phi,
        with_values: shape.include_values,
        existing_maps: &existing,
    };
    let linked = link_run::link_candidates(&run, found).await;
    let index_call = !linked.indexes.is_empty();
    let (mut strings, mut details) =
        shape::not_inferred_lists(&linked.not_inferred_detail, index_call);
    // The class names its own properties: the placeholder warning does not apply.
    details.retain(|d| d["what"] != "field names");
    strings.retain(|s| !s.starts_with("field names"));
    let mut answer = json!({
        "success": true,
        "model_version": model::MODEL_VERSION,
        "action": "propose",
        "namespace": stored.ns,
        "class": stored.class,
        "global": master_global,
        "index_candidates": linked.index_candidates,
        "limits": linked.limits,
        "not_inferred": strings,
        "not_inferred_detail": details,
    });
    if index_call {
        answer["indexes"] = Value::Array(linked.indexes);
    }
    crate::tools::exec::attach_identity(Ok(CallToolResult::structured(answer)), &cx.helper_report())
}
