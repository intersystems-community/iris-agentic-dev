//! A Live instance must read as Live. The Management Portal and `Config.Startup` store
//! `^%SYS("SystemMode")` as `LIVE`, and iad matched `Live`, so `probe()` reported Unknown and the
//! write gate fell to the namespace heuristic: writes on in USER on a production system (spec 140
//! triage, PLATFORM-22). This sets the mode for a moment, probes, and puts the old value back
//! before asserting anything.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection, SystemMode};

fn conn() -> IrisConnection {
    let get = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let host = std::env::var("IRIS_HOST").expect("IRIS_HOST names the live server");
    IrisConnection::new(
        format!("http://{host}:{}", get("IRIS_WEB_PORT", "52780")),
        "USER",
        get("IRIS_USERNAME", "_SYSTEM"),
        get("IRIS_PASSWORD", "SYS"),
        DiscoverySource::EnvVar,
    )
}

async fn set_mode(c: &IrisConnection, client: &reqwest::Client, mode: &str) -> String {
    let code = format!(
        " Set old=##class(Config.Startup).Open().SystemMode\n Set p(\"SystemMode\")=\"{mode}\"\n Set sc=##class(Config.Startup).Modify(.p)\n Write:'sc \"ERROR: \",$System.Status.GetErrorText(sc)\n Write:sc old"
    );
    c.execute_via_generator(&code, "%SYS", client)
        .await
        .expect("set SystemMode")
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris); sets SystemMode to LIVE for a moment and restores it"]
async fn a_live_instance_reads_as_live() {
    let client = reqwest::Client::new();
    let admin = conn();
    let old = set_mode(&admin, &client, "LIVE").await;
    assert!(!old.starts_with("ERROR"), "could not set the mode: {old}");

    let mut probed = conn();
    probed.probe().await;

    let back = set_mode(&admin, &client, old.trim()).await;
    assert!(back.trim() == "LIVE", "restore of {old:?} failed: {back}");
    assert_eq!(probed.system_mode, SystemMode::Live);
}
