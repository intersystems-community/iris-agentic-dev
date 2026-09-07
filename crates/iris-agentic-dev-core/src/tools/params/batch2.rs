//! Batch 2: namespace and database administration, plus container selection.
//!
//! `server` appears four times here and 20-odd times across the whole surface. It is redeclared per
//! struct rather than factored into a shared `#[serde(flatten)]` base: flattening is incompatible
//! with `#[serde(deny_unknown_fields)]` in serde, and the closed parameter set is the point of this
//! feature.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters of `iris_namespace_list`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisNamespaceListParams {
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `iris_namespace_create`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisNamespaceCreateParams {
    /// Name of the namespace to create.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Existing database to use for globals. Defaults to a database named after the namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_path: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `iris_database_list`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisDatabaseListParams {
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `iris_database_stats`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisDatabaseStatsParams {
    /// Database directory to report on. Omit for every database.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `iris_containers`.
///
/// No `server`: this tool works on Docker containers, not on registered IRIS instances.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisContainersParams {
    /// `list`, `select`, or `start`. Defaults to `list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("enum" = ["list", "select", "start"]))]
    pub action: Option<String>,
    /// Container name, for `select` and `start`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}
