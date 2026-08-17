//! Regression test for 076-interface-modernization User Story 4: `list_tools` pagination.
//!
//! This exercises `paginate_tool_list` directly — the pure function `list_tools` calls
//! after computing the full catalog and normalizing schemas — rather than the
//! `ServerHandler::list_tools` trait method itself, which needs a live
//! `rmcp::service::RequestContext` to invoke. `paginate_tool_list` has no dependency on
//! IRIS, the router, or any request context, so a direct unit test is both simpler and a
//! more precise regression backstop than round-tripping through the full trait method
//! would be.

use iris_agentic_dev_core::tools::paginate_tool_list;
use rmcp::model::Tool;
use std::sync::Arc;

/// Build `n` dummy tools named `t0000`, `t0001`, … — zero-padded so lexicographic order
/// (which is what `ToolRouter::list_all()` actually sorts by) matches numeric order,
/// making assertions about "the first page" / "the Nth page" straightforward.
fn dummy_tools(n: usize) -> Vec<Tool> {
    (0..n)
        .map(|i| Tool::new(format!("t{i:04}"), "", Arc::new(serde_json::Map::new())))
        .collect()
}

#[test]
fn test_no_cursor_first_page_starts_at_zero() {
    let tools = dummy_tools(10);
    let (page, next) = paginate_tool_list(tools, None, 4);
    let names: Vec<_> = page.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names, vec!["t0000", "t0001", "t0002", "t0003"]);
    assert_eq!(next, Some("4".to_string()));
}

#[test]
fn test_pages_cover_the_whole_list_with_no_overlap_or_gap() {
    let total = 23;
    let page_size = 7;
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let (page, next) = paginate_tool_list(dummy_tools(total), cursor.as_deref(), page_size);
        assert!(
            !page.is_empty() || next.is_none(),
            "an empty page must be the terminal page"
        );
        seen.extend(page.iter().map(|t| t.name.to_string()));
        match next {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    // Every tool name appears exactly once, and the full set matches — no duplicate, no
    // omission, regardless of page_size not evenly dividing total.
    let mut expected: Vec<String> = (0..total).map(|i| format!("t{i:04}")).collect();
    let mut actual = seen;
    expected.sort();
    actual.sort();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), total, "no duplicates and no omissions");
}

#[test]
fn test_last_page_has_no_next_cursor() {
    let tools = dummy_tools(10);
    // Page size divides evenly — the last page ends exactly at the list boundary.
    let (page, next) = paginate_tool_list(tools, Some("8"), 2);
    assert_eq!(page.len(), 2);
    assert!(next.is_none());
}

#[test]
fn test_page_size_larger_than_total_returns_everything_in_one_page() {
    let tools = dummy_tools(5);
    let (page, next) = paginate_tool_list(tools, None, 100);
    assert_eq!(page.len(), 5);
    assert!(next.is_none());
}

#[test]
fn test_cursor_at_exact_end_returns_empty_final_page() {
    let tools = dummy_tools(6);
    let (page, next) = paginate_tool_list(tools, Some("6"), 3);
    assert!(page.is_empty());
    assert!(next.is_none());
}

#[test]
fn test_malformed_cursor_degrades_to_start_from_beginning() {
    let tools = dummy_tools(5);
    let (page, _next) = paginate_tool_list(tools, Some("not-a-number"), 3);
    let names: Vec<_> = page.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(
        names,
        vec!["t0000", "t0001", "t0002"],
        "an unparseable cursor must restart from the beginning, not panic or skip"
    );
}

#[test]
fn test_out_of_range_cursor_degrades_to_start_from_beginning() {
    // A cursor pointing past the end of a catalog that shrank since it was minted (e.g. an
    // IRIS_ENABLED_TOOLS change between requests) must not panic or produce a nonsensical
    // permanent empty page — it restarts instead.
    let tools = dummy_tools(5);
    let (page, _next) = paginate_tool_list(tools, Some("999"), 3);
    let names: Vec<_> = page.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names, vec!["t0000", "t0001", "t0002"]);
}

#[test]
fn test_empty_catalog_returns_empty_page_and_no_cursor() {
    let (page, next) = paginate_tool_list(vec![], None, 10);
    assert!(page.is_empty());
    assert!(next.is_none());
}

#[test]
fn test_page_size_zero_is_treated_as_at_least_one() {
    // page_size is only ever server-configured (IRIS_LIST_TOOLS_PAGE_SIZE via
    // read_inline_threshold, which already floors at 1 for a configured value of 0), but
    // paginate_tool_list floors it too so it can never wedge into an infinite loop of
    // empty-page-with-next-cursor on a directly-called page_size of 0.
    let tools = dummy_tools(3);
    let (page, next) = paginate_tool_list(tools, None, 0);
    assert_eq!(page.len(), 1);
    assert_eq!(next, Some("1".to_string()));
}
