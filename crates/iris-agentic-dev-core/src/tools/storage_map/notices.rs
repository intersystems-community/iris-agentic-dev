//! Spec 173, T073 to T075: what an answer says about the map and the data, for a reader.

use super::model::StorageModel;
use super::shape::ShapeReport;
use serde_json::Value;

pub fn read_storage(m: &StorageModel) -> Value {
    serde_json::json!(m.storage)
}

pub fn computed_warnings(_m: &StorageModel) -> Vec<String> {
    Vec::new()
}

pub fn with_warning_lines(markdown: &str, _warnings: &[String]) -> String {
    markdown.to_string()
}

pub fn no_data_fields(_report: &ShapeReport, _ns: &str) -> Option<Value> {
    None
}
