//! Storage model layer: typed structs built from the element tree.

use super::tree::{emit_body, normalize, Node};
use serde::Serialize;
use std::collections::BTreeMap;

/// Model version for the JSON contract (data-model.md).
pub const MODEL_VERSION: u32 = 1;

/// FNV-1a 64-bit hash implementation.
///
/// FNV-1a is a non-cryptographic hash used for version digests.
/// Algorithm: start with FNV offset basis, for each byte: xor then multiply by FNV prime.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Returns a version hex digest (16 lowercase hex digits) for the given text.
pub fn version_hex(text: &str) -> String {
    format!("{:016x}", fnv1a64(text.as_bytes()))
}

// ───────────────────────────────────────────────────────────────────────────
// T037: Typed structs
// ───────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StorageModel {
    pub model_version: u32,
    pub class: String,
    pub storage: SqlStorage,
    pub derived: Vec<Derived>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SqlStorage {
    pub name: String,
    pub r#type: String,
    pub version: String,
    pub editable: bool,
    pub not_editable_reason: Option<String>,
    pub keywords: BTreeMap<String, String>,
    pub description: Option<String>,
    pub maps: Vec<SqlMap>,
    pub default_storage: Option<DefaultStorage>,
    pub extra: Vec<ExtraElement>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SqlMap {
    pub name: String,
    pub version: String,
    pub r#type: String,
    pub master: bool,
    pub used: bool,
    pub global: String,
    pub subscripts: Vec<Subscript>,
    pub data: BTreeMap<String, DataField>,
    pub row_id: Vec<RowIdPart>,
    pub condition: Option<String>,
    pub condition_fields: Option<String>,
    pub conditional_with_host_vars: Option<String>,
    pub population_type: Option<String>,
    pub population_pct: Option<String>,
    pub block_count: Option<String>,
    pub structure: Option<String>,
    pub row_reference: Option<String>,
    pub deferred: Option<String>,
    pub extra: Vec<ExtraElement>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Subscript {
    pub level: u32,
    pub expression: String,
    pub access_type: String,
    pub delimiter: Option<String>,
    pub data_access: Option<String>,
    pub next_code: Option<String>,
    pub start_value: Option<String>,
    pub stop_value: Option<String>,
    pub stop_expression: Option<String>,
    pub loop_init_value: Option<String>,
    pub null_marker: Option<String>,
    pub access_vars: Vec<AccessVar>,
    pub invalid_conditions: Vec<InvalidCondition>,
    pub extra: Vec<ExtraElement>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AccessVar {
    pub level: u32,
    pub code: String,
    pub variable: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InvalidCondition {
    pub level: u32,
    pub expression: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DataField {
    pub name: String,
    pub node: Option<String>,
    pub delimiter: Option<String>,
    pub piece: Option<String>,
    pub retrieval_code: Option<String>,
    pub extra: Vec<ExtraElement>,
    pub derived: DataDerived,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DataDerived {
    pub format: String, // "$LIST" | "delimited" | "whole node"
    pub computed: bool,
    pub filed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RowIdPart {
    pub position: u32,
    pub field: Option<String>,
    pub expression: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ExtraElement {
    pub tag: String,
    pub name: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Derived {
    pub path: String,
    pub fact: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DefaultStorage {
    pub data_location: String,
    pub id_location: String,
    pub index_location: String,
    pub stream_location: String,
    pub nodes: Vec<DefaultDataNode>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DefaultDataNode {
    pub node: String,
    pub value_name: String,
    pub slots: Vec<DefaultSlot>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DefaultSlot {
    pub slot: u32,
    pub property: String,
}

// ───────────────────────────────────────────────────────────────────────────
// T038: from_tree
// ───────────────────────────────────────────────────────────────────────────

pub fn from_tree(class: &str, storage_name: &str, nodes: &[Node]) -> StorageModel {
    let mut storage_type = "%Storage.SQL".to_string();
    let mut keywords = BTreeMap::new();
    let mut description = None;
    let mut maps = Vec::new();
    let mut default_storage = None;
    let mut extra_elements = Vec::new();
    let mut derived_warnings = Vec::new();

    for node in nodes {
        match node.tag.as_str() {
            "Type" => {
                storage_type = node.value.clone().unwrap_or_default();
            }
            "Description" => {
                description = node.value.clone();
            }
            "SQLMap" => {
                let (map, warnings) = parse_sql_map(node, class);
                maps.push(map);
                derived_warnings.extend(warnings);
            }
            "Data" | "DataLocation" | "DefaultData" | "IdLocation" | "IndexLocation"
            | "StreamLocation" => {
                // Check if this is default storage format: <Data name="x"><Value name="n">...
                let is_default_storage = node.tag == "Data"
                    && node.name.is_some()
                    && node.children.as_ref().is_some_and(|children| {
                        children
                            .iter()
                            .any(|c| c.tag == "Value" && c.name.is_some())
                    });

                if is_default_storage {
                    if default_storage.is_none() {
                        default_storage = parse_default_storage(nodes);
                    }
                    continue;
                }
                // Otherwise it's a keyword
                keywords.insert(node.tag.clone(), node.value.clone().unwrap_or_default());
            }
            _ => {
                // Unknown tag or storage-level keyword
                if node.is_leaf() {
                    keywords.insert(node.tag.clone(), node.value.clone().unwrap_or_default());
                } else {
                    extra_elements.push(ExtraElement {
                        tag: node.tag.clone(),
                        name: node.name.clone(),
                        value: node.value.clone(),
                    });
                }
            }
        }
    }

    // Determine editable and not_editable_reason
    let (editable, not_editable_reason) = if default_storage.is_some() {
        (false, Some("default storage".to_string()))
    } else if storage_type != "%Storage.SQL" {
        (
            false,
            Some(format!("storage type {} is read-only in 172", storage_type)),
        )
    } else {
        (true, None)
    };

    // Set master and used flags for data maps
    let mut first_data_map = true;
    for map in &mut maps {
        if map.r#type == "data" {
            map.master = first_data_map;
            map.used = first_data_map;
            first_data_map = false;
        }
    }

    // Calculate storage version
    let normalized = normalize(nodes.to_vec());
    let canonical = emit_body(&normalized);
    let version = version_hex(&canonical);

    let storage = SqlStorage {
        name: storage_name.to_string(),
        r#type: storage_type,
        version,
        editable,
        not_editable_reason,
        keywords,
        description,
        maps,
        default_storage,
        extra: extra_elements,
    };

    StorageModel {
        model_version: MODEL_VERSION,
        class: class.to_string(),
        storage,
        derived: derived_warnings,
    }
}

fn parse_sql_map(node: &Node, class: &str) -> (SqlMap, Vec<Derived>) {
    let map_name = node.name.clone().unwrap_or_else(|| "Unknown".to_string());
    let mut map_type = String::new();
    let mut global = String::new();
    let mut subscripts = Vec::new();
    let mut data_fields = BTreeMap::new();
    let mut row_id_parts = Vec::new();
    let mut condition = None;
    let mut condition_fields = None;
    let mut conditional_with_host_vars = None;
    let mut population_type = None;
    let mut population_pct = None;
    let mut block_count = None;
    let mut structure = None;
    let mut row_reference = None;
    let mut deferred = None;
    let mut extra = Vec::new();
    let mut warnings = Vec::new();

    if let Some(children) = &node.children {
        for child in children {
            match child.tag.as_str() {
                "Type" => map_type = child.value.clone().unwrap_or_default(),
                "Global" => global = child.value.clone().unwrap_or_default(),
                "Condition" => condition = child.value.clone(),
                "ConditionFields" => condition_fields = child.value.clone(),
                "ConditionalWithHostVars" => conditional_with_host_vars = child.value.clone(),
                "PopulationType" => population_type = child.value.clone(),
                "PopulationPct" => population_pct = child.value.clone(),
                "BlockCount" => block_count = child.value.clone(),
                "Structure" => structure = child.value.clone(),
                "RowReference" => row_reference = child.value.clone(),
                "Deferred" => deferred = child.value.clone(),
                "Subscript" => {
                    let (sub, warn) =
                        parse_subscript(child, class, &map_name, subscripts.len() + 1);
                    subscripts.push(sub);
                    if let Some(w) = warn {
                        warnings.push(w);
                    }
                }
                "Data" => {
                    let data_name = child.name.clone().unwrap_or_else(|| "Unknown".to_string());
                    let field = parse_data_field(child, &data_name);
                    data_fields.insert(data_name, field);
                }
                "RowIdSpec" => {
                    let (row_id, warn) =
                        parse_row_id_spec(child, class, &map_name, row_id_parts.len() + 1);
                    row_id_parts.push(row_id);
                    if let Some(w) = warn {
                        warnings.push(w);
                    }
                }
                _ => {
                    extra.push(ExtraElement {
                        tag: child.tag.clone(),
                        name: child.name.clone(),
                        value: child.value.clone(),
                    });
                }
            }
        }
    }

    // Calculate map version
    let map_nodes = vec![node.clone()];
    let normalized = normalize(map_nodes);
    let canonical = emit_body(&normalized);
    let version = version_hex(&canonical);

    let map = SqlMap {
        name: map_name,
        version,
        r#type: map_type,
        master: false, // Will be set later in from_tree
        used: false,   // Will be set later in from_tree
        global,
        subscripts,
        data: data_fields,
        row_id: row_id_parts,
        condition,
        condition_fields,
        conditional_with_host_vars,
        population_type,
        population_pct,
        block_count,
        structure,
        row_reference,
        deferred,
        extra,
    };

    (map, warnings)
}

fn parse_subscript(
    node: &Node,
    _class: &str,
    map_name: &str,
    expected_level: usize,
) -> (Subscript, Option<Derived>) {
    let name_str = node.name.as_deref().unwrap_or("");
    let level = match name_str.parse::<u32>() {
        Ok(n) => n,
        Err(_) => {
            let warning = Derived {
                path: format!("/maps/{}/subscripts/{}", map_name, name_str),
                fact: format!("non-integer subscript name: {}", name_str),
            };
            return (
                Subscript {
                    level: expected_level as u32,
                    expression: String::new(),
                    access_type: "Sub".to_string(),
                    delimiter: None,
                    data_access: None,
                    next_code: None,
                    start_value: None,
                    stop_value: None,
                    stop_expression: None,
                    loop_init_value: None,
                    null_marker: None,
                    access_vars: Vec::new(),
                    invalid_conditions: Vec::new(),
                    extra: Vec::new(),
                },
                Some(warning),
            );
        }
    };

    let mut expression = String::new();
    let mut access_type = "Sub".to_string();
    let mut delimiter = None;
    let mut data_access = None;
    let mut next_code = None;
    let mut start_value = None;
    let mut stop_value = None;
    let mut stop_expression = None;
    let mut loop_init_value = None;
    let mut null_marker = None;
    let mut access_vars = Vec::new();
    let mut invalid_conditions = Vec::new();
    let mut extra = Vec::new();

    if let Some(children) = &node.children {
        for child in children {
            match child.tag.as_str() {
                "Expression" => expression = child.value.clone().unwrap_or_default(),
                "AccessType" => {
                    access_type = child.value.clone().unwrap_or_else(|| "Sub".to_string())
                }
                "Delimiter" => delimiter = child.value.clone(),
                "DataAccess" => data_access = child.value.clone(),
                "NextCode" => next_code = child.value.clone(),
                "StartValue" => start_value = child.value.clone(),
                "StopValue" => stop_value = child.value.clone(),
                "StopExpression" => stop_expression = child.value.clone(),
                "LoopInitValue" => loop_init_value = child.value.clone(),
                "NullMarker" => null_marker = child.value.clone(),
                "Accessvar" => {
                    if let Some(var) = parse_access_var(child) {
                        access_vars.push(var);
                    }
                }
                "Invalidcondition" => {
                    if let Some(cond) = parse_invalid_condition(child) {
                        invalid_conditions.push(cond);
                    }
                }
                _ => {
                    extra.push(ExtraElement {
                        tag: child.tag.clone(),
                        name: child.name.clone(),
                        value: child.value.clone(),
                    });
                }
            }
        }
    }

    (
        Subscript {
            level,
            expression,
            access_type,
            delimiter,
            data_access,
            next_code,
            start_value,
            stop_value,
            stop_expression,
            loop_init_value,
            null_marker,
            access_vars,
            invalid_conditions,
            extra,
        },
        None,
    )
}

fn parse_access_var(node: &Node) -> Option<AccessVar> {
    let name_str = node.name.as_deref()?;
    let level = name_str.parse::<u32>().ok()?;

    let mut code = String::new();
    let mut variable = String::new();

    if let Some(children) = &node.children {
        for child in children {
            match child.tag.as_str() {
                "Code" => code = child.value.clone().unwrap_or_default(),
                "Variable" => variable = child.value.clone().unwrap_or_default(),
                _ => {}
            }
        }
    }

    Some(AccessVar {
        level,
        code,
        variable,
    })
}

fn parse_invalid_condition(node: &Node) -> Option<InvalidCondition> {
    let name_str = node.name.as_deref()?;
    let level = name_str.parse::<u32>().ok()?;
    // IRIS stores it as a child `<Expression>` (`%Dictionary.StorageSQLMapSubInvalidcondition
    // Definition`); a bare value is read too.
    let expression = node
        .children
        .iter()
        .flatten()
        .find(|c| c.tag == "Expression")
        .and_then(|c| c.value.clone())
        .or_else(|| node.value.clone())
        .unwrap_or_default();

    Some(InvalidCondition { level, expression })
}

fn parse_data_field(node: &Node, field_name: &str) -> DataField {
    let mut node_val = None;
    let mut delimiter = None;
    let mut piece = None;
    let mut retrieval_code = None;
    let mut extra = Vec::new();

    if let Some(children) = &node.children {
        for child in children {
            match child.tag.as_str() {
                "Node" => node_val = child.value.clone(),
                "Delimiter" => delimiter = child.value.clone(),
                "Piece" => piece = child.value.clone(),
                "RetrievalCode" => retrieval_code = child.value.clone(),
                _ => {
                    extra.push(ExtraElement {
                        tag: child.tag.clone(),
                        name: child.name.clone(),
                        value: child.value.clone(),
                    });
                }
            }
        }
    }

    // Derive format, computed, filed
    let format = if delimiter.is_some() {
        "delimited".to_string()
    } else if node_val.is_none() {
        "$LIST".to_string()
    } else {
        "whole node".to_string()
    };

    let computed = retrieval_code.is_some();
    let filed = !computed;

    DataField {
        name: field_name.to_string(),
        node: node_val,
        delimiter,
        piece,
        retrieval_code,
        extra,
        derived: DataDerived {
            format,
            computed,
            filed,
        },
    }
}

fn parse_row_id_spec(
    node: &Node,
    _class: &str,
    map_name: &str,
    expected_position: usize,
) -> (RowIdPart, Option<Derived>) {
    let name_str = node.name.as_deref().unwrap_or("");
    let position = match name_str.parse::<u32>() {
        Ok(n) => n,
        Err(_) => {
            let warning = Derived {
                path: format!("/maps/{}/row_id/{}", map_name, name_str),
                fact: format!("non-integer row ID spec name: {}", name_str),
            };
            return (
                RowIdPart {
                    position: expected_position as u32,
                    field: None,
                    expression: None,
                },
                Some(warning),
            );
        }
    };

    let mut field = None;
    let mut expression = None;

    if let Some(children) = &node.children {
        for child in children {
            match child.tag.as_str() {
                "Field" => field = child.value.clone(),
                "Expression" => expression = child.value.clone(),
                _ => {}
            }
        }
    }

    (
        RowIdPart {
            position,
            field,
            expression,
        },
        None,
    )
}

fn parse_default_storage(nodes: &[Node]) -> Option<DefaultStorage> {
    let mut data_location = String::new();
    let mut id_location = String::new();
    let mut index_location = String::new();
    let mut stream_location = String::new();
    let mut data_nodes = Vec::new();

    for node in nodes {
        match node.tag.as_str() {
            "DataLocation" => data_location = node.value.clone().unwrap_or_default(),
            "IdLocation" => id_location = node.value.clone().unwrap_or_default(),
            "IndexLocation" => index_location = node.value.clone().unwrap_or_default(),
            "StreamLocation" => stream_location = node.value.clone().unwrap_or_default(),
            "Data" => {
                let Some(name) = &node.name else {
                    continue;
                };
                let Some(children) = &node.children else {
                    continue;
                };

                let mut slots = Vec::new();
                for value_node in children {
                    if value_node.tag != "Value" {
                        continue;
                    }
                    let Some(value_name) = &value_node.name else {
                        continue;
                    };
                    let Ok(slot_num) = value_name.parse::<u32>() else {
                        continue;
                    };
                    let Some(value_children) = &value_node.children else {
                        continue;
                    };

                    for v in value_children {
                        if v.tag == "Value" {
                            if let Some(prop) = &v.value {
                                slots.push(DefaultSlot {
                                    slot: slot_num,
                                    property: prop.clone(),
                                });
                            }
                        }
                    }
                }

                if !slots.is_empty() {
                    data_nodes.push(DefaultDataNode {
                        node: name.clone(),
                        value_name: name.clone(),
                        slots,
                    });
                }
            }
            _ => {}
        }
    }

    if data_nodes.is_empty() && data_location.is_empty() {
        return None;
    }

    Some(DefaultStorage {
        data_location,
        id_location,
        index_location,
        stream_location,
        nodes: data_nodes,
    })
}

// ───────────────────────────────────────────────────────────────────────────
// T039: Derived facts
// ───────────────────────────────────────────────────────────────────────────

pub fn derived_facts(model: &StorageModel) -> Vec<Derived> {
    let mut facts = Vec::new();

    // Add warnings from from_tree
    facts.extend(model.derived.clone());

    // Check each map
    for (map_idx, map) in model.storage.maps.iter().enumerate() {
        // Check for fields with no delimiter (read as $LIST) - R5
        for (field_name, field) in &map.data {
            if field.delimiter.is_none() {
                facts.push(Derived {
                    path: format!("/maps/{}/data/{}", map.name, field_name),
                    fact: "no delimiter: node read as $LIST".to_string(),
                });
            }

            // Check for computed fields (RetrievalCode)
            if field.retrieval_code.is_some() {
                facts.push(Derived {
                    path: format!("/maps/{}/data/{}", map.name, field_name),
                    fact: "computed field: not filed".to_string(),
                });
            }
        }

        // Check if table is writable (master global starts with ^, $ or @)
        if map.master && map_idx == 0 {
            let starts_with_writable = map.global.starts_with('^')
                || map.global.starts_with('$')
                || map.global.starts_with('@');
            if starts_with_writable {
                facts.push(Derived {
                    path: "/storage".to_string(),
                    fact: "table is writable: master global starts with ^, $, or @".to_string(),
                });
            } else {
                facts.push(Derived {
                    path: "/storage".to_string(),
                    fact: "table is read-only: master global does not start with ^, $, or @"
                        .to_string(),
                });
            }
        }

        // T040: D11 inside one map: some fields sit on the row node itself (no `Node`) and others on
        // named sub-nodes, so the row node holds a value and has children ($Data 11).
        let on_row: Vec<&String> = map
            .data
            .iter()
            .filter(|(_, f)| f.node.is_none() && f.retrieval_code.is_none())
            .map(|(n, _)| n)
            .collect();
        let sub_nodes: std::collections::BTreeSet<&String> =
            map.data.values().filter_map(|f| f.node.as_ref()).collect();
        if map.r#type == "data" && !on_row.is_empty() && !sub_nodes.is_empty() {
            facts.push(Derived {
                path: format!("/maps/{}", map.name),
                fact: format!(
                    "row node holds fields ({}) and has sub-nodes ({}): $Data 11",
                    on_row
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    sub_nodes
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
    }

    facts
}

// ───────────────────────────────────────────────────────────────────────────
// T040: Nested maps
// ───────────────────────────────────────────────────────────────────────────

/// The derived facts FR-003a adds to the parent of each nested map: a SQL DELETE of one of its rows
/// kills the row node's whole subtree, so it also deletes the nested map's rows stored under it
/// (measured, research R17). Paths are `<class>.<map>` as `nested_maps` returns them.
pub fn nesting_facts(models: &[StorageModel]) -> Vec<(String, Derived)> {
    nested_maps(models)
        .into_iter()
        .map(|(child, parent)| {
            (
                parent.clone(),
                Derived {
                    path: format!("/maps/{}", parent.rsplit('.').next().unwrap_or(&parent)),
                    fact: format!(
                        "map {child} stores its rows inside this map's row nodes: a SQL DELETE of a row here also deletes its {child} rows"
                    ),
                },
            )
        })
        .collect()
}

pub fn nested_maps(models: &[StorageModel]) -> Vec<(String, String)> {
    let mut nested = Vec::new();

    // Build a list of all data maps with their globals and subscript counts
    let mut all_maps: Vec<(String, String, usize, Vec<String>)> = Vec::new();

    for model in models {
        for map in &model.storage.maps {
            if map.r#type == "data" {
                let key = format!("{}.{}", model.class, map.name);
                let subscripts: Vec<String> = map
                    .subscripts
                    .iter()
                    .map(|s| s.expression.clone())
                    .collect();
                all_maps.push((key, map.global.clone(), subscripts.len(), subscripts));
            }
        }
    }

    // For each map, check if it's nested inside another
    for (child_key, child_global, child_count, child_subscripts) in &all_maps {
        // A map is nested if:
        // 1. Same global as another map
        // 2. More subscripts than that map
        // 3. At least one subscript is a literal
        let has_literal = child_subscripts.iter().any(|s| s.starts_with('"'));
        if !has_literal {
            continue;
        }

        // Find a potential parent: same global, fewer subscripts
        for (parent_key, parent_global, parent_count, _parent_subscripts) in &all_maps {
            if child_key == parent_key {
                continue;
            }
            let literal_next = child_subscripts
                .get(*parent_count)
                .is_some_and(|s| s.starts_with('"'));
            if child_global == parent_global && child_count > parent_count && literal_next {
                // This is a nested map
                nested.push((child_key.clone(), parent_key.clone()));
                break;
            }
        }
    }

    nested
}

// ───────────────────────────────────────────────────────────────────────────
// Model JSON back to elements (check, preview, replace take one SqlMap as `model`)
// ───────────────────────────────────────────────────────────────────────────

type Obj = serde_json::Map<String, serde_json::Value>;

/// The `SQLMap` element for one SqlMap in model JSON, the inverse of the parser above. Output-only
/// keys (`version`, `master`, `used`, `derived`) are ignored; any other key iad does not know is
/// refused, so nothing the caller set is dropped without a word. `access_type` `Sub` (the
/// default) writes no element.
pub fn map_node(map: &serde_json::Value) -> Result<Node, String> {
    let o = object(map, "model")?;
    known(
        o,
        "model",
        &[
            "name",
            "version",
            "type",
            "master",
            "used",
            "global",
            "subscripts",
            "data",
            "row_id",
            "condition",
            "condition_fields",
            "conditional_with_host_vars",
            "population_type",
            "population_pct",
            "block_count",
            "structure",
            "row_reference",
            "deferred",
            "extra",
        ],
    )?;
    let name = text(o, "name", "model")?.ok_or("model: the map has no `name`")?;
    let mut children = Vec::new();
    for (key, tag) in [
        ("type", "Type"),
        ("global", "Global"),
        ("condition", "Condition"),
        ("condition_fields", "ConditionFields"),
        ("conditional_with_host_vars", "ConditionalWithHostVars"),
        ("population_type", "PopulationType"),
        ("population_pct", "PopulationPct"),
        ("block_count", "BlockCount"),
        ("structure", "Structure"),
        ("row_reference", "RowReference"),
        ("deferred", "Deferred"),
    ] {
        push_leaf(&mut children, tag, text(o, key, "model")?);
    }
    for (i, sub) in list(o, "subscripts", "model")?.iter().enumerate() {
        children.push(subscript_node(sub, &format!("model/subscripts/{i}"))?);
    }
    if let Some(data) = o.get("data") {
        let data = object(data, "model/data")?;
        for (field, d) in data {
            children.push(data_node(field, d, &format!("model/data/{field}"))?);
        }
    }
    for (i, part) in list(o, "row_id", "model")?.iter().enumerate() {
        let at = format!("model/row_id/{i}");
        let p = object(part, &at)?;
        known(p, &at, &["position", "field", "expression"])?;
        let mut c = Vec::new();
        push_leaf(&mut c, "Field", text(p, "field", &at)?);
        push_leaf(&mut c, "Expression", text(p, "expression", &at)?);
        children.push(branch("RowIdSpec", Some(number(p, "position", &at)?), c));
    }
    children.extend(extra_nodes(o, "model")?);
    Ok(branch("SQLMap", Some(name), children))
}

fn subscript_node(v: &serde_json::Value, at: &str) -> Result<Node, String> {
    let s = object(v, at)?;
    known(
        s,
        at,
        &[
            "level",
            "expression",
            "access_type",
            "delimiter",
            "data_access",
            "next_code",
            "start_value",
            "stop_value",
            "stop_expression",
            "loop_init_value",
            "null_marker",
            "access_vars",
            "invalid_conditions",
            "extra",
        ],
    )?;
    let mut c = Vec::new();
    push_leaf(&mut c, "Expression", text(s, "expression", at)?);
    push_leaf(
        &mut c,
        "AccessType",
        text(s, "access_type", at)?.filter(|a| a != "Sub"),
    );
    for (key, tag) in [
        ("delimiter", "Delimiter"),
        ("data_access", "DataAccess"),
        ("next_code", "NextCode"),
        ("start_value", "StartValue"),
        ("stop_value", "StopValue"),
        ("stop_expression", "StopExpression"),
        ("loop_init_value", "LoopInitValue"),
        ("null_marker", "NullMarker"),
    ] {
        push_leaf(&mut c, tag, text(s, key, at)?);
    }
    for (i, av) in list(s, "access_vars", at)?.iter().enumerate() {
        let here = format!("{at}/access_vars/{i}");
        let a = object(av, &here)?;
        known(a, &here, &["level", "code", "variable"])?;
        let mut ac = Vec::new();
        push_leaf(&mut ac, "Code", text(a, "code", &here)?);
        push_leaf(&mut ac, "Variable", text(a, "variable", &here)?);
        c.push(branch("Accessvar", Some(number(a, "level", &here)?), ac));
    }
    for (i, ic) in list(s, "invalid_conditions", at)?.iter().enumerate() {
        let here = format!("{at}/invalid_conditions/{i}");
        let a = object(ic, &here)?;
        known(a, &here, &["level", "expression"])?;
        let mut icc = Vec::new();
        push_leaf(&mut icc, "Expression", text(a, "expression", &here)?);
        c.push(branch(
            "Invalidcondition",
            Some(number(a, "level", &here)?),
            icc,
        ));
    }
    c.extend(extra_nodes(s, at)?);
    Ok(branch("Subscript", Some(number(s, "level", at)?), c))
}

fn data_node(field: &str, v: &serde_json::Value, at: &str) -> Result<Node, String> {
    let d = object(v, at)?;
    known(
        d,
        at,
        &[
            "name",
            "node",
            "delimiter",
            "piece",
            "retrieval_code",
            "extra",
            "derived",
        ],
    )?;
    if let Some(n) = text(d, "name", at)? {
        if n != field {
            return Err(format!("{at}: name {n:?} differs from its key {field:?}"));
        }
    }
    let mut c = Vec::new();
    for (key, tag) in [
        ("node", "Node"),
        ("delimiter", "Delimiter"),
        ("piece", "Piece"),
        ("retrieval_code", "RetrievalCode"),
    ] {
        push_leaf(&mut c, tag, text(d, key, at)?);
    }
    c.extend(extra_nodes(d, at)?);
    Ok(branch("Data", Some(field.to_string()), c))
}

fn extra_nodes(o: &Obj, at: &str) -> Result<Vec<Node>, String> {
    list(o, "extra", at)?
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let here = format!("{at}/extra/{i}");
            let e = object(e, &here)?;
            known(e, &here, &["tag", "name", "value"])?;
            Ok(Node {
                tag: text(e, "tag", &here)?.ok_or(format!("{here}: no `tag`"))?,
                name: text(e, "name", &here)?,
                value: text(e, "value", &here)?,
                children: None,
            })
        })
        .collect()
}

fn branch(tag: &str, name: Option<String>, children: Vec<Node>) -> Node {
    Node {
        tag: tag.to_string(),
        name,
        value: None,
        children: Some(children),
    }
}

fn push_leaf(into: &mut Vec<Node>, tag: &str, value: Option<String>) {
    if let Some(value) = value {
        into.push(Node {
            tag: tag.to_string(),
            name: None,
            value: Some(value),
            children: None,
        });
    }
}

fn object<'a>(v: &'a serde_json::Value, at: &str) -> Result<&'a Obj, String> {
    v.as_object().ok_or(format!("{at}: expected an object"))
}

fn known(o: &Obj, at: &str, keys: &[&str]) -> Result<(), String> {
    match o.keys().find(|k| !keys.contains(&k.as_str())) {
        Some(k) => Err(format!("{at}: unknown key `{k}`")),
        None => Ok(()),
    }
}

/// A string value; `null` and absent are `None`.
fn text(o: &Obj, key: &str, at: &str) -> Result<Option<String>, String> {
    match o.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("{at}/{key}: expected text")),
    }
}

/// A positive integer, as the element's `name`.
fn number(o: &Obj, key: &str, at: &str) -> Result<String, String> {
    o.get(key)
        .and_then(serde_json::Value::as_u64)
        .filter(|n| *n > 0)
        .map(|n| n.to_string())
        .ok_or(format!("{at}/{key}: expected a positive integer"))
}

fn list<'a>(o: &'a Obj, key: &str, at: &str) -> Result<&'a [serde_json::Value], String> {
    match o.get(key) {
        None | Some(serde_json::Value::Null) => Ok(&[]),
        Some(serde_json::Value::Array(a)) => Ok(a),
        Some(_) => Err(format!("{at}/{key}: expected a list")),
    }
}
