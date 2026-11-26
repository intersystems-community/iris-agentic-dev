//! Bitmap indexes and bit-string masters (spec 173), pure.
//!
//! Measured on IRIS 2026.2 (`fact_7`, `fact_10`, research R1): a bitmap index global holds
//! `(value, chunk)` with a `$BIT` string as the node value, the chunk always the last subscript, and
//! the row ID of bit `p` in chunk `c` is `(c - 1) * 64000 + p - 1`. A hand-written `Type bitmap`
//! map with the field as its only subscript files exactly that, so a bitmap draft carries the field
//! alone (a second subscript only adds nodes).
//!
//! A master global of the shape `^G(const, chunk) = $BIT` is a data map whose third level is
//! computed (`AccessType Other`, an access variable, `NextCode`, an `Invalidcondition`), the form
//! spec 172's `IadSm172.Bits` fixture shows; [`chunk_map_draft`] writes it.

use serde_json::{json, Value};

/// Rows a chunk holds: bit positions run 1 to this.
pub const CHUNK_SIZE: u64 = 64000;

/// The row ID of bit `position` (1-based) in chunk `chunk` (1-based).
pub fn decode_id(chunk: u64, position: u64) -> u64 {
    (chunk - 1) * CHUNK_SIZE + position - 1
}

/// A `Type bitmap` index map named `name` over `global` with the one subscript `field_spelling`
/// (a field's draft expression, such as `{Status}` or `$$SQLUPPER({Name})`).
pub fn bitmap_map(name: &str, global: &str, field_spelling: &str) -> Value {
    json!({
        "name": name,
        "type": "bitmap",
        "global": global,
        "subscripts": [{"level": 1, "expression": field_spelling}],
    })
}

/// The access variable's code, the chunk walk and the validity test of the computed level, as
/// IRIS accepted them for `IadSm172.Bits`.
const ACCESS_VAR_CODE: &str = "s {2D2}=({L2}-1)*64000-1";
const INVALID_CONDITION: &str = "'$bit({3D},{L3}-{2D2})";
const NEXT_CODE: &str = " n pos s pos={L3}-{2D2}\n s:pos<0 pos=0\n s pos=$bitfind({3D},1,pos+1)\n i 'pos s {L3}=\"\" QUIT\n s {L3}={2D2}+pos\n q";

/// The data map of a bit-string master `^G(<constant>, chunk) = $BIT`: level 1 the constant, level 2
/// the chunk (`{K}\64000+1`, with the chunk's base ID in access variable `2D2`), level 3 the row ID
/// found by code. `constant` is an ObjectScript literal (a number or a quoted string) and `key` the
/// row-ID name the expressions use. Returns the map in model JSON and the paths it guessed.
pub fn chunk_map_draft(
    global: &str,
    constant: &str,
    key: &str,
) -> Result<(Value, Vec<String>), String> {
    if !is_literal(constant) {
        return Err(format!(
            "the constant first subscript must be a number or a quoted string, not {constant:?}"
        ));
    }
    if !is_name(key) {
        return Err(format!("{key:?} is not a usable row-ID name"));
    }
    let map = json!({
        "name": "Master",
        "type": "data",
        "global": global,
        "subscripts": [
            {"level": 1, "expression": constant},
            {
                "level": 2,
                "expression": format!("{{{key}}}\\64000+1"),
                "access_vars": [{"level": 2, "code": ACCESS_VAR_CODE, "variable": "2D2"}],
            },
            {
                "level": 3,
                "expression": format!("{{{key}}}"),
                "access_type": "Other",
                "next_code": NEXT_CODE,
                "invalid_conditions": [{"level": 1, "expression": INVALID_CONDITION}],
            },
        ],
        "data": {},
    });
    let guesses = [
        "/maps/Master/subscripts/1",
        "/maps/Master/subscripts/2",
        "/maps/Master/subscripts/3",
    ]
    .map(String::from)
    .to_vec();
    Ok((map, guesses))
}

/// A number, or a string literal whose quotes are all doubled.
fn is_literal(s: &str) -> bool {
    if let Some(inner) = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        return !inner.replace("\"\"", "").contains('"');
    }
    let digits = s.strip_prefix('-').unwrap_or(s);
    let (whole, frac) = digits.split_once('.').unwrap_or((digits, "0"));
    !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && !frac.is_empty()
        && frac.bytes().all(|b| b.is_ascii_digit())
}

/// A letter or `%` first, then letters and digits.
fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '%')
        && chars.all(|c| c.is_ascii_alphanumeric())
}
