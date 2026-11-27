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

use super::link::{
    assemble, check_draftable, check_with_validator, is_linked, master_field, ratio, spelling,
    Assembled, BitmapLine, IdForm, IdLine, LinkReport, MasterView, Verdict,
};
use super::shape::ShapeReport;
use serde::Deserialize;
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
    if !super::master::is_valid_property_name(key) {
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

// ── a bit-string master: the shape, the probe's answer, the constant ────────

/// Whether a sampled master could be `^G(const, chunk) = $BIT`: no value at depth 1, plain or
/// delimited strings at depth 2 (a dense bit string holds the delimiter bytes the sampler looks
/// for, so `delimited` does not rule it out), every depth-2 subscript numeric, no named sub-node,
/// nothing deeper and no `$LIST`. The probe then decides; this only keeps it from running on every
/// master.
pub fn looks_like_bit_master(report: &ShapeReport) -> bool {
    let at = |m: &std::collections::BTreeMap<String, u32>, d: &str| m.get(d).copied().unwrap_or(0);
    let valued_at_2 = at(&report.depths, "2");
    valued_at_2 > 0
        && at(&report.depths, "1") == 0
        && report.depths.keys().all(|d| d == "2")
        && at(&report.numeric_subscripts, "2") >= valued_at_2
        && report.sub_nodes.values().all(Vec::is_empty)
        && report
            .formats
            .iter()
            .all(|f| f.depth == 2 && matches!(f.format.as_str(), "plain" | "delimited"))
}

/// What the master probe read: the constant first subscript, how many chunk nodes and IDs it
/// decoded (at most 1,000 IDs), the sum of those IDs and the first and last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MasterBits {
    pub constant: String,
    pub chunks: u32,
    pub ids: u32,
    pub id_sum: u64,
    pub first: u64,
    pub last: u64,
}

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum ProbeLine {
    MasterBits {
        found: bool,
        #[serde(default)]
        constant: String,
        #[serde(default)]
        chunks: u32,
        #[serde(default)]
        ids: u32,
        #[serde(default)]
        idsum: u64,
        #[serde(default)]
        first: u64,
        #[serde(default)]
        last: u64,
    },
    Error {
        text: String,
    },
    Done,
}

/// The probe's answer: `Some` when the master is a constant over chunks of bit strings, `None`
/// when it is not. The `done` line is required, so an IRIS error or a cut-off answer is an error.
pub fn parse_master_bits(out: &str) -> Result<Option<MasterBits>, String> {
    let mut found = None;
    let mut done = false;
    for text in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if done {
            return Err(format!("the probe wrote more after its done line: {text}"));
        }
        let line: ProbeLine = serde_json::from_str(text)
            .map_err(|e| format!("the probe wrote something unexpected ({e}): {text}"))?;
        match line {
            ProbeLine::MasterBits {
                found: yes,
                constant,
                chunks,
                ids,
                idsum,
                first,
                last,
            } => {
                if yes {
                    if ids == 0 || chunks == 0 {
                        return Err(format!(
                            "the probe found bit strings but {ids} IDs in {chunks} chunks"
                        ));
                    }
                    found = Some(MasterBits {
                        constant,
                        chunks,
                        ids,
                        id_sum: idsum,
                        first,
                        last,
                    });
                }
            }
            ProbeLine::Error { text } => return Err(format!("IRIS error in the probe: {text}")),
            ProbeLine::Done => done = true,
        }
    }
    if !done {
        return Err("the probe stopped before it finished (no done line)".into());
    }
    Ok(found)
}

/// A name the way an index name or a constant subscript is one: a letter, then letters and digits.
fn is_name(s: &str) -> bool {
    s.len() <= 64
        && s.starts_with(|c: char| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The constant first subscript as an ObjectScript literal: a whole number as it is, a name quoted.
/// Anything else is refused, since the literal goes into the draft's map.
pub fn constant_literal(constant: &str) -> Result<String, String> {
    if !constant.is_empty() && constant.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(constant.to_string());
    }
    if is_name(constant) {
        return Ok(format!("\"{constant}\""));
    }
    Err(format!(
        "the constant first subscript {constant:?} is neither a whole number nor a name"
    ))
}

// ── a bitmap candidate: the verdict, the link, the draft ────────────────────

/// The verdict of a bitmap candidate and what the decode assumed. IDs that do not find master rows
/// are `not_linked`; a position verdict weaker than `ambiguous` stands; IDs from chunk 2 on that do
/// not find rows, when chunk 1's do, leave the chunk size unconfirmed: `ambiguous`, with `assumed`
/// naming it.
pub fn bitmap_verdict(line: &BitmapLine, positions: Verdict) -> (Verdict, Vec<String>) {
    if line.ids_checked == 0 || !is_linked(ratio(line.ids_found, line.ids_checked)) {
        return (Verdict::NotLinked, Vec::new());
    }
    if !positions.is_explained() {
        return (positions, Vec::new());
    }
    if line.later_checked > 0 && !is_linked(ratio(line.later_found, line.later_checked)) {
        return (
            Verdict::Ambiguous,
            vec![format!(
                "chunk size 64000: {} of the {} IDs decoded from chunk 2 on found a master row, so \
                 the size is assumed from chunk 1 and not confirmed",
                line.later_found, line.later_checked
            )],
        );
    }
    (positions, Vec::new())
}

/// The link of a bitmap candidate from a linker report that has a `bitmap` line, or `None` without
/// one. The decoded IDs are the row IDs (`IdForm::Value`: they are not a subscript), and the index
/// name a class-compiled bitmap holds first is a constant that explains no field, so its position
/// is left out.
pub fn assemble_bitmap(global: &str, map: &str, report: &LinkReport) -> Option<Assembled> {
    let b = report.bitmaps.first()?;
    let mut r = report.clone();
    r.ids = vec![IdLine {
        form: IdForm::Value,
        positions: Vec::new(),
        resolved: b.ids_found,
        total: b.ids_checked,
    }];
    if b.lead.is_some() {
        r.positions.retain(|p| p.position != 1);
    }
    Some(assemble(global, map, &r))
}

/// The `Type bitmap` map of a bitmap link: the index name first when the global has one, then each
/// explained position's expression, and no ID (the chunk is appended by the engine). The paths of
/// the elements it guessed are returned with it. The map passes the model validator or this is an
/// error.
pub fn draft_bitmap_map(
    a: &Assembled,
    view: &MasterView,
    lead: Option<&str>,
) -> Result<(Value, Vec<String>), String> {
    let link = &a.link;
    check_draftable(link)?;
    let mut subs = Vec::new();
    if let Some(name) = lead {
        if !is_name(name) {
            return Err(format!("{name:?} is not an index name"));
        }
        subs.push(json!({"level": 1, "expression": format!("\"{name}\"")}));
    }
    let first = u32::from(lead.is_some()) + 1;
    let mut held: Vec<_> = link.subscripts.iter().collect();
    held.sort_by_key(|s| s.position);
    if held.is_empty() {
        return Err("the bitmap has no subscript to draft".into());
    }
    let mut guesses = Vec::new();
    for (i, sub) in held.iter().enumerate() {
        let level = first + u32::try_from(i).map_err(|_| "too many subscripts")?;
        if sub.position != level {
            return Err(format!(
                "position {} where level {level} was expected: a gap in the subscripts",
                sub.position
            ));
        }
        let pick = sub
            .pick
            .as_ref()
            .filter(|_| sub.verdict.is_explained())
            .ok_or_else(|| {
                format!(
                    "position {} is {}: no expression to draft",
                    sub.position,
                    sub.verdict.as_str()
                )
            })?;
        if sub.verdict == Verdict::Ambiguous {
            guesses.push(format!("/maps/{}/subscripts/{level}", link.map_name));
        }
        let expression = spelling(
            pick.transform,
            master_field(view, pick.field)?,
            pick.length,
            pick.piece,
        )?;
        subs.push(json!({"level": level, "expression": expression}));
    }
    let map = json!({
        "name": link.map_name,
        "type": "bitmap",
        "global": link.global,
        "subscripts": subs,
    });
    check_with_validator(&map)?;
    Ok((map, guesses))
}
