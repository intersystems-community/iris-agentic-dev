//! The master `propose` links index globals against (spec 173, FR-001a, FR-005, FR-015a): either
//! the one `propose` drafts from a sample of the master global (`MasterSpec::from_draft`) or the
//! master map of a class that already has storage (`MasterSpec::from_class`). Pure: no IRIS call.
//!
//! Both paths give the same shape, so the linker and the drafting code do not care which one made
//! it: the global, the master's subscripts, the names of its key parts, and its fields with the
//! delimiter and piece (or node) each one is read from.

use super::model::{DataField, RowIdPart, StorageModel};
use super::shape::{draft, ShapeReport};
use std::collections::HashSet;

/// The longest a property name may be.
const MAX_PROPERTY_NAME: usize = 128;

/// Why a master could not be built: an `INVALID_PARAMS`-style text for a bad call, or the
/// `STORAGE_MAP_READ_ONLY` text `read` gives for storage iad only reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Refusal {}

fn invalid(message: String) -> Refusal {
    Refusal {
        code: "INVALID_PARAMS",
        message,
    }
}

fn read_only(message: String) -> Refusal {
    Refusal {
        code: "STORAGE_MAP_READ_ONLY",
        message,
    }
}

/// A valid property name: a letter or `%` first, then letters and digits, at most 128 characters.
/// ASCII only, because the name goes into generated class source. A lone `%` is not a name.
pub fn is_valid_property_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_PROPERTY_NAME {
        return false;
    }
    let body = name.strip_prefix('%').unwrap_or(name);
    body.starts_with(|c: char| c.is_ascii_alphabetic())
        && body.chars().all(|c| c.is_ascii_alphanumeric())
}

/// One field of the master: where its value is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MasterField {
    /// The name drafts use: the caller's (`field_names`), the class's own, or the placeholder.
    pub name: String,
    /// The placeholder the master draft gave the field (`Field1`, `Contact2`); the class's own
    /// name in class mode.
    pub placeholder: String,
    /// The sub-node the field lives under, as a quoted literal (`"contact"`), when it has one.
    pub node: Option<String>,
    /// The delimiter expression (`"^"`); `None` for a `$LIST` element or a whole node.
    pub delimiter: Option<String>,
    /// The piece or element number, as the map spells it.
    pub piece: Option<String>,
    /// `$LIST`, `delimited` or `whole node`.
    pub format: String,
    /// The name or the delimiter and piece came from a guess, not from a class.
    pub guessed: bool,
}

/// The master map as the linker needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct MasterSpec {
    /// `^Name`.
    pub global: String,
    /// Class mode: the class whose master this is.
    pub class: Option<String>,
    /// The master map's name (`Master` for a draft).
    pub map_name: String,
    /// The master's subscript expressions in level order (`{BookId}`, `{ID}`, a constant).
    pub subscripts: Vec<String>,
    /// The names of the key parts, one per subscript that is a field reference: the RowIdSpec
    /// fields when the map has them, else the names inside the braces.
    pub key_names: Vec<String>,
    /// The map's RowIdSpec parts (class mode; empty for a draft and for a `{ID}` master).
    pub row_id: Vec<RowIdPart>,
    pub fields: Vec<MasterField>,
}

impl MasterSpec {
    /// The key's length: how many subscripts hold the row ID (FR-005's `k`).
    pub fn key_count(&self) -> usize {
        self.key_names.len()
    }

    /// The master `propose` drafts from `report` (`shape::draft`), with `field_names` renaming
    /// its fields by position. `Field1…n` stay when the list is empty. A list shorter than the
    /// field count, a name that is not a property name, and a duplicate are `INVALID_PARAMS`;
    /// names past the field count are ignored.
    pub fn from_draft(report: &ShapeReport, field_names: &[String]) -> Result<Self, Refusal> {
        let (map, guesses) = draft(report);
        let subscripts: Vec<String> = map["subscripts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|s| s["expression"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let key_names = brace_names(&subscripts);
        // `shape::draft` returns its data map sorted by name, and its guesses in the order it
        // made the fields: row pieces first, then each sub-node's. Field position is that order.
        let prefix = "/maps/Master/data/";
        let placeholders: Vec<&str> = guesses
            .iter()
            .filter_map(|g| g.strip_prefix(prefix))
            .collect();
        let chosen = choose_names(&placeholders, field_names, &key_names)?;
        let fields = placeholders
            .iter()
            .zip(chosen)
            .map(|(placeholder, name)| {
                let f = &map["data"][*placeholder];
                let delimiter = f["delimiter"].as_str().map(String::from);
                MasterField {
                    name,
                    placeholder: (*placeholder).to_string(),
                    node: f["node"].as_str().map(String::from),
                    format: if delimiter.is_some() {
                        "delimited".into()
                    } else {
                        "$LIST".into()
                    },
                    delimiter,
                    piece: f["piece"].as_str().map(String::from),
                    guessed: true,
                }
            })
            .collect();
        Ok(Self {
            global: report.global.clone(),
            class: None,
            map_name: "Master".into(),
            subscripts,
            key_names,
            row_id: Vec::new(),
            fields,
        })
    }

    /// Rename the fields of the master draft (and the paths of its guesses) to this spec's names:
    /// the draft calls them by placeholder (`Field1`), the spec by what `field_names` gave.
    pub fn apply_names(&self, draft: &mut serde_json::Value, guesses: &mut [String]) {
        let Some(data) = draft["data"].as_object().cloned() else {
            return;
        };
        let mut renamed = serde_json::Map::new();
        for (key, mut field) in data {
            let name = self
                .fields
                .iter()
                .find(|f| f.placeholder == key)
                .map_or(key.clone(), |f| f.name.clone());
            field["name"] = serde_json::json!(name);
            renamed.insert(name, field);
        }
        draft["data"] = serde_json::Value::Object(renamed);
        for g in guesses.iter_mut() {
            if let Some(ph) = g.strip_prefix("/maps/Master/data/") {
                if let Some(f) = self.fields.iter().find(|f| f.placeholder == ph) {
                    *g = format!("/maps/Master/data/{}", f.name);
                }
            }
        }
    }

    /// The master of a class: its first `data` map, read as `read` reads it. Refused with the
    /// reasons `read` gives (`STORAGE_MAP_READ_ONLY`): default storage or any other storage iad
    /// only reads, and a columnar map; a class with no `data` map has no master to link against.
    pub fn from_class(model: &StorageModel) -> Result<Self, Refusal> {
        let class = &model.class;
        if !model.storage.editable {
            let why = model
                .storage
                .not_editable_reason
                .as_deref()
                .unwrap_or_default();
            return Err(read_only(format!(
                "the storage of {class} is read-only in iris_storage_map: {why}"
            )));
        }
        if let Some(col) = model
            .storage
            .maps
            .iter()
            .find(|m| m.r#type.eq_ignore_ascii_case("columnar"))
        {
            return Err(read_only(format!(
                "the storage of {class} is read-only in iris_storage_map: its map {} is columnar",
                col.name
            )));
        }
        let Some(master) = model.storage.maps.iter().find(|m| m.r#type == "data") else {
            return Err(invalid(format!(
                "class {class} has no data map, so there is no master to link an index against"
            )));
        };
        let subscripts: Vec<String> = master
            .subscripts
            .iter()
            .map(|s| s.expression.clone())
            .collect();
        let from_row_id: Vec<String> = master
            .row_id
            .iter()
            .filter_map(|p| p.field.clone().or_else(|| p.expression.clone()))
            .collect();
        let key_names = if from_row_id.len() == master.row_id.len() && !from_row_id.is_empty() {
            from_row_id
        } else {
            brace_names(&subscripts)
        };
        let fields = master
            .data
            .values()
            .map(|d: &DataField| MasterField {
                name: d.name.clone(),
                placeholder: d.name.clone(),
                node: d.node.clone(),
                delimiter: d.delimiter.clone(),
                piece: d.piece.clone(),
                format: d.derived.format.clone(),
                guessed: false,
            })
            .collect();
        Ok(Self {
            global: master.global.clone(),
            class: Some(class.clone()),
            map_name: master.name.clone(),
            subscripts,
            key_names,
            row_id: master.row_id.clone(),
            fields,
        })
    }
}

/// The names inside the braces of the subscripts that are a single field reference (`{ID}`);
/// a constant or a computed subscript is not a key part.
fn brace_names(subscripts: &[String]) -> Vec<String> {
    subscripts
        .iter()
        .filter_map(|s| {
            let inner = s.strip_prefix('{')?.strip_suffix('}')?;
            (!inner.is_empty() && inner.chars().all(|c| c.is_ascii_alphanumeric() || c == '%'))
                .then(|| inner.to_string())
        })
        .collect()
}

/// The names for `placeholders`, by position: the caller's when given, else the placeholders.
fn choose_names(
    placeholders: &[&str],
    given: &[String],
    key_names: &[String],
) -> Result<Vec<String>, Refusal> {
    if given.is_empty() {
        return Ok(placeholders.iter().map(|p| (*p).to_string()).collect());
    }
    let found = placeholders.len();
    if given.len() < found {
        return Err(invalid(format!(
            "field_names has {} name{} but the sample found {found} field{}: give one name per \
             field, in field order",
            given.len(),
            if given.len() == 1 { "" } else { "s" },
            if found == 1 { "" } else { "s" },
        )));
    }
    if let Some(bad) = given.iter().find(|n| !is_valid_property_name(n)) {
        return Err(invalid(format!(
            "field_names: {bad:?} is not a valid property name (a letter or %, then letters and \
             digits, at most {MAX_PROPERTY_NAME} characters)"
        )));
    }
    let mut seen: HashSet<String> = key_names.iter().map(|k| k.to_ascii_lowercase()).collect();
    for name in &given[..found] {
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(invalid(format!(
                "field_names: {name:?} is a duplicate (names compare without case, and the key \
                 parts {key_names:?} are taken)"
            )));
        }
    }
    Ok(given[..found].to_vec())
}
