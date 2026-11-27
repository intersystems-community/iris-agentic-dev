//! Spec 173, T073 to T075 (feedback on the 172 pre-release): what an answer says about the map and
//! the data, for the reader of the answer. Presentation only: nothing here changes the model, the
//! tree or what `edit` and `replace` write.

use super::model::{level_signs, StorageModel};
use super::shape::ShapeReport;
use serde_json::{json, Value};

/// The `storage` a `read` answer carries: the model's, without a keyword the compiler added and
/// the class does not use (`compiler_added` names it, so nothing is hidden silently). The model
/// itself still holds the element, so the lossless tree and every write keep it.
pub fn read_storage(m: &StorageModel) -> Value {
    let mut storage = json!(m.storage);
    if let Some(keywords) = storage["keywords"].as_object_mut() {
        for name in &m.compiler_added {
            keywords.remove(name);
        }
    }
    storage
}

/// One line per level a map computes by code and per access variable it sets by code, from the
/// same reading of the map as the `derived` facts (`model::level_signs`): which map and level,
/// what iad does not evaluate, and that `compare` samples the real global to confirm.
pub fn computed_warnings(m: &StorageModel) -> Vec<String> {
    let mut lines = Vec::new();
    for map in &m.storage.maps {
        for sub in &map.subscripts {
            let signs = level_signs(sub);
            for var in &signs.vars {
                lines.push(format!(
                    "map {}, level {}: access variable {} is set by code, not read from the global, and iad does not evaluate that code; `compare` samples the real global to confirm what the level holds",
                    map.name, sub.level, var.variable
                ));
            }
            if signs.filter_only {
                lines.push(format!(
                    "map {}, level {} filters its keys by code (Invalidcondition): iad does not evaluate it, so which keys the map skips is not known here; `compare` samples the real global to confirm",
                    map.name, sub.level
                ));
            } else if !signs.computed.is_empty() {
                lines.push(format!(
                    "map {}, level {} is computed by code ({}): iad does not evaluate that code, so the level may not be a subscript of the global (a $BIT or $LIST value is a common reason); check it with `compare`, which samples the real global to confirm",
                    map.name,
                    sub.level,
                    signs.computed.join(", ")
                ));
            }
        }
    }
    lines
}

/// `markdown` with one visible `**Warning:**` line per warning above it; unchanged without any.
pub fn with_warning_lines(markdown: &str, warnings: &[String]) -> String {
    if warnings.is_empty() {
        return markdown.to_string();
    }
    let mut out = String::new();
    for w in warnings {
        out.push_str("**Warning:** ");
        out.push_str(w);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(markdown);
    out
}

/// The keys an answer carries when the sampler found no node in the global: it looked, and the
/// global holds nothing, so nothing was checked against data. `None` when nodes were sampled.
pub fn no_data_fields(report: &ShapeReport, ns: &str) -> Option<Value> {
    (report.nodes_scanned == 0).then(|| {
        json!({
            "no_data": true,
            "sampled_nodes": 0,
            "message": format!(
                "looked at {} in {ns}: it holds no data, so nothing here is checked against data",
                report.global
            ),
        })
    })
}

/// Copy the keys of `extra` (an object, or nothing) into `answer`.
pub fn merge(answer: &mut Value, extra: Option<Value>) {
    if let (Some(target), Some(Value::Object(fields))) = (answer.as_object_mut(), extra) {
        target.extend(fields);
    }
}
