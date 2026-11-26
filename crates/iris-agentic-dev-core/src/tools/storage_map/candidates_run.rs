//! Finding the candidate index globals for `propose` (spec 173, FR-001 to FR-004): the runner
//! half of `candidates.rs`. One IRIS call as the call's execute identity lists the namespace's
//! globals that begin with the master's name and, when the master's row ID is its first
//! subscript, the literal first subscripts of the master that hold sub-trees. The answer merges
//! those with the names the caller gave. A listing that fails is said, with IRIS's text, and the
//! other sources still count (FR-004).

use super::candidates::{
    dedupe, from_listing, from_parameter, from_sub_trees, listing_code_for, parse_listing,
    parse_sub_trees, sub_tree_code, Candidate, LISTING_QUERY,
};
use crate::iris::connection::IrisConnection;
use serde_json::{json, Value};

/// The candidates of one call and what the listing said.
pub struct Discovery {
    /// Names the caller gave first, then sub-trees inside the master, then names from the
    /// listing; each global and sub-tree once.
    pub candidates: Vec<Candidate>,
    /// `Some(text)` when the namespace could not be listed.
    pub listing_error: Option<String>,
    /// `{what, why}` for `not_inferred_detail`: the listing and the scan, when either fell short.
    pub details: Vec<Value>,
    /// Why the sub-trees inside the master were not looked for, when they were not.
    pub scan_skipped: Option<String>,
}

impl Discovery {
    /// Only the names the caller gave: no listing was asked for.
    pub fn explicit(names: &[String]) -> Self {
        Self {
            candidates: dedupe(from_parameter(names)),
            listing_error: None,
            details: Vec::new(),
            scan_skipped: None,
        }
    }

    /// `index_candidates` before the candidates themselves are added.
    pub fn listing_json(&self) -> Value {
        let mut v = match &self.listing_error {
            None => json!({"listing": "ok"}),
            Some(why) => json!({"listing": "unavailable", "listing_reason": why}),
        };
        if let Some(why) = &self.scan_skipped {
            v["sub_trees_not_looked_for"] = json!(why);
        }
        v
    }
}

fn detail(what: &str, why: String) -> Value {
    json!({"what": what, "why": why})
}

/// The class query the listing runs: [`LISTING_QUERY`], or the one a test installed.
fn listing_query() -> String {
    #[cfg(feature = "testing")]
    if let Some(q) = super::candidates::LISTING_QUERY_FOR_TESTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    {
        return q;
    }
    LISTING_QUERY.to_string()
}

/// The candidates for `master` (`^Name`) in `ns`. `scan` asks for the sub-trees inside the master
/// too: only a master whose row ID is its first subscript has a literal first subscript that is
/// not a row.
pub async fn discover(
    conn: &IrisConnection,
    client: &reqwest::Client,
    ns: &str,
    master: &str,
    scan: bool,
    explicit: &[String],
) -> Discovery {
    let mut out = Discovery::explicit(explicit);
    let code = match listing_code_for(master, &listing_query()).and_then(|l| {
        if scan {
            sub_tree_code(master).map(|s| format!("{l}\n{s}"))
        } else {
            Ok(l)
        }
    }) {
        Ok(code) => code,
        Err(why) => {
            out.listing_error = Some(why.clone());
            out.details
                .push(detail("namespace listing was not available", why));
            return out;
        }
    };
    let text = match conn
        .execute_via_generator_prefixed(&code, ns, client, "IrisDevSm")
        .await
    {
        Ok(text) => text,
        Err(e) => {
            let why = format!("the listing could not run: {e}");
            out.listing_error = Some(why.clone());
            out.details
                .push(detail("namespace listing was not available", why));
            return out;
        }
    };
    let mut listed = Vec::new();
    match parse_listing(&text) {
        Ok(names) => listed = from_listing(master, &names),
        Err(why) => {
            out.details.push(detail(
                "namespace listing was not available",
                format!("{why}; globals whose names begin with the master's were not looked for"),
            ));
            out.listing_error = Some(why);
        }
    }
    let mut inside = Vec::new();
    if scan {
        match parse_sub_trees(&text) {
            Ok(found) => {
                let (cands, skipped) = from_sub_trees(master, &found.names);
                inside = cands;
                if skipped > 0 {
                    out.details.push(detail(
                        "sub-trees inside the master",
                        format!(
                            "{skipped} have names a map cannot spell as a literal (empty, over 64 \
                             characters, a quote or a control character) and were left out"
                        ),
                    ));
                }
                if found.capped {
                    out.details.push(detail(
                        "sub-trees inside the master",
                        "the scan of the master's string subscripts stopped at its cap, so there \
                         may be more than the ones listed"
                            .into(),
                    ));
                }
            }
            Err(why) => out.details.push(detail(
                "sub-trees inside the master",
                format!("they could not be looked for: {why}"),
            )),
        }
    } else {
        out.scan_skipped =
            Some("looked for only when the master's row ID is its first subscript".into());
    }
    out.candidates.extend(inside);
    out.candidates.extend(listed);
    out.candidates = dedupe(std::mem::take(&mut out.candidates));
    out
}
