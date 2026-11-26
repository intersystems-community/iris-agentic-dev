//! Linking an index global to its master (spec 173), the half that needs no IRIS: the closed set
//! of transforms and how a draft spells each, the verdict thresholds, the reader for the lines the
//! linker writes (the contract is in `link_code.rs`), and the index map drafted from a link.
//!
//! Nothing here sees a sampled value. The linker's output carries counts, positions, field numbers
//! and transform names, and a draft is built from those and the master's own names.

use super::model::map_node;
use super::tree::Node;
use super::validate::validate;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// A tie is two scores within this much of the best (FR-011); floats get a hair of slack so a gap
/// of exactly 0.01 counts.
const TIE: f64 = 0.01;
const EPS: f64 = 1e-9;
/// Agreement at which a position is explained (FR-011) and the ID floor below which an index is
/// not linked to the master at all.
const EXPLAINED: f64 = 0.95;
const ID_FLOOR: f64 = 0.9;
/// Usable nodes needed before a link may be called inferred (FR-011).
const MIN_NODES: u32 = 20;

/// The share of index nodes without a master row above which it is reported.
const ORPHAN_SHARE: f64 = 0.05;

// ── transforms and their spelling ──────────────────────────────────────────

/// The closed set of FR-007. `COLLATE` is recognised by the planner but never drafted, and
/// `EXACT`, `PLUS`, `MINUS`, `SPACE` are not map-expression functions (research R4, fact 1), so
/// none of them is a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Transform {
    #[serde(rename = "exact")]
    Exact,
    #[serde(rename = "sqlupper")]
    SqlUpper,
    #[serde(rename = "sqlstring")]
    SqlString,
    #[serde(rename = "string")]
    Str,
    #[serde(rename = "alphaup")]
    AlphaUp,
    #[serde(rename = "upper")]
    Upper,
    #[serde(rename = "mvr")]
    Mvr,
    #[serde(rename = "truncate")]
    Truncate,
}

impl Transform {
    /// The name the linker writes and the answer carries.
    pub fn name(self) -> &'static str {
        match self {
            Transform::Exact => "exact",
            Transform::SqlUpper => "sqlupper",
            Transform::SqlString => "sqlstring",
            Transform::Str => "string",
            Transform::AlphaUp => "alphaup",
            Transform::Upper => "upper",
            Transform::Mvr => "mvr",
            Transform::Truncate => "truncate",
        }
    }

    /// The transform called `name`, or an error naming the set: anything outside it is refused.
    pub fn parse(name: &str) -> Result<Transform, String> {
        const ALL: [Transform; 8] = [
            Transform::Exact,
            Transform::SqlUpper,
            Transform::SqlString,
            Transform::Str,
            Transform::AlphaUp,
            Transform::Upper,
            Transform::Mvr,
            Transform::Truncate,
        ];
        ALL.into_iter().find(|t| t.name() == name).ok_or_else(|| {
            format!(
                "{name:?} is not a transform of the closed set (exact, sqlupper, sqlstring, \
                 string, alphaup, upper, mvr, truncate)"
            )
        })
    }

    /// The map-expression function, or `None` for the value as written.
    fn function(self) -> Option<&'static str> {
        match self {
            Transform::Exact => None,
            Transform::SqlUpper => Some("SQLUPPER"),
            Transform::SqlString => Some("SQLSTRING"),
            Transform::Str => Some("STRING"),
            Transform::AlphaUp => Some("ALPHAUP"),
            Transform::Upper => Some("UPPER"),
            Transform::Mvr => Some("MVR"),
            Transform::Truncate => Some("TRUNCATE"),
        }
    }
}

/// Whether `transform` may carry `length` and `piece`: the rows of data-model.md's table and no
/// others. A length belongs to `sqlupper`, `sqlstring`, `string` (optional) and `truncate`
/// (required); a piece to every transform but `exact`; both only on `truncate`.
fn check_variant(
    transform: Transform,
    length: Option<u32>,
    piece: Option<u8>,
) -> Result<(), String> {
    let name = transform.name();
    if length == Some(0) {
        return Err(format!("{name}: a length of 0 is not a length"));
    }
    if let Some(p) = piece {
        if p != 1 && p != 2 {
            return Err(format!("{name}: piece {p} is not 1 or 2"));
        }
    }
    match transform {
        Transform::Exact if length.is_some() || piece.is_some() => {
            Err("exact takes neither a length nor a piece".into())
        }
        Transform::AlphaUp | Transform::Upper | Transform::Mvr if length.is_some() => {
            Err(format!("{name} has no length variant"))
        }
        Transform::Truncate if length.is_none() => Err("truncate needs a length".into()),
        Transform::SqlUpper | Transform::SqlString | Transform::Str
            if length.is_some() && piece.is_some() =>
        {
            Err(format!(
                "{name} has a length variant and a piece variant, not both"
            ))
        }
        _ => Ok(()),
    }
}

/// A property name safe to put inside `{...}`: a letter or `%`, then letters and digits.
fn is_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c == '%' || c.is_alphabetic())
        && chars.all(char::is_alphanumeric)
        && name.chars().count() <= 128
}

/// The draft's expression for `field_name` under `transform` (the table of data-model.md):
/// `{F}`, `$$SQLUPPER({F})`, `$$SQLUPPER({F},4)`, `$p($$ALPHAUP({F}),",",2)`. Only these spellings
/// are ever returned; a combination the table lacks, or a field name that could break out of the
/// braces, is an error.
pub fn spelling(
    transform: Transform,
    field_name: &str,
    length: Option<u32>,
    piece: Option<u8>,
) -> Result<String, String> {
    if !is_field_name(field_name) {
        return Err(format!("{field_name:?} is not a field name"));
    }
    check_variant(transform, length, piece)?;
    let field = format!("{{{field_name}}}");
    let Some(function) = transform.function() else {
        return Ok(field);
    };
    let call = match length {
        Some(n) => format!("$${function}({field},{n})"),
        None => format!("$${function}({field})"),
    };
    Ok(match piece {
        Some(p) => format!("$p({call},\",\",{p})"),
        None => call,
    })
}

// ── verdicts ───────────────────────────────────────────────────────────────

/// The one vocabulary of `verdict` and `subscripts[].verdict` (data-model.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Inferred,
    Ambiguous,
    TooFewNodes,
    NotLinked,
    NotExplained,
    Constant,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Inferred => "inferred",
            Verdict::Ambiguous => "ambiguous",
            Verdict::TooFewNodes => "too_few_nodes",
            Verdict::NotLinked => "not_linked",
            Verdict::NotExplained => "not_explained",
            Verdict::Constant => "constant",
        }
    }

    /// Whether a pair explains the position (or the link): `inferred` or `ambiguous`.
    pub fn is_explained(self) -> bool {
        matches!(self, Verdict::Inferred | Verdict::Ambiguous)
    }
}

/// `part` over `whole`, 0 when `whole` is 0.
fn ratio(part: u32, whole: u32) -> f64 {
    if whole == 0 {
        0.0
    } else {
        f64::from(part) / f64::from(whole)
    }
}

/// Whether an ID agreement is high enough to link the index to the master at all.
fn is_linked(id_agreement: f64) -> bool {
    id_agreement.is_finite() && id_agreement + EPS >= ID_FLOOR
}

/// How well one (field, transform, length, piece) explains a position: `matches` of `total`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PositionScore {
    pub field: u32,
    pub transform: Transform,
    pub length: Option<u32>,
    pub piece: Option<u8>,
    pub matches: u32,
    pub total: u32,
}

impl PositionScore {
    pub fn agreement(&self) -> f64 {
        ratio(self.matches, self.total)
    }
}

/// What `verdict` judges: the usable nodes (row ID resolved to a master row), how well the row ID
/// was found (`id_agreement`, 0 to 1), how many distinct values the position held, and the
/// position's scores (the linker's best few).
#[derive(Debug, Clone, Copy)]
pub struct VerdictInput<'a> {
    pub usable_nodes: u32,
    pub id_agreement: f64,
    pub distinct: u32,
    pub scores: &'a [PositionScore],
}

/// The verdict and what it rests on. `agreement` is the ID agreement for `not_linked` and the
/// best score's otherwise (0 with no score); `best` is the winning pair; `alternatives` lists every
/// tied pair, best first, for `ambiguous` and is empty otherwise.
#[derive(Debug, Clone, PartialEq)]
pub struct Judgement {
    pub verdict: Verdict,
    pub agreement: f64,
    pub best: Option<PositionScore>,
    pub alternatives: Vec<PositionScore>,
}

/// FR-011, in the order that keeps each verdict honest: an index whose IDs do not find master rows
/// is `not_linked` whatever else holds (an unrelated global also has few usable nodes); then
/// fewer than 20 usable nodes is `too_few_nodes`; a position of one value is `constant` (every
/// field of a constant column ties, so this comes before the tie rule); no pair at 0.95 is
/// `not_explained`; two or more pairs within 0.01 of the best are `ambiguous`; the rest `inferred`.
pub fn verdict(input: &VerdictInput<'_>) -> Judgement {
    let mut ranked: Vec<&PositionScore> = input.scores.iter().collect();
    // Stable: equal agreements keep the linker's order, best first.
    ranked.sort_by(|a, b| b.agreement().total_cmp(&a.agreement()));
    let best = ranked.first().map(|s| (*s).clone());
    let best_agreement = best.as_ref().map_or(0.0, PositionScore::agreement);
    let judged = |verdict, agreement, alternatives: Vec<PositionScore>| Judgement {
        verdict,
        agreement,
        best: best.clone(),
        alternatives,
    };
    if !is_linked(input.id_agreement) {
        return judged(Verdict::NotLinked, input.id_agreement, vec![]);
    }
    if input.usable_nodes < MIN_NODES {
        return judged(Verdict::TooFewNodes, best_agreement, vec![]);
    }
    if input.distinct <= 1 {
        return judged(Verdict::Constant, best_agreement, vec![]);
    }
    if best.is_none() || best_agreement + EPS < EXPLAINED {
        return judged(Verdict::NotExplained, best_agreement, vec![]);
    }
    let tied: Vec<PositionScore> = ranked
        .iter()
        .take_while(|s| best_agreement - s.agreement() <= TIE + EPS)
        .map(|s| (*s).clone())
        .collect();
    if tied.len() >= 2 {
        return judged(Verdict::Ambiguous, best_agreement, tied);
    }
    judged(Verdict::Inferred, best_agreement, vec![])
}

// ── map names ──────────────────────────────────────────────────────────────

/// The map name for `global` (FR-015): the last name segment (after the last `.`), letters and
/// digits only, first letter in upper case (`^App.x` is `X`), made unique against `taken` by a
/// numeric suffix (`X2`, `X3`), comparing without regard to case.
pub fn map_name(global: &str, taken: &[String]) -> String {
    let bare = global.strip_prefix('^').unwrap_or(global);
    let segment = bare.rsplit('.').next().unwrap_or(bare);
    let mut base: String = segment
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) {
        base.insert_str(0, "Index");
    }
    if let Some(first) = base.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    let is_taken = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if !is_taken(&base) {
        return base;
    }
    (2u32..)
        .map(|n| format!("{base}{n}"))
        .find(|n| !is_taken(n))
        .unwrap_or(base)
}

// ── the linker's output ────────────────────────────────────────────────────

/// Why the linker's walk stopped (the `sample` line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stopped {
    None,
    Nodes,
    Time,
    CountCap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdForm {
    Subscript,
    Value,
}

impl IdForm {
    pub fn as_str(self) -> &'static str {
        match self {
            IdForm::Subscript => "subscript",
            IdForm::Value => "value",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NullKind {
    C0,
    Minus1e14,
    Space,
    SpaceC0,
    Discovered,
}

impl NullKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NullKind::C0 => "c0",
            NullKind::Minus1e14 => "minus1e14",
            NullKind::Space => "space",
            NullKind::SpaceC0 => "space_c0",
            NullKind::Discovered => "discovered",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmptyKind {
    EmptyString,
    Null,
}

impl EmptyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EmptyKind::EmptyString => "empty_string",
            EmptyKind::Null => "null",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingRule {
    Null,
    EmptyString,
    Fixed,
    None,
}

impl MissingRule {
    pub fn as_str(self) -> &'static str {
        match self {
            MissingRule::Null => "null",
            MissingRule::EmptyString => "empty_string",
            MissingRule::Fixed => "fixed",
            MissingRule::None => "none",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SampleLine {
    pub nodes: u32,
    pub distinct_first: u32,
    pub usable: u32,
    pub stopped: Stopped,
}

/// One way the row ID was tried: `positions` is the run of subscripts (empty for `value`),
/// `resolved` of `total` nodes found a master row.
#[derive(Debug, Clone, PartialEq)]
pub struct IdLine {
    pub form: IdForm,
    pub positions: Vec<u32>,
    pub resolved: u32,
    pub total: u32,
}

/// A non-ID position with its best scores (each carries the line's `total`).
#[derive(Debug, Clone, PartialEq)]
pub struct PositionLine {
    pub position: u32,
    pub total: u32,
    pub distinct: u32,
    pub scores: Vec<PositionScore>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NullLine {
    pub position: u32,
    pub field: u32,
    pub kind: NullKind,
    pub empty: EmptyKind,
    pub count: u32,
    /// A discovered marker's length.
    pub length: Option<u32>,
    /// A discovered marker's literal; the linker writes it only under `include_values`.
    pub literal: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataLine {
    pub field: u32,
    pub element: u32,
    pub matches: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MissingLine {
    pub share: f64,
    pub rule: MissingRule,
    pub field: Option<u32>,
    /// The fixed value of a `fixed` rule; the linker writes it only under `include_values`.
    pub literal: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BitmapLine {
    pub chunk_position: u32,
    pub ids_checked: u32,
    pub ids_found: u32,
    /// Of the IDs decoded from chunk 2 on, how many and how many found a master row: the chunk size
    /// is confirmed only by these.
    pub later_checked: u32,
    pub later_found: u32,
    /// The constant first subscript of a class-compiled bitmap: the index name.
    pub lead: Option<String>,
}

/// Everything one linker run wrote, after its `done` line.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkReport {
    pub sample: SampleLine,
    pub ids: Vec<IdLine>,
    pub positions: Vec<PositionLine>,
    pub nulls: Vec<NullLine>,
    pub data: Vec<DataLine>,
    pub missing: Option<MissingLine>,
    pub bitmaps: Vec<BitmapLine>,
}

#[derive(Deserialize)]
struct RawScore {
    field: u32,
    transform: Transform,
    length: Option<u32>,
    piece: Option<u8>,
    matches: u32,
}

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum Line {
    Sample {
        nodes: u32,
        distinct_first: u32,
        usable: u32,
        stopped: Stopped,
    },
    Id {
        form: IdForm,
        positions: Vec<u32>,
        resolved: u32,
        total: u32,
    },
    Pos {
        position: u32,
        total: u32,
        distinct: u32,
        scores: Vec<RawScore>,
    },
    Null {
        position: u32,
        field: u32,
        kind: NullKind,
        empty: EmptyKind,
        count: u32,
        #[serde(default)]
        length: Option<u32>,
        #[serde(default)]
        literal: Option<String>,
    },
    Data {
        field: u32,
        element: u32,
        matches: u32,
        total: u32,
    },
    Missing {
        share: f64,
        rule: MissingRule,
        #[serde(default)]
        field: Option<u32>,
        #[serde(default)]
        literal: Option<String>,
    },
    Bitmap {
        chunk_position: u32,
        ids_checked: u32,
        ids_found: u32,
        #[serde(default)]
        later_checked: u32,
        #[serde(default)]
        later_found: u32,
        #[serde(default)]
        lead: Option<String>,
    },
    Error {
        text: String,
    },
    Done,
}

fn at_least_one(what: &str, n: u32) -> Result<(), String> {
    if n == 0 {
        Err(format!(
            "the linker wrote {what} 0, which is not a position or field number"
        ))
    } else {
        Ok(())
    }
}

fn within(what: &str, part: u32, whole: u32) -> Result<(), String> {
    if part > whole {
        Err(format!("the linker wrote {what} {part} of {whole}"))
    } else {
        Ok(())
    }
}

/// The report from the linker's output: every line read, the `done` line required and last, so an
/// IRIS error, a cut-off answer or a line the contract does not describe is an error, never a
/// short report.
pub fn parse_link_output(out: &str) -> Result<LinkReport, String> {
    let mut sample = None;
    let mut ids = Vec::new();
    let mut positions: Vec<PositionLine> = Vec::new();
    let mut nulls = Vec::new();
    let mut data = Vec::new();
    let mut missing = None;
    let mut bitmaps = Vec::new();
    let mut done = false;
    for text in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if done {
            return Err(format!("the linker wrote more after its done line: {text}"));
        }
        let line: Line = serde_json::from_str(text)
            .map_err(|e| format!("the linker wrote something unexpected ({e}): {text}"))?;
        match line {
            Line::Sample {
                nodes,
                distinct_first,
                usable,
                stopped,
            } => {
                within("usable nodes", usable, nodes)?;
                let line = SampleLine {
                    nodes,
                    distinct_first,
                    usable,
                    stopped,
                };
                if sample.replace(line).is_some() {
                    return Err("the linker wrote two sample lines".into());
                }
            }
            Line::Id {
                form,
                positions: at,
                resolved,
                total,
            } => {
                within("resolved IDs", resolved, total)?;
                for p in &at {
                    at_least_one("id position", *p)?;
                }
                ids.push(IdLine {
                    form,
                    positions: at,
                    resolved,
                    total,
                });
            }
            Line::Pos {
                position,
                total,
                distinct,
                scores,
            } => {
                at_least_one("position", position)?;
                if positions.iter().any(|p| p.position == position) {
                    return Err(format!("the linker wrote position {position} twice"));
                }
                let mut checked = Vec::with_capacity(scores.len());
                for s in scores {
                    at_least_one("field", s.field)?;
                    within("matches", s.matches, total)?;
                    check_variant(s.transform, s.length, s.piece)
                        .map_err(|e| format!("the linker wrote a score the table lacks: {e}"))?;
                    checked.push(PositionScore {
                        field: s.field,
                        transform: s.transform,
                        length: s.length,
                        piece: s.piece,
                        matches: s.matches,
                        total,
                    });
                }
                positions.push(PositionLine {
                    position,
                    total,
                    distinct,
                    scores: checked,
                });
            }
            Line::Null {
                position,
                field,
                kind,
                empty,
                count,
                length,
                literal,
            } => {
                at_least_one("position", position)?;
                at_least_one("field", field)?;
                nulls.push(NullLine {
                    position,
                    field,
                    kind,
                    empty,
                    count,
                    length,
                    literal,
                });
            }
            Line::Data {
                field,
                element,
                matches,
                total,
            } => {
                at_least_one("field", field)?;
                at_least_one("element", element)?;
                within("matches", matches, total)?;
                data.push(DataLine {
                    field,
                    element,
                    matches,
                    total,
                });
            }
            Line::Missing {
                share,
                rule,
                field,
                literal,
            } => {
                if !(0.0..=1.0).contains(&share) {
                    return Err(format!("the linker wrote a missing share of {share}"));
                }
                let line = MissingLine {
                    share,
                    rule,
                    field,
                    literal,
                };
                if missing.replace(line).is_some() {
                    return Err("the linker wrote two missing lines".into());
                }
            }
            Line::Bitmap {
                chunk_position,
                ids_checked,
                ids_found,
                later_checked,
                later_found,
                lead,
            } => {
                at_least_one("chunk position", chunk_position)?;
                within("IDs found", ids_found, ids_checked)?;
                within("later IDs checked", later_checked, ids_checked)?;
                within("later IDs found", later_found, later_checked)?;
                bitmaps.push(BitmapLine {
                    chunk_position,
                    ids_checked,
                    ids_found,
                    later_checked,
                    later_found,
                    lead,
                });
            }
            Line::Error { text } => return Err(format!("IRIS error in the linker: {text}")),
            Line::Done => done = true,
        }
    }
    if !done {
        return Err("the linker stopped before it finished (no done line)".into());
    }
    let sample = sample.ok_or("the linker wrote no sample line")?;
    Ok(LinkReport {
        sample,
        ids,
        positions,
        nulls,
        data,
        missing,
        bitmaps,
    })
}

// ── from the report to a link ──────────────────────────────────────────────

/// One non-ID position's verdict, with the distinct count it rested on.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionJudgement {
    pub position: u32,
    pub judgement: Judgement,
    pub distinct: u32,
}

/// A link built from a report, and what its JSON form needs beyond the link itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Assembled {
    pub link: Link,
    /// IDs that found a master row, over nodes read at the ID's depth (0 with no ID line).
    pub id_agreement: f64,
    pub judgements: Vec<PositionJudgement>,
}

/// The link's one verdict (FR-011): no ID position at 0.9 is `not_linked`, fewer than 20 usable
/// nodes `too_few_nodes`; otherwise the weakest position decides, in the order `not_explained`,
/// `constant`, `ambiguous`, `inferred`. An index with no position to explain is `not_explained`:
/// there is no map to draft from it.
pub fn overall_verdict(id_agreement: f64, usable_nodes: u32, positions: &[Verdict]) -> Verdict {
    if !is_linked(id_agreement) {
        return Verdict::NotLinked;
    }
    if usable_nodes < MIN_NODES {
        return Verdict::TooFewNodes;
    }
    if positions.is_empty() {
        return Verdict::NotExplained;
    }
    [
        Verdict::NotLinked,
        Verdict::TooFewNodes,
        Verdict::NotExplained,
        Verdict::Constant,
        Verdict::Ambiguous,
    ]
    .into_iter()
    .find(|v| positions.contains(v))
    .unwrap_or(Verdict::Inferred)
}

/// The link for `global` from one linker run: the best ID line is the row ID, each non-ID
/// position is judged by [`verdict`], and the link's verdict follows from them. The covering data
/// lines are not read here yet.
pub fn assemble(global: &str, map: &str, report: &LinkReport) -> Assembled {
    let best_id = report.ids.first();
    let id_agreement = best_id.map_or(0.0, |l| ratio(l.resolved, l.total));
    let id = best_id.map_or(
        IdLink {
            form: IdForm::Subscript,
            positions: Vec::new(),
        },
        |l| IdLink {
            form: l.form,
            positions: l.positions.clone(),
        },
    );
    let usable = report.sample.usable;
    let mut judgements = Vec::new();
    let mut subscripts = Vec::new();
    for line in &report.positions {
        let judgement = verdict(&VerdictInput {
            usable_nodes: usable,
            id_agreement,
            distinct: line.distinct,
            scores: &line.scores,
        });
        subscripts.push(SubscriptLink {
            position: line.position,
            verdict: judgement.verdict,
            pick: judgement
                .best
                .clone()
                .filter(|_| judgement.verdict.is_explained()),
            alternatives: judgement.alternatives.clone(),
            null_marker: report
                .nulls
                .iter()
                .find(|n| n.position == line.position)
                .map(NullMarker::from_line),
        });
        judgements.push(PositionJudgement {
            position: line.position,
            judgement,
            distinct: line.distinct,
        });
    }
    subscripts.sort_by_key(|s| s.position);
    judgements.sort_by_key(|j| j.position);
    let verdicts: Vec<Verdict> = subscripts.iter().map(|s| s.verdict).collect();
    Assembled {
        link: Link {
            global: global.to_string(),
            map_name: map.to_string(),
            verdict: overall_verdict(id_agreement, usable, &verdicts),
            id,
            subscripts,
            data_in_value: covering_data(report),
        },
        id_agreement,
        judgements,
    }
}

/// The `$LIST` elements of the node value that hold a master field on at least 95% of the usable
/// nodes, one per field.
fn covering_data(report: &LinkReport) -> Vec<DataInValue> {
    let mut out: Vec<DataInValue> = Vec::new();
    for d in &report.data {
        if report.sample.usable >= MIN_NODES
            && ratio(d.matches, d.total) >= 0.95
            && !out.iter().any(|o| o.field == d.field)
        {
            out.push(DataInValue {
                field: d.field,
                element: d.element,
            });
        }
    }
    out.sort_by_key(|d| d.element);
    out
}

/// An agreement as the answer carries it: four decimals.
fn rounded(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// A pair as the answer spells it: field name, transform, and a length and piece only when it has
/// one, added to `o`.
fn insert_pair(o: &mut Map<String, Value>, score: &PositionScore, master: &MasterView) {
    let name = master_field(master, score.field)
        .map_or_else(|_| format!("Field{}", score.field), str::to_string);
    o.insert("field".into(), json!(name));
    o.insert("transform".into(), json!(score.transform.name()));
    if let Some(n) = score.length {
        o.insert("length".into(), json!(n));
    }
    if let Some(p) = score.piece {
        o.insert("piece".into(), json!(p));
    }
}

/// A pair with its agreement, as an alternative is listed.
fn pair_json(score: &PositionScore, master: &MasterView) -> Value {
    let mut o = Map::new();
    insert_pair(&mut o, score, master);
    o.insert("agreement".into(), json!(rounded(score.agreement())));
    Value::Object(o)
}

fn null_marker_json(m: &NullMarker) -> Value {
    let mut o = Map::new();
    o.insert("kind".into(), json!(m.kind.as_str()));
    o.insert("empty".into(), json!(m.empty.as_str()));
    o.insert("count".into(), json!(m.count));
    if let Some(n) = m.length {
        o.insert("length".into(), json!(n));
    }
    if let Some(l) = &m.literal {
        o.insert("literal".into(), json!(l));
    }
    Value::Object(o)
}

/// The item of `indexes` for a link: the keys of data-model.md that come from the linker's report.
/// `found_by`, `map`, `verified` and `guesses` are added by the caller.
pub fn link_json(a: &Assembled, master: &MasterView, report: &LinkReport) -> Value {
    let subscripts: Vec<Value> = a
        .judgements
        .iter()
        .map(|j| {
            let mut o = Map::new();
            o.insert("position".into(), json!(j.position));
            o.insert("verdict".into(), json!(j.judgement.verdict.as_str()));
            o.insert("agreement".into(), json!(rounded(j.judgement.agreement)));
            if let (true, Some(best)) = (j.judgement.verdict.is_explained(), &j.judgement.best) {
                insert_pair(&mut o, best, master);
            }
            let alternatives: Vec<Value> = j
                .judgement
                .alternatives
                .iter()
                .map(|s| pair_json(s, master))
                .collect();
            o.insert("alternatives".into(), Value::Array(alternatives));
            if let Some(m) = a
                .link
                .subscripts
                .iter()
                .find(|s| s.position == j.position)
                .and_then(|s| s.null_marker.as_ref())
            {
                o.insert("null_marker".into(), null_marker_json(m));
            }
            Value::Object(o)
        })
        .collect();
    let not_explained: Vec<Value> = a
        .judgements
        .iter()
        .filter(|j| j.judgement.verdict == Verdict::NotExplained)
        .map(|j| json!({"position": j.position, "best_agreement": rounded(j.judgement.agreement)}))
        .collect();
    let mut item = json!({
        "global": a.link.global,
        "nodes_sampled": report.sample.nodes,
        "distinct_first_values": report.sample.distinct_first,
        "usable_nodes": report.sample.usable,
        "id": {
            "form": a.link.id.form.as_str(),
            "positions": a.link.id.positions,
            "agreement": rounded(a.id_agreement),
        },
        "subscripts": subscripts,
        "data_in_value": a.link.data_in_value.iter().map(|d| json!({
            "field": master_field(master, d.field).map_or_else(|_| format!("Field{}", d.field), str::to_string),
            "element": d.element,
        })).collect::<Vec<_>>(),
        "verdict": a.link.verdict.as_str(),
        "not_explained": not_explained,
    });
    if let Some((count, share)) = orphan_nodes(a, report) {
        item["orphan_nodes"] = json!({"count": count, "share": rounded(share)});
    }
    item
}

/// Index nodes that found no master row (deleted rows leave them behind), as a count and a share of
/// the nodes read at the ID's depth: reported only for a linked index and only above 5%. Below that
/// they change nothing; under the 0.9 floor the link is `not_linked` and its agreement says so.
pub fn orphan_nodes(a: &Assembled, report: &LinkReport) -> Option<(u32, f64)> {
    let best = report.ids.first()?;
    let share = 1.0 - ratio(best.resolved, best.total);
    (is_linked(a.id_agreement) && share > ORPHAN_SHARE).then(|| (best.total - best.resolved, share))
}

// ── drafting ───────────────────────────────────────────────────────────────

/// What the master draft (or the class) calls things: the global, the field names by position
/// (field 1 first), and the row-ID part names (the master's own in class mode, placeholders in
/// global mode). Stands in for `master.rs`'s spec until the two meet.
#[derive(Debug, Clone, PartialEq)]
pub struct MasterView {
    pub global: String,
    pub field_names: Vec<String>,
    pub key_names: Vec<String>,
}

/// Where the row ID sits in an index node.
#[derive(Debug, Clone, PartialEq)]
pub struct IdLink {
    pub form: IdForm,
    /// The run of subscript positions holding the key; empty for `IdForm::Value`.
    pub positions: Vec<u32>,
}

/// One non-ID subscript position of a link, with the pair that explains it and, for an ambiguous
/// one, every tied pair.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscriptLink {
    pub position: u32,
    pub verdict: Verdict,
    pub pick: Option<PositionScore>,
    pub alternatives: Vec<PositionScore>,
    /// The marker the index files for an empty field at this position, when one was seen.
    pub null_marker: Option<NullMarker>,
}

/// The null marker found at one position: which kind, what it stands for, how many nodes it covers,
/// and (a discovered marker only) its length and, under `include_values`, its literal.
#[derive(Debug, Clone, PartialEq)]
pub struct NullMarker {
    pub kind: NullKind,
    pub empty: EmptyKind,
    pub count: u32,
    pub length: Option<u32>,
    pub literal: Option<String>,
}

impl NullMarker {
    fn from_line(l: &NullLine) -> Self {
        Self {
            kind: l.kind,
            empty: l.empty,
            count: l.count,
            length: l.length,
            literal: l.literal.clone(),
        }
    }

    /// The `NullMarker` expression a draft needs, `Ok(None)` for a marker the engine files itself
    /// (`$c(0)` for an empty string, `-1E14` for NULL), `Err` when the marker is not known (a
    /// discovered one without its literal, or one with a control character).
    pub fn expression(&self) -> Result<Option<String>, String> {
        match self.kind {
            NullKind::C0 | NullKind::Minus1e14 => Ok(None),
            NullKind::Space => Ok(Some("\" \"".into())),
            NullKind::SpaceC0 => Ok(Some("$c(32,0)".into())),
            NullKind::Discovered => {
                let lit = self
                    .literal
                    .as_deref()
                    .ok_or("the marker's literal is not known")?;
                if lit.chars().any(char::is_control) {
                    return Err("the marker holds a control character".into());
                }
                Ok(Some(format!("\"{}\"", lit.replace('"', "\"\""))))
            }
        }
    }
}

/// A node value that is a `$LIST` whose element `element` is master field `field`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataInValue {
    pub field: u32,
    pub element: u32,
}

/// An index global linked to its master, as far as a draft needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub global: String,
    pub map_name: String,
    pub verdict: Verdict,
    pub id: IdLink,
    pub subscripts: Vec<SubscriptLink>,
    pub data_in_value: Vec<DataInValue>,
}

pub(super) fn master_field(master: &MasterView, field: u32) -> Result<&str, String> {
    usize::try_from(field)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| master.field_names.get(i))
        .map(String::as_str)
        .ok_or_else(|| {
            format!(
                "field {field} is not one of the master's {} fields",
                master.field_names.len()
            )
        })
}

/// The expression of the level `position` holds: the key part for an ID position, the spelling
/// of the explained pair otherwise. `Ok(None)` when nothing holds it.
fn level_expression(
    link: &Link,
    master: &MasterView,
    position: u32,
    guesses: &mut Vec<String>,
) -> Result<Option<String>, String> {
    if let Some(run) = link.id.positions.iter().position(|p| *p == position) {
        if link.id.positions.len() == 1 {
            return Ok(Some("{ID}".into()));
        }
        let name = master.key_names.get(run).ok_or_else(|| {
            format!(
                "the row ID is {} subscripts and the master names {} key parts",
                link.id.positions.len(),
                master.key_names.len()
            )
        })?;
        if !is_field_name(name) {
            return Err(format!("{name:?} is not a key part name"));
        }
        return Ok(Some(format!("{{{name}}}")));
    }
    let mut held = link.subscripts.iter().filter(|s| s.position == position);
    let Some(sub) = held.next() else {
        return Ok(None);
    };
    if held.next().is_some() {
        return Err(format!("position {position} is explained twice"));
    }
    if !sub.verdict.is_explained() {
        return Err(format!(
            "position {position} is {}: no expression to draft",
            sub.verdict.as_str()
        ));
    }
    let pick = sub
        .pick
        .as_ref()
        .ok_or_else(|| format!("position {position} has no pair to spell"))?;
    if sub.verdict == Verdict::Ambiguous {
        guesses.push(format!("/maps/{}/subscripts/{position}", link.map_name));
    }
    spelling(
        pick.transform,
        master_field(master, pick.field)?,
        pick.length,
        pick.piece,
    )
    .map(Some)
}

/// The index map for `link` in the model's JSON form, and the path of every element it guessed
/// (the pick of an ambiguous position). The map holds only expression spellings, the master's
/// names and the global, so it holds no sampled value, and it passes the model validator or this
/// is an error: a link that is not `inferred` or `ambiguous`, a position nothing explains or that
/// is constant, a gap in the positions, a field the master lacks, a global that is not a plain
/// name.
pub fn draft_index_map(link: &Link, master: &MasterView) -> Result<(Value, Vec<String>), String> {
    if !link.verdict.is_explained() {
        return Err(format!(
            "{} is {}: no map is drafted",
            link.global,
            link.verdict.as_str()
        ));
    }
    if link.map_name.is_empty() || !link.map_name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!("{:?} is not a map name", link.map_name));
    }
    let bare = link.global.strip_prefix('^').unwrap_or(&link.global);
    if !link.global.starts_with('^') || crate::objectscript::validate_global_name(bare).is_err() {
        return Err(format!("{:?} is not a plain global name", link.global));
    }
    match (link.id.form, link.id.positions.is_empty()) {
        (IdForm::Subscript, true) => {
            return Err("the row ID is in the subscripts but no position holds it".into())
        }
        (IdForm::Value, false) => {
            return Err("the row ID is in the node value but a position holds it".into())
        }
        _ => {}
    }
    let all_positions = || {
        link.id
            .positions
            .iter()
            .copied()
            .chain(link.subscripts.iter().map(|s| s.position))
    };
    if all_positions().any(|p| p == 0) {
        return Err("position 0 is not a position".into());
    }
    let levels = all_positions().max().unwrap_or(0);
    if levels == 0 {
        return Err("the index has no subscript to draft".into());
    }
    let mut guesses = Vec::new();
    let mut subscripts = Vec::new();
    for position in 1..=levels {
        let expression = level_expression(link, master, position, &mut guesses)?
            .ok_or_else(|| format!("position {position} of {levels} holds nothing"))?;
        let mut level = json!({"level": position, "expression": expression});
        let marker = link
            .subscripts
            .iter()
            .find(|s| s.position == position)
            .and_then(|s| s.null_marker.as_ref());
        if let Some(m) = marker {
            match m.expression() {
                Ok(Some(e)) => level["null_marker"] = json!(e),
                Ok(None) => {}
                Err(_) => guesses.push(format!(
                    "/maps/{}/subscripts/{position}/null_marker",
                    link.map_name
                )),
            }
        }
        subscripts.push(level);
    }
    let mut map = Map::new();
    map.insert("name".into(), json!(link.map_name));
    map.insert("type".into(), json!("index"));
    map.insert("global".into(), json!(link.global));
    map.insert("subscripts".into(), Value::Array(subscripts));
    let mut data = Map::new();
    if link.id.form == IdForm::Value {
        data.insert("Id".into(), json!({"name": "Id"}));
        map.insert(
            "row_id".into(),
            json!([{"position": 1, "expression": "{Id}", "field": "Id"}]),
        );
    }
    for d in &link.data_in_value {
        let name = master_field(master, d.field)?;
        if !is_field_name(name) {
            return Err(format!("{name:?} is not a field name"));
        }
        let entry = json!({"name": name, "piece": d.element.to_string()});
        if data.insert(name.to_string(), entry).is_some() {
            return Err(format!("{name} is covered twice"));
        }
    }
    if !data.is_empty() {
        map.insert("data".into(), Value::Object(data));
    }
    let map = Value::Object(map);
    check_with_validator(&map)?;
    Ok((map, guesses))
}

/// The `<Type>%Storage.SQL</Type>` node every SQL Storage tree starts with.
pub(super) fn storage_type_node() -> Node {
    Node {
        tag: "Type".into(),
        name: None,
        value: Some("%Storage.SQL".into()),
        children: None,
    }
}

/// The model validator on `map` in a Storage tree, as the write path will see it.
pub(super) fn check_with_validator(map: &Value) -> Result<(), String> {
    let nodes = vec![storage_type_node(), map_node(map)?];
    validate(&nodes).map_err(|found| {
        let broken: Vec<String> = found
            .iter()
            .map(|i| format!("{} ({})", i.path, i.rule))
            .collect();
        format!(
            "the drafted map breaks the model's rules: {}",
            broken.join(", ")
        )
    })
}
