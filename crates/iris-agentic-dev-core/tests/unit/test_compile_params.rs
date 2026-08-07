//! T015: Unit tests for CompileParams deserialization.

#[cfg(test)]
mod tests {
    use iris_agentic_dev_core::tools::CompileParams;

    #[test]
    fn compile_params_basic() {
        let p: CompileParams = serde_json::from_str(r#"{"target":"MyApp.Patient.cls"}"#).unwrap();
        assert_eq!(p.target, "MyApp.Patient.cls");
        assert_eq!(p.flags, "cuk");
        // Omitted namespace stays None; resolution falls back to the connection namespace.
        assert_eq!(p.namespace, None);
        assert!(!p.force_writable);
    }

    #[test]
    fn compile_params_wildcard() {
        let p: CompileParams =
            serde_json::from_str(r#"{"target":"MyApp.*.cls","flags":"ck"}"#).unwrap();
        assert_eq!(p.target, "MyApp.*.cls");
        assert!(p.target.contains('*'), "wildcard target must contain *");
        assert_eq!(p.flags, "ck");
    }

    #[test]
    fn compile_params_full() {
        let p: CompileParams = serde_json::from_str(
            r#"{"target":"HS.FHIR.*.cls","flags":"cuk","namespace":"HSLIB","force_writable":true}"#,
        )
        .unwrap();
        assert_eq!(p.namespace.as_deref(), Some("HSLIB"));
        assert!(p.force_writable);
    }
}
