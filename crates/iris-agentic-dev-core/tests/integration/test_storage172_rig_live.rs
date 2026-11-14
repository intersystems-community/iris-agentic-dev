//! Spec 172 fixtures, each loaded on live IRIS, round-tripped byte for byte and asked its known
//! question (T011 onward). Run:
//! `cargo test --features testing --test integration test_storage172_rig_live -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Where};
use crate::storage172_rig::check_fixture;
use iris_agentic_dev_core::tools::Toolset;

macro_rules! fixture_case {
    ($test:ident, $name:literal) => {
        #[tokio::test]
        #[ignore]
        async fn $test() {
            let r = rig(&Where::live(), Toolset::Merged).await;
            check_fixture(&r, $name).await;
        }
    };
}

fixture_case!(member, "Member");
fixture_case!(loan_list, "LoanList");
fixture_case!(book_multi, "BookMulti");
fixture_case!(branch, "Branch");
fixture_case!(shelf, "Shelf");
fixture_case!(member_by_city, "MemberByCity");
fixture_case!(active_loan, "ActiveLoan");
fixture_case!(genre, "Genre");
fixture_case!(loan, "Loan");
fixture_case!(loan_event, "LoanEvent");
fixture_case!(member_d11, "MemberD11");
fixture_case!(copy, "Copy");
fixture_case!(loan_union, "LoanUnion");
fixture_case!(notes, "Notes");
fixture_case!(default_book, "DefaultBook");
// T101a–e: patterns from CORPUS.md.
fixture_case!(ledger, "Ledger");
fixture_case!(route, "Route");
fixture_case!(shipment, "Shipment");
fixture_case!(badge, "Badge");
