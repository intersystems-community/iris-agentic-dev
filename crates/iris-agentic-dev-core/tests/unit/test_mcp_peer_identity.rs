// MCP peer task-local: absent outside a tool call, present with name+version inside one,
// and None when the peer sent no clientInfo.
//
// The task-local `MCP_PEER` must be task-scoped (not global) because the HTTP transport
// clones one IrisTools struct across sessions — a global would let one session's peer bleed
// into another's marker (plan.md research R3).

use iris_agentic_dev_core::tools::{mcp_peer, MCP_PEER};

#[tokio::test]
async fn mcp_peer_is_absent_outside_a_tool_call() {
    // Outside a scope() the accessor must return None — callers like user_agent() that read
    // the peer must not panic when called outside a tool call.
    assert!(
        mcp_peer().is_none(),
        "outside a tool call scope there is no peer: got {:?}",
        mcp_peer()
    );
}

#[tokio::test]
async fn mcp_peer_is_present_inside_scope() {
    let peer = Some(("claude-code".to_string(), "2.1.0".to_string()));
    MCP_PEER
        .scope(peer.clone(), async {
            let got = mcp_peer();
            assert_eq!(
                got, peer,
                "inside a scope the peer should be visible: {:?}",
                got
            );
        })
        .await;
}

#[tokio::test]
async fn mcp_peer_none_when_no_client_info() {
    // When the client sent no clientInfo at initialize, we set None in the scope.
    MCP_PEER
        .scope(None::<(String, String)>, async {
            let got = mcp_peer();
            assert!(
                got.is_none(),
                "no clientInfo should stay None inside scope: {:?}",
                got
            );
        })
        .await;
}

#[tokio::test]
async fn mcp_peer_does_not_bleed_across_tasks() {
    // Each task must see its own peer, not another task's.
    let peer_a = Some(("claude-code".to_string(), "2.1.0".to_string()));
    let peer_b = Some(("cursor".to_string(), "0.99.0".to_string()));

    let task_a = tokio::spawn(MCP_PEER.scope(peer_a.clone(), async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        mcp_peer()
    }));
    let task_b = tokio::spawn(MCP_PEER.scope(peer_b.clone(), async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        mcp_peer()
    }));

    let got_a = task_a.await.unwrap();
    let got_b = task_b.await.unwrap();
    assert_eq!(got_a, peer_a, "task A should see peer_a, not peer_b");
    assert_eq!(got_b, peer_b, "task B should see peer_b, not peer_a");
}
