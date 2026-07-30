//! Session-state helpers for `iris_execute`.
//!
//! Implements the `%ctx`-carrier pattern: a `%DynamicObject` injected into every
//! session-enabled execution. The caller holds the serialized state (a Base64-encoded
//! JSON string produced by IRIS) between calls; nothing is written to IRIS.
//!
//! Rust never needs to encode/decode Base64 — IRIS does that. Rust only validates
//! that the token is non-empty and safe to embed in generated ObjectScript.

use anyhow::{anyhow, Result};

/// Opaque session state token held by the MCP client between `iris_execute` calls.
///
/// The token is the raw Base64 string emitted by IRIS after `__SESSION_STATE__:`.
/// Rust treats it as opaque and embeds it verbatim in the generated preamble.
#[derive(Debug, Clone)]
pub struct SessionToken(String);

impl SessionToken {
    /// Wrap a token string, validating it is safe to embed in ObjectScript.
    ///
    /// The token must be non-empty and contain only Base64-safe characters
    /// (A-Za-z0-9+/=) plus optional whitespace (IRIS may wrap long Base64).
    pub fn new(token: &str) -> Result<Self> {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("session_state token is empty"));
        }
        // Base64 alphabet + newlines (IRIS wraps at 76 chars)
        if trimmed
            .chars()
            .any(|c| !matches!(c, 'A'..='Z' | 'a'..='z' | '0'..='9' | '+' | '/' | '=' | '\n' | '\r' | ' '))
        {
            return Err(anyhow!(
                "session_state token contains unexpected characters (expected Base64)"
            ));
        }
        // Strip all whitespace for safe embedding in ObjectScript string literal
        let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
        Ok(SessionToken(compact))
    }

    /// The compact (no-whitespace) Base64 string, safe to embed in an ObjectScript string literal.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Generate the ObjectScript preamble to inject before user code.
///
/// If `token` is `None`, injects `Set %ctx = {}` (fresh session).
/// If `token` is `Some`, injects the full restore block (Base64Decode → %FromJSON →
/// two-pass OID re-open).
///
/// Returns an error if the token fails validation.
pub fn build_session_preamble(token: Option<&str>) -> Result<String> {
    match token {
        None => Ok("Set %ctx = {}\n".to_string()),
        Some(t) => {
            let tok = SessionToken::new(t)?;
            Ok(format!(
                "Set zToken = \"{token}\"\n\
                 Try {{\n\
                     Set %ctx = ##class(%DynamicObject).%FromJSON($system.Encryption.Base64Decode(zToken))\n\
                 }} Catch zEx {{ Write \"__SESSION_INVALID__:\", zEx.DisplayString(), ! Quit }}\n\
                 Kill zToken\n\
                 Set zToRestore = []\n\
                 Set zIter = %ctx.%GetIterator()\n\
                 While zIter.%GetNext(.zK, .zV) {{\n\
                     If $isobject(zV) && zV.%IsDefined(\"_cls\") {{ Do zToRestore.%Push(zK) }}\n\
                 }}\n\
                 Kill zIter, zK, zV\n\
                 Set zI = 0\n\
                 While zI < zToRestore.%Size() {{\n\
                     Set zK = zToRestore.%Get(zI)\n\
                     Set zStub = %ctx.%Get(zK)\n\
                     Set zCls = zStub.\"_cls\"  Set zId = zStub.\"_id\"\n\
                     Try {{ Set zObj = $classmethod(zCls, \"%OpenId\", zId) }}\n\
                     Catch zEx {{ Write \"__SESSION_RESTORE_FAILED__:\", zK, \":\", zCls, ! Quit }}\n\
                     If '$isobject(zObj) {{ Write \"__SESSION_RESTORE_FAILED__:\", zK, \":\", zCls, ! Quit }}\n\
                     Do %ctx.%Set(zK, zObj)\n\
                     Set zI = zI + 1\n\
                 }}\n\
                 Kill zToRestore, zI, zK, zStub, zCls, zId, zObj\n",
                token = tok.as_str()
            ))
        }
    }
}

/// Generate the ObjectScript epilogue to inject after user code.
///
/// Scans `%ctx` for live `%Persistent` objects, re-stubs them, then serializes
/// the whole `%ctx` to a Base64 token emitted on a sentinel line.
pub fn build_session_epilogue() -> String {
    "Set zToStub = []\n\
     Set zIter = %ctx.%GetIterator()\n\
     While zIter.%GetNext(.zK, .zV) {\n\
         If $isobject(zV) && zV.%IsA(\"%Library.Persistent\") { Do zToStub.%Push(zK) }\n\
     }\n\
     Kill zIter, zK, zV\n\
     Set zI = 0\n\
     While zI < zToStub.%Size() {\n\
         Set zK = zToStub.%Get(zI)\n\
         Set zV = %ctx.%Get(zK)\n\
         Do %ctx.%Set(zK, {\"_cls\": ($classname(zV)), \"_id\": (zV.%Id())})\n\
         Set zI = zI + 1\n\
     }\n\
     Kill zToStub, zI, zK, zV\n\
     Try {\n\
         Write \"__SESSION_STATE__:\", $system.Encryption.Base64Encode(%ctx.%ToJSON()), !\n\
     } Catch zEx { Write \"__SESSION_SERIALIZE_FAILED__:\", zEx.DisplayString(), ! }\n"
        .to_string()
}

/// Sentinel prefixes written by the generated ObjectScript.
pub const SENTINEL_STATE: &str = "__SESSION_STATE__:";
pub const SENTINEL_INVALID: &str = "__SESSION_INVALID__:";
pub const SENTINEL_RESTORE_FAILED: &str = "__SESSION_RESTORE_FAILED__:";
pub const SENTINEL_SERIALIZE_FAILED: &str = "__SESSION_SERIALIZE_FAILED__:";

/// Parse sentinel lines out of `execute_via_generator` output.
///
/// Returns `(visible_output, session_state_token, error)`.
/// `visible_output` has all sentinel lines removed.
/// `error` is `Some((error_code, detail))` if a fatal sentinel was found.
pub fn parse_session_output(raw: &str) -> (String, Option<String>, Option<(String, String)>) {
    let mut visible: Vec<&str> = Vec::new();
    let mut token: Option<String> = None;
    let mut error: Option<(String, String)> = None;

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix(SENTINEL_STATE) {
            token = Some(rest.to_string());
        } else if let Some(detail) = line.strip_prefix(SENTINEL_INVALID) {
            error = Some(("SESSION_INVALID".to_string(), detail.to_string()));
        } else if let Some(detail) = line.strip_prefix(SENTINEL_RESTORE_FAILED) {
            error = Some(("SESSION_RESTORE_FAILED".to_string(), detail.to_string()));
        } else if let Some(detail) = line.strip_prefix(SENTINEL_SERIALIZE_FAILED) {
            error = Some(("SESSION_SERIALIZE_FAILED".to_string(), detail.to_string()));
        } else {
            visible.push(line);
        }
    }

    let mut out = visible.join("\n");
    if raw.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }

    (out, token, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SessionToken ──────────────────────────────────────────────────────────

    #[test]
    fn token_accepts_valid_base64() {
        // eyJ4IjoxfQ== is {"x":1}
        let t = SessionToken::new("eyJ4IjoxfQ==").unwrap();
        assert_eq!(t.as_str(), "eyJ4IjoxfQ==");
    }

    #[test]
    fn token_strips_whitespace() {
        let t = SessionToken::new("eyJ4\nIjox\nfQ==").unwrap();
        assert_eq!(t.as_str(), "eyJ4IjoxfQ==");
    }

    #[test]
    fn token_rejects_empty() {
        assert!(SessionToken::new("").is_err());
        assert!(SessionToken::new("   ").is_err());
    }

    #[test]
    fn token_rejects_non_base64() {
        assert!(SessionToken::new("not!!base64").is_err());
        assert!(SessionToken::new("{\"x\":1}").is_err());
    }

    // ── build_session_preamble ────────────────────────────────────────────────

    #[test]
    fn preamble_fresh_session() {
        let p = build_session_preamble(None).unwrap();
        assert!(p.contains("Set %ctx = {}"));
        assert!(!p.contains("Base64Decode"));
    }

    #[test]
    fn preamble_with_token_contains_restore_block() {
        let token = "eyJ4IjoxfQ==";
        let p = build_session_preamble(Some(token)).unwrap();
        assert!(p.contains("Base64Decode"));
        assert!(p.contains("%FromJSON"));
        assert!(p.contains("__SESSION_INVALID__:"));
        assert!(p.contains("__SESSION_RESTORE_FAILED__:"));
        assert!(p.contains("zToRestore"));
        assert!(p.contains(token));
    }

    #[test]
    fn preamble_no_unabbreviated_functions() {
        let token = "eyJ4IjoxfQ==";
        let p = build_session_preamble(Some(token)).unwrap();
        assert!(!p.contains("$LENGTH"));
        assert!(!p.contains("$PIECE"));
        assert!(!p.contains("$DATA("));
    }

    #[test]
    fn preamble_invalid_token_returns_error() {
        assert!(build_session_preamble(Some("not!!base64")).is_err());
        assert!(build_session_preamble(Some("")).is_err());
    }

    // ── build_session_epilogue ────────────────────────────────────────────────

    #[test]
    fn epilogue_contains_required_sentinels() {
        let e = build_session_epilogue();
        assert!(e.contains("__SESSION_STATE__:"));
        assert!(e.contains("__SESSION_SERIALIZE_FAILED__:"));
        assert!(e.contains("Base64Encode"));
        assert!(e.contains("%IsA(\"%Library.Persistent\")"));
    }

    #[test]
    fn epilogue_no_unabbreviated_functions() {
        let e = build_session_epilogue();
        assert!(!e.contains("$LENGTH"));
        assert!(!e.contains("$PIECE"));
    }

    // ── parse_session_output ──────────────────────────────────────────────────

    #[test]
    fn parse_extracts_state_token() {
        let raw = "hello\n__SESSION_STATE__:abc123\nworld\n";
        let (vis, tok, err) = parse_session_output(raw);
        assert_eq!(vis, "hello\nworld\n");
        assert_eq!(tok, Some("abc123".to_string()));
        assert!(err.is_none());
    }

    #[test]
    fn parse_session_invalid() {
        let raw = "__SESSION_INVALID__:bad json error\n";
        let (_, tok, err) = parse_session_output(raw);
        assert!(tok.is_none());
        let (code, detail) = err.unwrap();
        assert_eq!(code, "SESSION_INVALID");
        assert!(detail.contains("bad json error"));
    }

    #[test]
    fn parse_restore_failed() {
        let raw = "__SESSION_RESTORE_FAILED__:hdr:NoSuch.Class\n";
        let (_, _, err) = parse_session_output(raw);
        let (code, detail) = err.unwrap();
        assert_eq!(code, "SESSION_RESTORE_FAILED");
        assert!(detail.contains("NoSuch.Class"));
    }

    #[test]
    fn parse_serialize_failed() {
        let raw = "__SESSION_SERIALIZE_FAILED__:some error\n";
        let (_, _, err) = parse_session_output(raw);
        let (code, _) = err.unwrap();
        assert_eq!(code, "SESSION_SERIALIZE_FAILED");
    }

    #[test]
    fn parse_no_sentinel_unchanged() {
        let raw = "line1\nline2\n";
        let (vis, tok, err) = parse_session_output(raw);
        assert_eq!(vis, "line1\nline2\n");
        assert!(tok.is_none());
        assert!(err.is_none());
    }

    #[test]
    fn parse_empty_output() {
        let (vis, tok, err) = parse_session_output("");
        assert!(vis.is_empty());
        assert!(tok.is_none());
        assert!(err.is_none());
    }
}
