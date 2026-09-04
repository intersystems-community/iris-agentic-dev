/// Macro that collapses the repeated policy-gating preamble found in heavy tool handlers.
///
/// This macro encapsulates the pattern:
/// 1. Get server manager and policy
/// 2. Construct params_json
/// 3. Check dispatch_gate (Err path) and audit if blocked
/// 4. Check policy_gate (Some path) and audit if blocked
/// 5. Audit "allowed" if both checks pass
///
/// # Example
/// ```text
/// tool_gate!(self, "iris_compile", serde_json::json!({
///     "target": p.target,
///     "namespace": p.namespace
/// }))?;
/// // Handler body continues here
/// ```
///
/// # Returns
/// Returns early with `ok_json(gate)` if either gate rejects the tool.
/// Otherwise, execution continues and the allowed audit entry is written.
#[macro_export]
macro_rules! tool_gate {
    ($self_expr:expr, $tool_name:expr, $params_json:expr) => {{
        let (sm_server, policy) = $self_expr.active_server_manager_policy();
        let params_json = $params_json;

        // Check dispatch_gate (custom policy rules engine)
        if let Err(gate) = $crate::policy::gate::dispatch_gate(
            $tool_name,
            sm_server.as_deref().unwrap_or(""),
            policy.as_ref(),
            &params_json,
        ) {
            $self_expr.write_audit_entry(
                $tool_name,
                sm_server.as_deref().unwrap_or(""),
                policy.as_ref(),
                "blocked",
                Some("policy"),
                None,
                params_json,
            );
            return Ok(super::ok_json(gate));
        }

        // Check policy_gate (server manager policy)
        if let Some(gate) = $crate::iris::server_manager::policy_gate(
            $tool_name,
            sm_server.as_deref().unwrap_or(""),
            policy.as_ref(),
        ) {
            let allowed = gate["allowed_categories"].as_array().map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            });
            $self_expr.write_audit_entry(
                $tool_name,
                sm_server.as_deref().unwrap_or(""),
                policy.as_ref(),
                "blocked",
                Some("policy"),
                allowed,
                params_json,
            );
            return Ok(super::ok_json(gate));
        }

        // Both gates passed — audit the allowed access
        $self_expr.write_audit_entry(
            $tool_name,
            sm_server.as_deref().unwrap_or(""),
            policy.as_ref(),
            "allowed",
            None,
            None,
            params_json,
        );
    }};
}

#[cfg(test)]
mod tests {
    /// `tool_gate!` has no call site anywhere in the crate.
    ///
    /// Four tests used to live here — `test_tool_gate_macro_syntax`,
    /// `test_tool_gate_macro_compiles`, `test_tool_gate_early_return_dispatch_gate_err`,
    /// `test_tool_gate_early_return_policy_gate_some`. Every one had an empty body: a doc comment
    /// describing what the macro does, no code, no assertion. They reported `ok` on every run and
    /// showed up in the count as four tests covering the policy gate. They covered nothing, and
    /// four green lines beside a security gate is worse than no lines at all.
    ///
    /// A `macro_rules!` definition with no invocation is not even type-checked, so no test in this
    /// file can say anything about it. The honest statement is this one: the macro is unused, and
    /// the real gate is covered by `tests/unit/test_policy_gate.rs` (`dispatch_gate`,
    /// `policy_gate`) and `tests/integration/test_role_gate_e2e.rs` (end to end through the
    /// binary). When a handler is refactored to call `tool_gate!`, that handler's test is what
    /// covers the expansion.
    #[test]
    fn the_macro_is_unused_and_this_file_has_nothing_to_assert() {
        // Deliberately trivial, and it says so in the name. Kept as a marker so the next person to
        // add a call site finds the note above instead of re-adding empty tests.
    }
}
