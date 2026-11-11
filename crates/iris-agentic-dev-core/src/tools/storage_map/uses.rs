//! Finding classes that use a global (spec 172 US2, research R11).

use super::globals::{normalize_global, GlobalRef};
use crate::iris::connection::IrisConnection;
use serde_json::{json, Value};

/// A use of a global by a storage map.
#[derive(Debug, Clone)]
pub struct Use {
    pub namespace: String,
    pub class: String,
    pub storage: String,
    pub map: String,
    pub r#type: String,
}

/// Finds all classes whose storage maps use the given global.
///
/// Searches %Dictionary.CompiledStorageSQLMap for data/index/other maps,
/// plus %Dictionary.CompiledStorage for default storage locations.
pub async fn uses(
    iris: &IrisConnection,
    client: &reqwest::Client,
    global: &GlobalRef,
    namespaces: &[String],
) -> Result<Vec<Use>, String> {
    let mut results = Vec::new();
    let target = global.name.as_str();

    for ns in namespaces {
        let sql = "SELECT parent, Name, Type, _Global FROM %Dictionary.CompiledStorageSQLMap WHERE _Global = ?";
        let resp = iris
            .query(sql, vec![json!(target)], ns, client)
            .await
            .map_err(query_failed)?;

        for row in rows(&resp) {
            let (Some(parent), Some(map_name), Some(map_type), Some(found_global)) = (
                row["parent"].as_str(),
                row["Name"].as_str(),
                row["Type"].as_str(),
                row["_Global"].as_str(),
            ) else {
                continue;
            };
            if !same_global(found_global, target) {
                continue;
            }
            // parent is "Class||StorageName||MapName".
            let mut parts = parent.split("||");
            if let (Some(class), Some(storage)) = (parts.next(), parts.next()) {
                results.push(Use {
                    namespace: ns.clone(),
                    class: class.to_string(),
                    storage: storage.to_string(),
                    map: map_name.to_string(),
                    r#type: map_type.to_lowercase(),
                });
            }
        }

        // Default storage locations.
        let sql = "SELECT parent, DataLocation, IdLocation, IndexLocation FROM %Dictionary.CompiledStorage";
        let resp = iris
            .query(sql, vec![], ns, client)
            .await
            .map_err(query_failed)?;

        for row in rows(&resp) {
            let Some(class) = row["parent"].as_str() else {
                continue;
            };
            for (location_type, location_field) in [
                ("data", "DataLocation"),
                ("id", "IdLocation"),
                ("index", "IndexLocation"),
            ] {
                let location = row[location_field].as_str().unwrap_or("");
                if !location.is_empty() && same_global(location, target) {
                    results.push(Use {
                        namespace: ns.clone(),
                        class: class.to_string(),
                        storage: "default".to_string(),
                        map: format!("default_{}", location_type),
                        r#type: location_type.to_string(),
                    });
                }
            }
        }
    }

    Ok(results)
}

/// The rows of an Atelier query response.
fn rows(resp: &Value) -> impl Iterator<Item = &Value> {
    resp["result"]["content"].as_array().into_iter().flatten()
}

/// Does the global reference `raw` name the global `target` (a normalized `^Name`)?
fn same_global(raw: &str, target: &str) -> bool {
    normalize_global(raw).is_ok_and(|r| r.name == target)
}

fn query_failed(e: anyhow::Error) -> String {
    format!("Query failed: {:?}", e)
}

/// Converts a Use into JSON for the response.
pub fn use_to_json(u: &Use) -> Value {
    json!({
        "namespace": u.namespace,
        "class": u.class,
        "storage": u.storage,
        "map": u.map,
        "type": u.r#type,
    })
}
