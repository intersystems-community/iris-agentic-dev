//! The class draft `propose` returns (spec 173, US6, FR-020 to FR-022): one call to the server that
//! counts what the values of each master field look like, the type and collation each property
//! takes from those counts and from the linking, and the scratch compile of the whole class with
//! the drafted maps. The pure halves of typing are in `typing.rs`; this file is the generator, the
//! reader of its lines, and the part that talks to IRIS.
//!
//! # What the server counts
//!
//! The generated code walks the master the way the linker does: the first-level keys with
//! `$Order` (a counting pass, under ten times the node cap), then every k-th key so the sample
//! reaches the node cap (for a single-part key, one row per key; for a longer key, up to three
//! rows under each sampled first key). It reads each sampled row's fields by the draft's own node,
//! delimiter and piece, and for each field counts, over the values that are not empty:
//!
//! - `of`: the values examined;
//! - `numeric`: those `$IsValidNum` accepts and that equal their own canonical number (`v=+v`),
//!   so `007`, `12.50` and `1E3` are text, as the global files them;
//! - `integer`: the numeric ones with no fraction;
//! - `max_scale`: the most fraction digits of any numeric value;
//! - `date_fit`: the integers between the day numbers `$ZDATEH("1900-01-01",3)` and
//!   `$ZDATEH("2100-12-31",3)`, both asked of the server's own date function;
//! - `max_length`: the longest value, in characters.
//!
//! A value of `date_fit` is a day number (`$HOROLOG` form), which is what a `%Date` property
//! holds. A date written as text (`2024-03-15`) is text and stays `%String`.
//!
//! # Output: one JSON object per line
//!
//! ```text
//! {"t":"rows","n":40,"stopped":"none"}
//! {"t":"field","field":1,"of":40,"integer":40,"numeric":40,"max_scale":0,"date_fit":0,"max_length":2}
//! {"t":"error","text":"<IRIS error name and location>"}
//! {"t":"done"}
//! ```
//!
//! No value is written; every `Write` in the code writes a JSON object of counts. A property name
//! with an underscore is set through `%Set` because bare `o.max_scale` reads `_` as concatenation.

use super::link::{storage_type_node, Link, Transform, Verdict};
use super::link_code::{field_access, fields_table, global_literal, FieldAccess};
use super::link_run::detail;
use super::link_verify::answer_text;
use super::master::MasterSpec;
use super::model::map_node;
use super::tree::{emit_body, normalize, Node};
use super::typing::{
    class_source, collation_parameter, property_type, ClassDraft, Evidence, PropertySpec,
};
use super::write::{scratch_compile, scratch_probe};
use crate::iris::connection::IrisConnection;
use serde::Deserialize;
use serde_json::Value;

/// What the typing call needs to know about the master.
#[derive(Debug, Clone)]
pub struct TypingRequest<'a> {
    /// The master global, with its `^`.
    pub master: &'a str,
    /// Parts of the master's row ID (1 for a single key).
    pub key_parts: u32,
    /// Fields of the master draft, by number (`Field1` is entry 0).
    pub fields: &'a [FieldAccess],
    /// Rows read at most (the call's node cap).
    pub node_cap: u32,
    /// Seconds for the whole call.
    pub time_cap_secs: u32,
}

/// The typing call's ObjectScript, or why it cannot be built. The master name reaches the code
/// only through `global_literal` (a validated global name, quoted); everything else is a number
/// or a constant of this file.
pub fn build_code(req: &TypingRequest<'_>) -> Result<String, String> {
    let master = global_literal(req.master)?;
    if req.key_parts == 0 {
        return Err("a master has at least one key part".into());
    }
    if req.fields.is_empty() {
        return Err("a master draft has at least one field".into());
    }
    let mut code = format!(
        "Set mst={master},t0=$ZH\n Set nodecap={},ccap={},secs={},kp={}\n",
        req.node_cap.max(1),
        req.node_cap.max(1).saturating_mul(10),
        req.time_cap_secs,
        req.key_parts,
    );
    code.push_str(&fields_table(req.fields)?);
    code.push_str(BODY);
    Ok(code)
}

/// The walk, the counts and the lines. Rows are collected as references first (`rr`), then read
/// once each. A `Write` here writes a JSON object of counts and never a value.
const BODY: &str = r#" Set lo=$ZDATEH("1900-01-01",3),hi=$ZDATEH("2100-12-31",3)
 For j=1:1:nf { Set cn(j)=0,it(j)=0,nm(j)=0,sc(j)=0,df(j)=0,ml(j)=0 }
 Set failed=0,nr=0,stopped="none"
 Try {
 Set k="",nk=0
 For { Set k=$Order(@mst@(k)) Quit:k=""  Set nk=nk+1,keys(nk)=k
   If nk>=ccap { Set stopped="count_cap" Quit }
   If ($ZH-t0)>secs { Set stopped="time" Quit } }
 If kp=1 { Set stride=(nk-1)\nodecap+1,per=1 }
 Else { Set want=nodecap\3,stride=$Select(nk<7:1,want<1:1,nk\want<1:1,1:nk\want),per=$Select(nk<7:nodecap,1:3) }
 For j=1:stride:nk { Quit:nr>=nodecap  Quit:($ZH-t0)>secs
   Set key=keys(j),n=$Name(@mst@(key))
   If kp=1 { Set nr=nr+1,rr(nr)=n }
   Else { Set got=0,prev=""
     For { Set n=$Query(@n) Quit:n=""  Quit:$QSubscript(n,1)'=key  Quit:got>=per  Quit:nr>=nodecap
       If $QLength(n)>=kp { Set rk="" For q=1:1:kp { Set rk=rk_$Char(1)_$QSubscript(n,q) }
         If rk'=prev { Set prev=rk,mr=mst For q=1:1:kp { Set mr=$Name(@mr@($QSubscript(n,q))) } Set nr=nr+1,rr(nr)=mr,got=got+1 } } } } }
 For i=1:1:nr { Quit:($ZH-t0)>secs
   Set mr=rr(i)
   For j=1:1:nf { Set fnode=$List(fa(j),1),fd=$List(fa(j),2),fp=$List(fa(j),3)
     Set r=mr Set:fnode'="" r=$Name(@r@(fnode))
     Set raw=$Get(@r)
     If fd=0 { Set v=$Select($ListValid(raw)&&($ListLength(raw)>=fp):$ListGet(raw,fp),1:"") } Else { Set v=$Piece(raw,$Char(fd),fp) }
     If v'="" {
       Set cn(j)=cn(j)+1,ln=$Length(v) Set:ln>ml(j) ml(j)=ln
       If $IsValidNum(v)&&(v=+v) {
         Set nm(j)=nm(j)+1,fr=$Length($Piece(v,".",2)) Set:fr>sc(j) sc(j)=fr
         If fr=0 { Set it(j)=it(j)+1 If (v>=lo)&&(v<=hi) { Set df(j)=df(j)+1 } } } } } }
 If ($ZH-t0)>secs { Set stopped="time" } ElseIf (stopped="none")&&(nr>=nodecap) { Set stopped="nodes" }
 Set o={},o.t="rows",o.n=nr,o.stopped=stopped Write o.%ToJSON(),!
 For j=1:1:nf { Set o={},o.t="field",o.field=j,o.integer=it(j),o.numeric=nm(j)
   Do o.%Set("of",cn(j)) Do o.%Set("max_scale",sc(j)) Do o.%Set("date_fit",df(j)) Do o.%Set("max_length",ml(j))
   Write o.%ToJSON(),! }
 } Catch ex {
 Set failed=1,o={},o.t="error",o.text=ex.Name_" "_ex.Location
 Write o.%ToJSON(),!
 }
 If 'failed { Set o={},o.t="done" Write o.%ToJSON(),! }
"#;

/// What the call reported.
#[derive(Debug, Clone, PartialEq)]
pub struct TypingReport {
    /// Rows read.
    pub rows: u32,
    /// Why the walk stopped: `none`, `nodes`, `time` or `count_cap`.
    pub stopped: String,
    /// One entry per field of the master, in field order.
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Line {
    Rows {
        n: u32,
        stopped: String,
    },
    Field {
        field: u32,
        of: u32,
        integer: u32,
        numeric: u32,
        max_scale: u32,
        date_fit: u32,
        max_length: u32,
    },
    Error {
        text: String,
    },
    Done,
}

/// Read what the typing call wrote for a master of `field_count` fields. A run with no `done` line
/// failed; an `error` line is its text; counts that cannot be true (more whole numbers than
/// numbers, more numbers than values) are refused.
pub fn parse_output(out: &str, field_count: usize) -> Result<TypingReport, String> {
    let mut rows: Option<(u32, String)> = None;
    let mut evidence: Vec<Option<Evidence>> = vec![None; field_count];
    let mut done = false;
    for text in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if done {
            return Err(format!(
                "the typing call wrote more after its done line: {text}"
            ));
        }
        let line: Line = serde_json::from_str(text)
            .map_err(|e| format!("the typing call wrote something unexpected ({e}): {text}"))?;
        match line {
            Line::Rows { n, stopped } => {
                if rows.replace((n, stopped)).is_some() {
                    return Err("the typing call wrote two rows lines".into());
                }
            }
            Line::Field {
                field,
                of,
                integer,
                numeric,
                max_scale,
                date_fit,
                max_length,
            } => {
                let slot = usize::try_from(field)
                    .ok()
                    .and_then(|f| f.checked_sub(1))
                    .and_then(|i| evidence.get_mut(i))
                    .ok_or_else(|| {
                        format!("the typing call wrote field {field} of a master of {field_count}")
                    })?;
                if slot.is_some() {
                    return Err(format!("the typing call wrote field {field} twice"));
                }
                if !(date_fit <= integer && integer <= numeric && numeric <= of) {
                    return Err(format!(
                        "field {field}: counts that cannot be true (of {of}, numeric {numeric}, \
                         integer {integer}, date_fit {date_fit})"
                    ));
                }
                *slot = Some(Evidence {
                    of,
                    integer,
                    numeric,
                    max_scale,
                    date_fit,
                    max_length,
                });
            }
            Line::Error { text } => return Err(format!("the typing call failed: {text}")),
            Line::Done => done = true,
        }
    }
    if !done {
        return Err("the typing call ended without a done line".into());
    }
    let (rows, stopped) = rows.ok_or("the typing call wrote no rows line")?;
    let evidence = evidence
        .into_iter()
        .enumerate()
        .map(|(i, e)| e.ok_or_else(|| format!("the typing call wrote nothing for field {}", i + 1)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TypingReport {
        rows,
        stopped,
        evidence,
    })
}

/// The collation the linking found for one master field: the field's number (1-based, as the
/// linker numbers them), the transform and its length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldCollation {
    pub field: u32,
    pub transform: Transform,
    pub length: Option<u32>,
}

/// The collations a link's evidence supports: each non-ID position whose pair is `inferred` (a
/// tie is a guess, not evidence) and is the whole subscript (a piece of a collated value is no
/// property collation).
pub fn collation_picks(link: &Link) -> Vec<FieldCollation> {
    if !link.verdict.is_explained() {
        return Vec::new();
    }
    link.subscripts
        .iter()
        .filter(|s| s.verdict == Verdict::Inferred)
        .filter_map(|s| s.pick.as_ref())
        .filter(|p| p.piece.is_none())
        .map(|p| FieldCollation {
            field: p.field,
            transform: p.transform,
            length: p.length,
        })
        .collect()
}

/// A property for each of `names` with the type its `evidence` supports, and the collation of the
/// first pick for its field. Only a `%String` takes a `COLLATION` parameter (IRIS refuses it on
/// the others, ERROR #5480), so the others take none.
pub fn properties(
    names: &[String],
    evidence: &[Evidence],
    picks: &[FieldCollation],
) -> Result<Vec<PropertySpec>, String> {
    if names.len() != evidence.len() {
        return Err(format!(
            "{} field names and {} counts",
            names.len(),
            evidence.len()
        ));
    }
    if let Some(p) = picks
        .iter()
        .find(|p| usize::try_from(p.field).map_or(true, |f| f == 0 || f > names.len()))
    {
        return Err(format!(
            "a collation for field {}, and the master has {} fields",
            p.field,
            names.len()
        ));
    }
    names
        .iter()
        .zip(evidence)
        .enumerate()
        .map(|(i, (name, e))| {
            let ty = property_type(e);
            let collation = match picks.iter().find(|p| p.field as usize == i + 1) {
                Some(p) if ty.name == "%String" => {
                    collation_parameter(p.transform.name(), p.length)?
                }
                _ => None,
            };
            Ok(PropertySpec {
                name: name.clone(),
                ty,
                collation,
            })
        })
        .collect()
}

/// The class the draft is: `Draft.` and the global's letters and digits (`G` first when it does
/// not begin with a letter).
pub fn class_name_for(global: &str) -> String {
    let mut body: String = global.chars().filter(char::is_ascii_alphanumeric).collect();
    if body.is_empty() {
        body = "Master".into();
    } else if !body.starts_with(|c: char| c.is_ascii_alphabetic()) {
        body.insert(0, 'G');
    }
    format!("Draft.{body}")
}

/// The Storage tree of `maps`: the `%Storage.SQL` type and one `SQLMap` each.
pub fn storage_nodes(maps: &[&Value]) -> Result<Vec<Node>, String> {
    let mut nodes = vec![storage_type_node()];
    for m in maps {
        nodes.push(map_node(m)?);
    }
    Ok(nodes)
}

/// `source` (a `class_source` text, which ends in the class's closing brace) with a Storage block
/// of `maps` before that brace. The class carries `StorageStrategy = SQLStorage` and no block of its
/// own, so a compile without this fails.
pub fn splice_storage(source: &str, maps: &[&Value]) -> Result<String, String> {
    with_storage_block(source, "", &storage_nodes(maps)?)
}

/// `source` (a `class_source` text) with `members` (class members, each ending in a blank line, or
/// nothing) and a `Storage SQLStorage` block of `nodes` before its closing brace.
pub(super) fn with_storage_block(
    source: &str,
    members: &str,
    nodes: &[Node],
) -> Result<String, String> {
    let body = source
        .trim_end()
        .strip_suffix('}')
        .ok_or("the class draft does not end in a brace")?;
    Ok(format!(
        "{body}{members}Storage SQLStorage\n{{\n{}\n}}\n\n}}\n",
        emit_body(&normalize(nodes.to_vec()))
    ))
}

/// Everything the class draft needs from the call.
pub struct DraftRun<'a> {
    pub conn: &'a IrisConnection,
    pub client: &'a reqwest::Client,
    pub ns: &'a str,
    pub master: &'a MasterSpec,
    /// The master draft in model JSON (with the caller's field names applied).
    pub master_map: &'a Value,
    /// The index maps that verified, in model JSON.
    pub index_maps: &'a [Value],
    pub picks: &'a [FieldCollation],
    pub node_cap: u32,
}

/// The class draft and what the call could not say.
pub struct Drafted {
    /// The `class_draft` answer (data-model.md).
    pub answer: Value,
    /// `{what, why}` items for `not_inferred_detail`.
    pub notes: Vec<Value>,
}

/// Count the master's values on the server, type each property, build the class with its Storage
/// block and compile it in a scratch copy. `Err` is the reason there is no draft at all (the
/// master is one the call does not read, or the call failed); a draft that does not compile is
/// still returned, with `compiled: false` and the compiler's words in `notes`.
pub async fn class_draft(run: &DraftRun<'_>) -> Result<Drafted, String> {
    let master = run.master;
    if master.key_names.len() > 1 {
        return Err(
            "the master has a composite row ID, and the class draft does not draft its key \
             properties"
                .into(),
        );
    }
    if master.subscripts.len() != master.key_count() {
        return Err(
            "the master has a constant subscript, which the typing call does not read".into(),
        );
    }
    let access: Vec<FieldAccess> = master
        .fields
        .iter()
        .map(field_access)
        .collect::<Result<_, _>>()?;
    let key_parts = u32::try_from(master.key_count()).map_err(|_| "too many key parts")?;
    let code = build_code(&TypingRequest {
        master: &master.global,
        key_parts,
        fields: &access,
        node_cap: run.node_cap,
        time_cap_secs: 5,
    })?;
    let out = run
        .conn
        .execute_via_generator_prefixed(&code, run.ns, run.client, "IrisDevSm")
        .await
        .map_err(|e| format!("the typing call could not run: {e}"))?;
    let report = parse_output(&out, access.len())?;
    let names: Vec<String> = master.fields.iter().map(|f| f.name.clone()).collect();
    let props = properties(&names, &report.evidence, run.picks)?;
    let draft = class_source(&class_name_for(&master.global), &props)?;
    let mut maps: Vec<&Value> = vec![run.master_map];
    maps.extend(run.index_maps.iter());
    let text = splice_storage(&draft.source, &maps)?;
    let mut notes = Vec::new();
    if report.stopped != "none" {
        notes.push(detail(
            "class_draft",
            format!(
                "the typing call stopped on its {} cap after {} rows: the types rest on those rows",
                report.stopped, report.rows
            ),
        ));
    }
    let compiled = match compile(run, &text, &maps).await {
        Ok(()) => true,
        Err(why) => {
            notes.push(detail("class_draft did not compile in a scratch copy", why));
            false
        }
    };
    let shown = ClassDraft {
        source: text,
        ..draft
    };
    Ok(Drafted {
        answer: shown.to_json(compiled),
        notes,
    })
}

/// Compile the class text with its maps in a scratch copy; nothing real is touched.
async fn compile(run: &DraftRun<'_>, text: &str, maps: &[&Value]) -> Result<(), String> {
    let nodes = storage_nodes(maps)?;
    scratch_probe(text, &nodes).map_err(|refusal| answer_text(&refusal))?;
    scratch_compile(
        run.conn,
        run.client,
        text,
        &nodes,
        run.ns,
        None,
        "no real class was touched",
    )
    .await
    .map(|_| ())
    .map_err(|answer| answer_text(&answer))
}

/// Put `class_draft` on a `propose` answer, or say in `details` why there is none. The one call
/// `propose` makes for the class draft.
pub async fn add_class_draft(run: &DraftRun<'_>, answer: &mut Value, details: &mut Vec<Value>) {
    match class_draft(run).await {
        Ok(drafted) => {
            answer["class_draft"] = drafted.answer;
            details.extend(drafted.notes);
        }
        Err(why) => details.push(detail("class_draft was not drafted", why)),
    }
}
