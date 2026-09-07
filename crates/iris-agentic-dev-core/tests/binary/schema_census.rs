//! Suite-wide census of what the server actually advertises.
//!
//! Every assertion here reads `tools/list` from a spawned `iris-agentic-dev mcp`, not
//! `schema_for!`. The two differ: `list_tools` runs `normalize_schema_openapi3` over each schema,
//! rewriting nullable types into `anyOf` branches, and it is the normalized form a client sees. A
//! test asserting on `schema_for!` would pass while the tool advertises something else — which is
//! how `server_version` sat in `check_config`'s payload and description for five minor versions
//! without ever being in its schema.
//!
//! `REMAINING_UNDECLARED` was the progress metric for the conversion: it started at 31 and each
//! batch lowered it. It is 0 now, and the assertion below fails in both directions, so it has turned
//! from a countdown into the guard the spec asked for — tool #82 cannot ship with an open parameter
//! set without failing this test.

use iris_agentic_dev_core::testing::{
    advertised_properties, advertised_schemas, advertised_tools_in_toolset, read_keys,
    require_iad_binary, resolve_property, tool_names, tools_with_open_parameter_sets,
};

/// Tools that advertise no properties while their handler reads named keys.
///
/// A ceiling, not a target: the test fails if the count rises, and fails if the count drops without
/// this constant being lowered. The second half matters — a stale ceiling is how a progress metric
/// stops measuring progress.
const REMAINING_UNDECLARED: usize = 0;

/// The ceiling is not a dial (113 FR-012). Raising it to make a new tool pass is now a compile
/// error, not an edit — the countdown is over, and the only correct value from here on is zero.
const _: () = assert!(
    REMAINING_UNDECLARED == 0,
    "a tool may not ship with an open parameter set; declare its params struct instead"
);

/// The six tools that legitimately take no arguments. Named, not pattern-matched, so tool #82
/// cannot join the list by accident.
const NO_ARGUMENT_TOOLS: &[&str] = &[
    "agent_stats",
    "check_config",
    "iris_import_servers",
    "iris_reload_pool",
    "skill_community_list",
    "skill_list",
];

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_undeclared_count_matches_the_ceiling() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = advertised_schemas();
    let undeclared = tools_with_open_parameter_sets(&schemas, &read_keys);

    assert_eq!(
        undeclared.len(),
        REMAINING_UNDECLARED,
        "expected exactly {REMAINING_UNDECLARED} tool(s) advertising an open parameter set, found \
         {}: {undeclared:?}\n\
         Higher means a regression. Lower means a batch landed without lowering \
         REMAINING_UNDECLARED, which leaves the metric no longer measuring anything.",
        undeclared.len()
    );
}

/// SC-006, cargo half: the census above must actually fail on a tool that reads a key it does not
/// advertise. Every other test here asserts the tree is clean, which passes just as well if the
/// predicate can never fire — the `max_chars` fallback in `test_docs_contract.rs` looked like a
/// working guard for five minor versions on exactly that basis. So the predicate is handed a
/// fixture: `phantom` advertises an open object and reads `max_chars`, the shape of the original
/// bug. Needs no binary, so it is not `#[ignore]`d.
#[test]
fn the_census_predicate_fires_on_a_tool_that_reads_an_unadvertised_key() {
    let schemas = std::collections::BTreeMap::from([
        (
            "declared".to_string(),
            serde_json::json!({"type": "object", "properties": {"name": {"type": "string"}}}),
        ),
        (
            "no_arguments".to_string(),
            serde_json::json!({"type": "object", "properties": {}}),
        ),
        ("phantom".to_string(), serde_json::json!({"type": "object"})),
    ]);
    let reads = |tool: &str| -> std::collections::BTreeSet<String> {
        match tool {
            "phantom" => ["max_chars".to_string()].into_iter().collect(),
            "declared" => ["name".to_string()].into_iter().collect(),
            _ => std::collections::BTreeSet::new(),
        }
    };

    let open = tools_with_open_parameter_sets(&schemas, &reads);
    assert_eq!(
        open,
        vec!["phantom".to_string()],
        "the predicate must name the tool that reads a key it never advertised, and must leave \
         alone both the tool that declares its parameters and the one that declares it takes none"
    );
    assert_ne!(
        open.len(),
        REMAINING_UNDECLARED,
        "with the ceiling at {REMAINING_UNDECLARED}, one open tool has to make the census \
         assertion fail — if these were equal the census would pass on the `max_chars` bug"
    );
}

/// The no-argument tools must say so structurally. Before this feature, `iris_import_servers` and
/// `iris_reload_pool` took no Rust parameter at all, so their schemas carried neither `properties`
/// nor `additionalProperties` — indistinguishable from "parameters unspecified".
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn no_argument_tools_declare_an_empty_closed_parameter_set() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = advertised_schemas();

    for tool in NO_ARGUMENT_TOOLS {
        let schema = schemas
            .get(*tool)
            .unwrap_or_else(|| panic!("`{tool}` must appear in tools/list"));
        let props = schema.get("properties").unwrap_or_else(|| {
            panic!("`{tool}` must advertise a `properties` key (empty object), got {schema}")
        });
        assert_eq!(
            props.as_object().map(serde_json::Map::len),
            Some(0),
            "`{tool}` takes no arguments but advertises {props}"
        );
        assert_eq!(
            schema.get("additionalProperties"),
            Some(&serde_json::json!(false)),
            "`{tool}` must reject unknown parameters; schema was {schema}"
        );
    }
}

/// The listing must cover the whole surface, and the source scan must agree with it. If the two
/// inventories drift, every per-tool comparison in this feature is checking a subset without
/// saying so.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_advertised_surface_matches_the_source_surface() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let advertised: std::collections::BTreeSet<String> =
        advertised_schemas().keys().cloned().collect();
    let in_source: std::collections::BTreeSet<String> = tool_names().into_iter().collect();

    assert!(
        advertised.len() >= 81,
        "tools/list advertised only {} tools: {advertised:?}",
        advertised.len()
    );
    let advertised_only: Vec<&String> = advertised.difference(&in_source).collect();
    assert!(
        advertised_only.is_empty(),
        "these tools are advertised but the source scan does not find them, so read_keys never \
         checks them: {advertised_only:?}"
    );
}

/// The thirty-one tools this feature converted, named so the totals below measure the conversion
/// rather than whatever the tool list happens to hold.
const CONVERTED: &[&str] = &[
    "capability_matrix",
    "compare_document",
    "compare_namespace",
    "global_kill",
    "global_preview",
    "hl7_schema_inspect",
    "hl7_schema_list",
    "iris_admin",
    "iris_business_rule_info",
    "iris_containers",
    "iris_credential_list",
    "iris_credential_manage",
    "iris_database_list",
    "iris_database_stats",
    "iris_interop_query",
    "iris_lookup_manage",
    "iris_lookup_transfer",
    "iris_message_body",
    "iris_mirror_status",
    "iris_namespace_create",
    "iris_namespace_list",
    "iris_production_diff",
    "iris_production_item",
    "iris_system_performance",
    "journal_search",
    "mermaid_class",
    "mermaid_production",
    "my_access",
    "query_audit_log",
    "resolve_storage",
    "stream_inspect",
];

/// The one sentence every tool uses for `server`. Fifty tools advertise the parameter and it means
/// the same thing in all fifty, so it reads the same in all fifty.
const SERVER_DESCRIPTION: &str =
    "Route this call to a named registered IRIS instance. If omitted, uses the default connection.";

/// The floor for what the conversion declared, asserted as one total. Every batch has its own
/// contract table, but a batch table can only fail on the tools it lists — this catches a batch that
/// quietly declares fewer properties than it reads.
const MIN_CONVERTED_SLOTS: usize = 132;
const MIN_CONVERTED_NAMES: usize = 71;

/// Every tool must reject a parameter it does not declare. This is the assertion that makes the
/// whole feature enforceable: a declared property set that still accepts anything else documents
/// the parameters without constraining them, which is where `max_chars` lived.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn every_tool_rejects_unknown_parameters() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = advertised_schemas();

    let mut open: Vec<&String> = schemas
        .iter()
        .filter(|(_, schema)| schema.get("additionalProperties") != Some(&serde_json::json!(false)))
        .map(|(name, _)| name)
        .collect();
    open.sort();

    assert!(
        open.is_empty(),
        "{} of {} tools accept undeclared parameters — each needs \
         `#[serde(deny_unknown_fields)]` on its params struct: {open:?}",
        open.len(),
        schemas.len()
    );
}

/// The tiers below Merged advertise nine tools Merged prunes, and every assertion above reads the
/// default listing — so those nine were never checked. `IRIS_TOOLSET=baseline` is the widest
/// listing the server can produce, which makes it the one to hold the rule against.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn every_tool_in_the_widest_toolset_rejects_unknown_parameters() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let tools = advertised_tools_in_toolset(Some("baseline"));

    let mut open: Vec<&String> = tools
        .iter()
        .filter(|(_, tool)| {
            tool.get("inputSchema")
                .and_then(|s| s.get("additionalProperties"))
                != Some(&serde_json::json!(false))
        })
        .map(|(name, _)| name)
        .collect();
    open.sort();

    assert!(
        open.len() < tools.len(),
        "the baseline listing returned {} tools and none of them declare \
         `additionalProperties: false` — IRIS_TOOLSET was probably ignored, so this test is \
         measuring the wrong server",
        tools.len()
    );
    assert!(
        open.is_empty(),
        "{} of {} tools in the baseline toolset accept undeclared parameters — each needs \
         `#[serde(deny_unknown_fields)]` on its params struct: {open:?}",
        open.len(),
        tools.len()
    );
}

/// `server` is the one parameter nearly every tool shares, so it is the one place where drift is
/// invisible: three phrasings of the same sentence read as three different parameters to a client
/// comparing tools, and nothing else in the suite compares them.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn server_reads_the_same_on_every_tool_that_takes_it() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = advertised_schemas();

    let mut declaring = 0usize;
    let mut wrong_type: Vec<String> = Vec::new();
    let mut required: Vec<String> = Vec::new();
    let mut phrasings: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();

    for (name, schema) in &schemas {
        if !advertised_properties(schema).contains("server") {
            continue;
        }
        declaring += 1;

        let prop = resolve_property(schema, "server");
        if prop.get("type") != Some(&serde_json::json!("string")) {
            wrong_type.push(format!("{name}: {prop}"));
        }
        // `server` selects a connection; a tool that demands one cannot be called against the
        // default connection at all, which would be a behaviour change disguised as a schema.
        if schema
            .get("required")
            .and_then(|r| r.as_array())
            .is_some_and(|r| r.iter().any(|v| v == "server"))
        {
            required.push(name.clone());
        }
        let described = prop
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("<none>")
            .to_string();
        phrasings.entry(described).or_default().push(name.clone());
    }

    assert!(
        declaring >= 50,
        "only {declaring} tools advertise `server`; the census expects at least 50, so either the \
         parameter was dropped somewhere or this test is now measuring a subset"
    );
    assert!(
        wrong_type.is_empty(),
        "`server` must be a string everywhere: {wrong_type:?}"
    );
    assert!(
        required.is_empty(),
        "`server` must stay optional; these tools demand it: {required:?}"
    );
    assert_eq!(
        phrasings.len(),
        1,
        "`server` is described {} different ways across {declaring} tools; a client comparing two \
         tools cannot tell that it is the same parameter. Phrasings and their tools: {phrasings:#?}",
        phrasings.len()
    );
    let (only, _) = phrasings.iter().next().expect("one phrasing");
    assert_eq!(
        only, SERVER_DESCRIPTION,
        "the shared `server` description drifted from the one this census pins"
    );
}

/// SC-002 as a total. Each batch asserts its own slots, but a batch table lists what the batch
/// declared — if a batch declared four properties for a tool that reads six, its own table agrees
/// with itself and only the total notices.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_conversion_declared_the_parameters_it_set_out_to() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let schemas = advertised_schemas();

    let mut slots = 0usize;
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for tool in CONVERTED {
        let schema = schemas
            .get(*tool)
            .unwrap_or_else(|| panic!("`{tool}` must appear in tools/list"));
        let props = advertised_properties(schema);
        assert!(
            !props.is_empty(),
            "`{tool}` was converted in this feature but advertises no properties: {schema}"
        );
        slots += props.len();
        names.extend(props);
    }

    assert!(
        slots >= MIN_CONVERTED_SLOTS,
        "the {} converted tools advertise {slots} property slots, below the {MIN_CONVERTED_SLOTS} \
         this feature set out to declare",
        CONVERTED.len()
    );
    assert!(
        names.len() >= MIN_CONVERTED_NAMES,
        "the converted tools advertise only {} distinct parameter names, below \
         {MIN_CONVERTED_NAMES}: {names:?}",
        names.len()
    );
}
