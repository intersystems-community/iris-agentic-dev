//! Global reference normalization for storage maps (spec 172 US2).

use serde::Serialize;

/// A normalized global reference.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GlobalRef {
    /// The global name with leading caret (e.g., "^X").
    pub name: String,
    /// Extended reference namespace or directory (e.g., "NS", "^^/db").
    pub extended: Option<String>,
    /// Subscript prefix if present (e.g., "(1,2)").
    pub subscripts_prefix: Option<String>,
}

/// Normalizes a global reference into its canonical form.
///
/// Accepts:
/// - `^X` or `X` → `{name: "^X", extended: None, subscripts_prefix: None}`
/// - `^X(...)` → with subscripts_prefix
/// - `^|"NS"|X` or `^["NS"]X` → with extended
/// - `^|"^^/db"|X` → extended reference to a directory
///
/// Rejects (returns `Err("dynamic")`):
/// - Anything with `@` (indirection)
/// - Anything with `$Name` (dynamic name construction)
pub fn normalize_global(raw: &str) -> Result<GlobalRef, String> {
    // Indirection anywhere, or `$NAME`/`$NA` in any case, makes the global unknowable here.
    if raw.contains('@') || raw.to_ascii_uppercase().contains("$NA") {
        return Err("dynamic".to_string());
    }

    let trimmed = raw.trim();
    let mut working = trimmed.strip_prefix('^').unwrap_or(trimmed);

    // Extended reference: `|"NS"|X` closes on `"|`, `["NS"]X` closes on `"]`.
    let closer = if working.starts_with('|') {
        Some("\"|")
    } else if working.starts_with('[') {
        Some("\"]")
    } else {
        None
    };
    let mut extended = None;
    if let Some(close) = closer {
        // The opening `|"` / `["` is two ASCII bytes, so slicing at 2 is on a char boundary.
        let rest = match working.get(1..2) {
            Some("\"") => &working[2..],
            _ => return Err(format!("unreadable extended reference: {trimmed}")),
        };
        let Some(close_pos) = rest.find(close) else {
            return Err(format!("unreadable extended reference: {trimmed}"));
        };
        extended = Some(rest[..close_pos].to_string());
        working = &rest[close_pos + close.len()..];
    }

    let (base, subscripts_prefix) = match working.find('(') {
        Some(paren_pos) => (
            &working[..paren_pos],
            Some(working[paren_pos..].to_string()),
        ),
        None => (working, None),
    };

    Ok(GlobalRef {
        name: format!("^{base}"),
        extended,
        subscripts_prefix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_forms() {
        let r = normalize_global("^X").unwrap();
        assert_eq!(r.name, "^X");
        assert!(r.extended.is_none());
        assert!(r.subscripts_prefix.is_none());

        let r = normalize_global("X").unwrap();
        assert_eq!(r.name, "^X");
    }

    #[test]
    fn with_subscripts() {
        let r = normalize_global("^X(1,2)").unwrap();
        assert_eq!(r.name, "^X");
        assert_eq!(r.subscripts_prefix, Some("(1,2)".to_string()));
    }

    #[test]
    fn extended_refs() {
        let r = normalize_global("^|\"NS\"|X").unwrap();
        assert_eq!(r.name, "^X");
        assert_eq!(r.extended, Some("NS".to_string()));

        let r = normalize_global("^[\"NS\"]X").unwrap();
        assert_eq!(r.name, "^X");
        assert_eq!(r.extended, Some("NS".to_string()));
    }

    #[test]
    fn dynamic_rejected() {
        assert!(normalize_global("^@X").is_err());
        assert!(normalize_global("^$Name(x)").is_err());
        assert!(normalize_global("$NAME(^X)").is_err());
        assert!(normalize_global("$na(^X)").is_err());
    }

    #[test]
    fn malformed_extended_is_an_error_not_a_panic() {
        for raw in ["^|\"|X", "^|", "^[", "^|é|X", "^[\"NS", "^|\"NS"] {
            assert!(normalize_global(raw).is_err(), "{raw}");
        }
        let r = normalize_global("^|\"^^/db/\"|X(1)").unwrap();
        assert_eq!(r.extended.as_deref(), Some("^^/db/"));
        assert_eq!(r.name, "^X");
    }
}
