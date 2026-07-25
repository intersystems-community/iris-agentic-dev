//! Unit tests for public functions and parameter deserialization in skills_tools.rs
//! Focuses on coverage gaps for namespace handling and parameter defaults.

#[cfg(test)]
mod tests {
    use iris_agentic_dev_core::tools::skills_tools::{
        skills_namespace, AgentInfoParams, KbParams, SkillCommunityParams, SkillParams,
    };

    /// `OBJECTSCRIPT_SKILLMCP_NAMESPACE` is process-global while `cargo test` runs this
    /// binary's tests on parallel threads, so concurrent set/remove calls clobber each other.
    /// Every test below reads the var through `skills_namespace()` and must hold this lock for
    /// the whole set → read → restore sequence. Poison is recovered so one panicking test does
    /// not fail every other test in the file.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // ── skills_namespace() - public function ──────────────────────────────────

    #[test]
    fn test_skills_namespace_absent_default() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE");
        let ns = skills_namespace();
        assert_eq!(ns, "USER", "Missing env var should default to USER");
    }

    #[test]
    fn test_skills_namespace_custom_value() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE", "MYAPP");
        let ns = skills_namespace();
        std::env::remove_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE");
        assert_eq!(ns, "MYAPP");
    }

    #[test]
    fn test_skills_namespace_empty_string() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE", "");
        let ns = skills_namespace();
        std::env::remove_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE");
        assert_eq!(ns, "", "Empty string should be preserved");
    }

    #[test]
    fn test_skills_namespace_with_special_chars() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE", "PROD-ENV_1");
        let ns = skills_namespace();
        std::env::remove_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE");
        assert_eq!(ns, "PROD-ENV_1");
    }

    #[test]
    fn test_skills_namespace_case_preserved() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE", "MixedCase");
        let ns = skills_namespace();
        std::env::remove_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE");
        assert_eq!(ns, "MixedCase", "Case should be preserved");
    }

    // ── Namespace reset cycle ────────────────────────────────────────────────

    #[test]
    fn test_skills_namespace_reset_cycle() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE", "TEMP");
        let set = skills_namespace();
        std::env::remove_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE");
        let cleared = skills_namespace();
        assert_eq!(set, "TEMP");
        assert_eq!(cleared, "USER");
    }

    #[test]
    fn test_skills_namespace_multiple_values() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let values = vec!["NS1", "NS2", "NS3"];
        let mut seen = Vec::new();
        for val in &values {
            std::env::set_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE", val);
            seen.push(skills_namespace());
        }
        std::env::remove_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE");
        assert_eq!(seen, values);
    }

    // ── Serde deserialization - default_top_k via KbParams ────────────────────

    #[test]
    fn test_kb_params_top_k_default() {
        let p: KbParams = serde_json::from_str(r#"{"action": "recall", "query": "test"}"#).unwrap();
        assert_eq!(p.top_k, 5, "default_top_k should be 5");
    }

    #[test]
    fn test_kb_params_top_k_positive() {
        let p: KbParams = serde_json::from_str(r#"{"action": "recall", "query": "test"}"#).unwrap();
        assert!(p.top_k > 0, "default_top_k should be positive");
    }

    #[test]
    fn test_kb_params_top_k_reasonable() {
        let p: KbParams = serde_json::from_str(r#"{"action": "recall", "query": "test"}"#).unwrap();
        assert!(p.top_k <= 100, "default_top_k should be reasonable (<=100)");
    }

    #[test]
    fn test_kb_params_top_k_override() {
        let p: KbParams =
            serde_json::from_str(r#"{"action": "recall", "query": "test", "top_k": 15}"#).unwrap();
        assert_eq!(p.top_k, 15, "top_k should use provided value");
    }

    #[test]
    fn test_kb_params_top_k_zero() {
        let p: KbParams =
            serde_json::from_str(r#"{"action": "recall", "query": "test", "top_k": 0}"#).unwrap();
        assert_eq!(p.top_k, 0);
    }

    #[test]
    fn test_kb_params_top_k_large() {
        let p: KbParams =
            serde_json::from_str(r#"{"action": "recall", "query": "test", "top_k": 1000}"#)
                .unwrap();
        assert_eq!(p.top_k, 1000);
    }

    // ── Serde deserialization - default_limit via AgentInfoParams ─────────────

    #[test]
    fn test_agent_info_params_limit_default() {
        let p: AgentInfoParams = serde_json::from_str(r#"{"what": "stats"}"#).unwrap();
        assert_eq!(p.limit, 20, "default_limit should be 20");
    }

    #[test]
    fn test_agent_info_params_limit_positive() {
        let p: AgentInfoParams = serde_json::from_str(r#"{"what": "stats"}"#).unwrap();
        assert!(p.limit > 0, "default_limit should be positive");
    }

    #[test]
    fn test_agent_info_params_limit_reasonable() {
        let p: AgentInfoParams = serde_json::from_str(r#"{"what": "stats"}"#).unwrap();
        assert!(p.limit <= 100, "default_limit should be reasonable (<=100)");
    }

    #[test]
    fn test_agent_info_params_limit_override() {
        let p: AgentInfoParams =
            serde_json::from_str(r#"{"what": "history", "limit": 50}"#).unwrap();
        assert_eq!(p.limit, 50, "limit should use provided value");
    }

    #[test]
    fn test_agent_info_params_limit_zero() {
        let p: AgentInfoParams =
            serde_json::from_str(r#"{"what": "history", "limit": 0}"#).unwrap();
        assert_eq!(p.limit, 0);
    }

    #[test]
    fn test_agent_info_params_limit_very_large() {
        let p: AgentInfoParams =
            serde_json::from_str(r#"{"what": "history", "limit": 999999}"#).unwrap();
        assert_eq!(p.limit, 999999);
    }

    // ── SkillParams variants ──────────────────────────────────────────────────

    #[test]
    fn test_skill_params_list_action() {
        let p: SkillParams = serde_json::from_str(r#"{"action": "list"}"#).unwrap();
        assert_eq!(p.action, "list");
        assert!(p.name.is_none());
        assert!(p.query.is_none());
    }

    #[test]
    fn test_skill_params_describe_action() {
        let p: SkillParams =
            serde_json::from_str(r#"{"action": "describe", "name": "iris-compile"}"#).unwrap();
        assert_eq!(p.action, "describe");
        assert_eq!(p.name.as_deref(), Some("iris-compile"));
    }

    #[test]
    fn test_skill_params_search_action() {
        let p: SkillParams =
            serde_json::from_str(r#"{"action": "search", "query": "compile class"}"#).unwrap();
        assert_eq!(p.action, "search");
        assert_eq!(p.query.as_deref(), Some("compile class"));
    }

    #[test]
    fn test_skill_params_forget_action() {
        let p: SkillParams =
            serde_json::from_str(r#"{"action": "forget", "name": "old-skill"}"#).unwrap();
        assert_eq!(p.action, "forget");
        assert_eq!(p.name.as_deref(), Some("old-skill"));
    }

    #[test]
    fn test_skill_params_propose_action() {
        let p: SkillParams = serde_json::from_str(r#"{"action": "propose"}"#).unwrap();
        assert_eq!(p.action, "propose");
        assert!(p.name.is_none());
        assert!(p.query.is_none());
    }

    #[test]
    fn test_skill_params_all_fields() {
        let p: SkillParams =
            serde_json::from_str(r#"{"action": "search", "name": "my-skill", "query": "compile"}"#)
                .unwrap();
        assert_eq!(p.action, "search");
        assert_eq!(p.name.as_deref(), Some("my-skill"));
        assert_eq!(p.query.as_deref(), Some("compile"));
    }

    #[test]
    fn test_skill_params_null_name() {
        let p: SkillParams = serde_json::from_str(r#"{"action": "list", "name": null}"#).unwrap();
        assert!(p.name.is_none());
    }

    // ── SkillCommunityParams variants ──────────────────────────────────────────

    #[test]
    fn test_skill_community_params_list_action() {
        let p: SkillCommunityParams = serde_json::from_str(r#"{"action": "list"}"#).unwrap();
        assert_eq!(p.action, "list");
        assert!(p.package.is_none());
    }

    #[test]
    fn test_skill_community_params_install_action() {
        let p: SkillCommunityParams =
            serde_json::from_str(r#"{"action": "install", "package": "iris-rag"}"#).unwrap();
        assert_eq!(p.action, "install");
        assert_eq!(p.package.as_deref(), Some("iris-rag"));
    }

    #[test]
    fn test_skill_community_params_package_null() {
        let p: SkillCommunityParams =
            serde_json::from_str(r#"{"action": "list", "package": null}"#).unwrap();
        assert_eq!(p.action, "list");
        assert!(p.package.is_none());
    }

    // ── KbParams variants ────────────────────────────────────────────────────

    #[test]
    fn test_kb_params_index_action() {
        let p: KbParams = serde_json::from_str(r#"{"action": "index", "path": "/docs"}"#).unwrap();
        assert_eq!(p.action, "index");
        assert_eq!(p.path.as_deref(), Some("/docs"));
        assert!(p.query.is_none());
    }

    #[test]
    fn test_kb_params_recall_action() {
        let p: KbParams =
            serde_json::from_str(r#"{"action": "recall", "query": "hello"}"#).unwrap();
        assert_eq!(p.action, "recall");
        assert_eq!(p.query.as_deref(), Some("hello"));
        assert!(p.path.is_none());
    }

    #[test]
    fn test_kb_params_empty_query() {
        let p: KbParams = serde_json::from_str(r#"{"action": "recall", "query": ""}"#).unwrap();
        assert_eq!(p.query.as_deref(), Some(""));
    }

    #[test]
    fn test_kb_params_null_query() {
        let p: KbParams = serde_json::from_str(r#"{"action": "recall", "query": null}"#).unwrap();
        assert!(p.query.is_none());
    }

    // ── AgentInfoParams variants ──────────────────────────────────────────────

    #[test]
    fn test_agent_info_params_stats_what() {
        let p: AgentInfoParams = serde_json::from_str(r#"{"what": "stats"}"#).unwrap();
        assert_eq!(p.what, "stats");
    }

    #[test]
    fn test_agent_info_params_history_what() {
        let p: AgentInfoParams = serde_json::from_str(r#"{"what": "history"}"#).unwrap();
        assert_eq!(p.what, "history");
    }

    #[test]
    fn test_agent_info_params_both_what_values() {
        let stats = AgentInfoParams {
            what: "stats".to_string(),
            limit: 10,
        };
        let history = AgentInfoParams {
            what: "history".to_string(),
            limit: 50,
        };
        assert_ne!(stats.what, history.what);
        assert_eq!(stats.what, "stats");
        assert_eq!(history.what, "history");
    }

    // ── JSON deserialization failures ─────────────────────────────────────────

    #[test]
    fn test_skill_params_missing_action_fails() {
        let r: Result<SkillParams, _> = serde_json::from_str(r#"{"name": "foo"}"#);
        assert!(r.is_err());
    }

    #[test]
    fn test_skill_community_params_missing_action_fails() {
        let r: Result<SkillCommunityParams, _> = serde_json::from_str(r#"{"package": "foo"}"#);
        assert!(r.is_err());
    }

    #[test]
    fn test_kb_params_missing_action_fails() {
        let r: Result<KbParams, _> = serde_json::from_str(r#"{"query": "hello"}"#);
        assert!(r.is_err());
    }

    #[test]
    fn test_agent_info_params_missing_what_fails() {
        let r: Result<AgentInfoParams, _> = serde_json::from_str(r#"{"limit": 10}"#);
        assert!(r.is_err());
    }

    // ── Parameter combinations ───────────────────────────────────────────────

    #[test]
    fn test_kb_params_all_fields() {
        let p: KbParams = serde_json::from_str(
            r#"{"action": "index", "path": "/docs", "query": "test", "top_k": 15}"#,
        )
        .unwrap();
        assert_eq!(p.action, "index");
        assert_eq!(p.path.as_deref(), Some("/docs"));
        assert_eq!(p.query.as_deref(), Some("test"));
        assert_eq!(p.top_k, 15);
    }
}
