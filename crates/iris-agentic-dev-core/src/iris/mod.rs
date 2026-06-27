pub mod audit_log;
pub mod connection;
pub mod discovery;
pub mod server_manager;
pub mod vscode_config;
pub mod workspace_config;

pub use connection::{DiscoverySource, IrisConnection};
pub use discovery::{discover_iris, probe_atelier};
