pub mod benchmark;
pub mod elicitation;
pub mod iris;
pub mod manifest;
pub mod objectscript;
pub mod policy;
pub mod skill_install;
pub mod skills;
pub mod sql_lex;
pub mod telemetry;
pub mod tools;

pub mod generate;

/// Helpers shared by the test binaries. Not part of the shipped surface.
#[cfg(feature = "testing")]
pub mod testing;

// The unit tests inside `src/` run in their own binary, which no aggregator's `main.rs` reaches.
// Same reason as the aggregators: isolate before `main` (spec 141 T183).
#[cfg(all(test, feature = "testing"))]
#[ctor::ctor]
fn isolate_the_lib_test_process() {
    testing::isolate_process();
}
