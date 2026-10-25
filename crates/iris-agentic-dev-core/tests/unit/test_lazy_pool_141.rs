//! Spec 141 T187: the connection pool is built when a pool member is first needed, not when
//! `IrisTools` is constructed.
//!
//! Building it reads every member's password, which opens the OS keychain. `tool --list` built an
//! `IrisTools` and hung on the keychain's approval dialog for 20 minutes. The binary-level proof is
//! `bin_integration::test_no_eager_keychain_141`; these pin the mechanism in-process.

use iris_agentic_dev_core::iris::connection_pool::ConnectionPool;
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::json;
use std::sync::Arc;

fn is_pending(tools: &IrisTools) -> bool {
    tools.pool.read().unwrap().pending_load().is_some()
}

#[test]
fn a_new_tool_set_holds_an_unloaded_pool() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).unwrap();
    assert!(is_pending(&tools), "construction must not load the pool");
}

#[test]
fn the_first_pool_read_loads_it_once() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).unwrap();
    let first = tools.pool_ref();
    assert!(
        first.pending_load().is_none(),
        "pool_ref returns a loaded pool"
    );
    assert!(!is_pending(&tools));
    let second = tools.pool_ref();
    assert!(
        Arc::ptr_eq(&first, &second),
        "the second read must not load again"
    );
}

#[test]
fn an_unloaded_pool_remembers_the_config_file_it_will_load() {
    let path = std::path::PathBuf::from("/nonexistent/.iris-agentic-dev.toml");
    let pool = ConnectionPool::unloaded(Some(path.clone()));
    assert_eq!(pool.pending_load().unwrap().config_file, Some(path));
    assert!(ConnectionPool::empty().pending_load().is_none());
}

#[tokio::test]
async fn a_call_on_the_default_connection_leaves_the_pool_unloaded() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).unwrap();
    let _ = tools.call_gated("skill_list", json!({})).await;
    assert!(
        is_pending(&tools),
        "a call that names no server must not build the pool"
    );
}

#[tokio::test]
async fn a_call_that_names_a_server_loads_the_pool() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).unwrap();
    let _ = tools
        .call_gated(
            "iris_query",
            json!({"query": "SELECT 1", "server": "nobody"}),
        )
        .await;
    assert!(
        !is_pending(&tools),
        "naming a server has to look it up in the pool"
    );
}
