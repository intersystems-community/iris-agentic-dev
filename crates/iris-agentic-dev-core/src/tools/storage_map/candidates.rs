//! The candidates `propose` examines for index links (spec 173, FR-001 to FR-004): globals the
//! caller names (`index_globals`), globals in the namespace whose name begins with the master's,
//! and literal first subscripts of the master that hold sub-trees. This is the pure half: what a
//! candidate is, how the three sources merge and are capped, the check on the caller's names, and
//! the ObjectScript that lists the namespace's globals with the lines it writes. Running that
//! code is the handler's work.

use super::globals::normalize_global;
use super::shape::ShapeReport;
use crate::objectscript::{os_str_expr, validate_global_name};
use serde::Serialize;

/// At most this many candidates are examined; the rest are named, not examined (FR-003).
pub const MAX_CANDIDATES: usize = 8;

/// The class query that lists the namespace's globals (a documented class query: names only).
pub const LISTING_QUERY: &str = "%SYS.GlobalQuery:NameSpaceList";

/// A test names another class query here so IRIS itself refuses the listing and the generated
/// code reports IRIS's own text. Compiled only into test builds (`testing`), so a release binary
/// has no way to change what it runs.
#[cfg(feature = "testing")]
pub static LISTING_QUERY_FOR_TESTS: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// How a candidate was found, best first: a name the caller gave beats the same global found by
/// the listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FoundBy {
    Parameter,
    NamePrefix,
    SubNode,
}

/// What became of a candidate. A candidate starts `Examined`; `cap` and the gates change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Examined,
    AlreadyMapped,
    Refused,
    NotExamined,
}

/// One candidate index global (data-model.md "Candidate").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    /// `^name`.
    pub global: String,
    pub found_by: FoundBy,
    /// For `sub_node`: the literal first subscripts cut off, as the strings the data holds.
    pub sub_prefix: Vec<String>,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Candidate {
    pub fn new(global: &str, found_by: FoundBy) -> Self {
        Self {
            global: global.to_string(),
            found_by,
            sub_prefix: Vec::new(),
            status: Status::Examined,
            reason: None,
        }
    }

    /// The candidate as a person reads it: `^X.x`, or `^X("idx")` for a sub-tree.
    pub fn label(&self) -> String {
        if self.sub_prefix.is_empty() {
            return self.global.clone();
        }
        let subs: Vec<String> = self
            .sub_prefix
            .iter()
            .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
            .collect();
        format!("{}({})", self.global, subs.join(","))
    }
}

/// The master's own sub-trees: each distinct literal first subscript, when the sample saw a first
/// level node that holds a sub-tree (`$Data` 10 or 11). The report keeps names per depth, not
/// per node, so a literal that holds only a value is listed when another first-level node holds
/// a sub-tree; the linker then finds it does not link.
pub fn from_sub_nodes(report: &ShapeReport) -> Vec<Candidate> {
    let holds_sub_tree = report.data_states.get("1").is_some_and(|s| {
        ["10", "11"]
            .iter()
            .any(|k| s.get(*k).copied().unwrap_or(0) > 0)
    });
    if !holds_sub_tree {
        return Vec::new();
    }
    let Some(names) = report.sub_nodes.get("1") else {
        return Vec::new();
    };
    names
        .iter()
        .map(|n| {
            let mut c = Candidate::new(&report.global, FoundBy::SubNode);
            c.sub_prefix = vec![n.clone()];
            c
        })
        .collect()
}

/// Candidates for names `validate_index_globals` accepted.
pub fn from_parameter(names: &[String]) -> Vec<Candidate> {
    names
        .iter()
        .map(|n| Candidate::new(&caret(n), FoundBy::Parameter))
        .collect()
}

/// Candidates from the namespace listing: names that begin with the master's name, the master
/// itself left out, anything that is not a plain global name dropped.
pub fn from_listing(master_name: &str, names: &[String]) -> Vec<Candidate> {
    let master = master_name.strip_prefix('^').unwrap_or(master_name);
    names
        .iter()
        .map(|n| n.strip_prefix('^').unwrap_or(n))
        .filter(|n| *n != master && n.starts_with(master) && validate_global_name(n).is_ok())
        .map(|n| Candidate::new(&format!("^{n}"), FoundBy::NamePrefix))
        .collect()
}

fn caret(name: &str) -> String {
    if name.starts_with('^') {
        name.to_string()
    } else {
        format!("^{name}")
    }
}

/// Removes a candidate named twice (the same global and sub-tree): the first one stays where it
/// is, and takes the better `found_by`, so a parameter wins over `name_prefix`.
pub fn dedupe(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::with_capacity(candidates.len());
    for c in candidates {
        match out
            .iter_mut()
            .find(|o| o.global == c.global && o.sub_prefix == c.sub_prefix)
        {
            Some(o) => o.found_by = o.found_by.min(c.found_by),
            None => out.push(c),
        }
    }
    out
}

/// The result of `cap`: the candidates to examine and the ones left out.
#[derive(Debug, Clone, PartialEq)]
pub struct Capped {
    pub kept: Vec<Candidate>,
    /// Each has status `not_examined` and the reason.
    pub not_examined: Vec<Candidate>,
}

impl Capped {
    /// The names of the candidates not examined, for the answer's `not_examined` list.
    pub fn not_examined_names(&self) -> Vec<String> {
        self.not_examined.iter().map(Candidate::label).collect()
    }
}

/// Keeps the first `max` candidates and marks the rest `not_examined`.
pub fn cap(mut candidates: Vec<Candidate>, max: usize) -> Capped {
    let mut rest = if candidates.len() > max {
        candidates.split_off(max)
    } else {
        Vec::new()
    };
    for c in &mut rest {
        c.status = Status::NotExamined;
        c.reason = Some(cap_reason(max));
    }
    Capped {
        kept: candidates,
        not_examined: rest,
    }
}

/// Why a candidate past the cap was not examined.
pub fn cap_reason(max: usize) -> String {
    format!("more than {max} candidates; only the first {max} are examined")
}

/// Checks the caller's `index_globals`: at most 8 names, each a plain global name, with or without
/// its caret. Returns the names with a caret, or the `INVALID_PARAMS` text. Nothing the caller
/// wrote reaches generated code unless it passed `validate_global_name`.
pub fn validate_index_globals(names: &[String]) -> Result<Vec<String>, String> {
    if names.len() > MAX_CANDIDATES {
        return Err(format!(
            "index_globals has {} names; at most {MAX_CANDIDATES} are examined",
            names.len()
        ));
    }
    names
        .iter()
        .enumerate()
        .map(|(i, raw)| plain_global(raw).map_err(|why| format!("index_globals[{i}]: {why}")))
        .collect()
}

fn plain_global(raw: &str) -> Result<String, String> {
    let shown = shown(raw);
    if let Ok(g) = normalize_global(raw) {
        if g.extended.is_some() {
            return Err(format!(
                "{shown} is an extended reference; give the plain global name and set 'namespace'"
            ));
        }
        if g.subscripts_prefix.is_some() {
            return Err(format!(
                "{shown} has subscripts; give the plain global name"
            ));
        }
    }
    let bare = raw.strip_prefix('^').unwrap_or(raw);
    if validate_global_name(bare).is_err() {
        return Err(format!(
            "{shown} is not a global name (an optional %, a letter, then letters and digits with \
             single periods between them)"
        ));
    }
    Ok(format!("^{bare}"))
}

/// The caller's text as it goes into a message: quoted, escaped, and cut at 40 characters.
fn shown(raw: &str) -> String {
    let cut: String = raw.chars().take(40).collect();
    let more = if raw.chars().count() > 40 { "..." } else { "" };
    format!("{cut:?}{more}")
}

/// The ObjectScript that lists the namespace's globals whose name begins with `master_name`
/// (FR-002): the class query `%SYS.GlobalQuery:NameSpaceList` with the mask `<name>*`. It writes
/// one `{"g":"<name>"}` line per global, without the caret, then `{"done":1}`, or one
/// `{"error":"<text>"}` line. `master_name` is checked as a global name and the mask is quoted
/// by `os_str_expr`; no other caller text is in the code.
pub fn listing_code(master_name: &str) -> Result<String, String> {
    listing_code_for(master_name, LISTING_QUERY)
}

/// [`listing_code`] for the class query `query` (`Class.Name:QueryName`). Production always passes
/// [`LISTING_QUERY`]; a test passes a query IRIS does not have, so the failure path runs on
/// IRIS's own refusal. The name is checked as a class query name and quoted by `os_str_expr`.
pub fn listing_code_for(master_name: &str, query: &str) -> Result<String, String> {
    let bare = master_name.strip_prefix('^').unwrap_or(master_name);
    if validate_global_name(bare).is_err() {
        return Err(format!("{} is not a global name", shown(master_name)));
    }
    if !is_class_query(query) {
        return Err(format!("{} is not a class query name", shown(query)));
    }
    let mask = os_str_expr(&format!("{bare}*"));
    let query = os_str_expr(query);
    Ok(format!(
        "Try {{
 Kill %objlasterror
 Set rs=##class(%ResultSet).%New({query})
 If '$IsObject(rs) {{ Write {{\"error\":($System.Status.GetErrorText($Get(%objlasterror)))}}.%ToJSON(),! Quit }}
 Set sc=rs.Execute($Namespace,{mask},1,,,0)
 If $System.Status.IsError(sc) {{ Write {{\"error\":($System.Status.GetErrorText(sc))}}.%ToJSON(),! Quit }}
 While rs.Next() {{ Write {{\"g\":(rs.GetData(1))}}.%ToJSON(),! }}
 Write {{\"done\":1}}.%ToJSON(),!
}} Catch e {{ Write {{\"error\":(e.DisplayString())}}.%ToJSON(),! }}"
    ))
}

/// `Class.Name:QueryName`: a class name (an optional `%`, a letter, then letters, digits and
/// periods) and a query name (letters and digits).
fn is_class_query(query: &str) -> bool {
    let Some((class, name)) = query.split_once(':') else {
        return false;
    };
    let class = class.strip_prefix('%').unwrap_or(class);
    class.starts_with(|c: char| c.is_ascii_alphabetic())
        && class.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The most string first subscripts the scan for sub-trees looks at, and the most sub-tree names
/// it keeps. Numbers sort before strings, so a master with a million numeric row IDs is scanned
/// from its first string subscript, not walked.
const SCAN_KEYS: u32 = 2000;
const SCAN_NAMES: u32 = 16;

/// The ObjectScript that lists the literal (string) first subscripts of the master that hold a
/// sub-tree (`$Data` 10 or 11), for the candidates inside the master global (FR-002). It writes
/// one `{"s":"<name>"}` line each and `{"sdone":1,"capped":0|1}`, or one `{"serror":"<text>"}`.
/// It reads subscripts and `$Data`, never a value. Appended to [`listing_code`]'s code, it runs in
/// the same IRIS call. The master name is checked and quoted; no other caller text is in the code.
pub fn sub_tree_code(master_name: &str) -> Result<String, String> {
    let bare = master_name.strip_prefix('^').unwrap_or(master_name);
    if validate_global_name(bare).is_err() {
        return Err(format!("{} is not a global name", shown(master_name)));
    }
    let master = os_str_expr(&format!("^{bare}"));
    // splice-ok: SCAN_KEYS and SCAN_NAMES are constants
    Ok(format!(
        "Try {{
 Set mst={master},sk=$Char(0),sn=0,ss=0,scap=0
 For {{ Set sk=$Order(@mst@(sk)) Quit:sk=\"\"  Set ss=ss+1
  If ss>{SCAN_KEYS} {{ Set scap=1 Quit }}
  If $Data(@mst@(sk))>=10 {{ Write {{\"s\":(sk)}}.%ToJSON(),! Set sn=sn+1 If sn>={SCAN_NAMES} {{ Set scap=1 Quit }} }} }}
 Write {{\"sdone\":1,\"capped\":(scap)}}.%ToJSON(),!
}} Catch e2 {{ Write {{\"serror\":(e2.DisplayString())}}.%ToJSON(),! }}"
    ))
}

/// What the scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubTrees {
    /// The literal first subscripts that hold a sub-tree, in subscript order.
    pub names: Vec<String>,
    /// The scan stopped at its key or name cap, so there may be more.
    pub capped: bool,
}

/// The scan's lines. An `serror` line is an error with IRIS's text; output with no `sdone` line is
/// an error, not a short list.
pub fn parse_sub_trees(out: &str) -> Result<SubTrees, String> {
    let mut names = Vec::new();
    let mut done = None;
    for line in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let row: serde_json::Value = serde_json::from_str(line)
            .map_err(|_| format!("the scan wrote something unexpected: {line}"))?;
        if let Some(e) = row["serror"].as_str() {
            return Err(e.to_string());
        }
        if row["sdone"] == 1 {
            done = Some(row["capped"] == 1);
        } else if let Some(n) = row["s"].as_str() {
            names.push(n.to_string());
        }
    }
    match done {
        Some(capped) => Ok(SubTrees { names, capped }),
        None => Err("the scan stopped before it finished".into()),
    }
}

/// Candidates inside the master from the scan's names, and how many names were left out: a name
/// the map cannot spell as a literal (empty, over 64 characters, a quote or a control character).
pub fn from_sub_trees(master_name: &str, names: &[String]) -> (Vec<Candidate>, usize) {
    let spellable = |n: &String| {
        !n.is_empty()
            && n.chars().count() <= 64
            && n.chars()
                .all(|c| (c.is_ascii_graphic() || c == ' ') && c != '"')
    };
    let global = caret(master_name);
    let found: Vec<Candidate> = names
        .iter()
        .filter(|n| spellable(n))
        .map(|n| {
            let mut c = Candidate::new(&global, FoundBy::SubNode);
            c.sub_prefix = vec![n.clone()];
            c
        })
        .collect();
    let skipped = names.len() - found.len();
    (found, skipped)
}

/// The quoted literal a map spells for the string `s`.
fn literal(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Puts the literal first subscripts `prefix` in front of a drafted index map's levels: the link
/// counts positions after the prefix is cut off, so the map the caller saves must start with it.
/// Levels move down by `prefix.len()`, and a guess about one of this map's levels
/// (`/maps/<name>/subscripts/<n>...`) follows its level.
pub fn prefix_levels(map: &mut serde_json::Value, guesses: &mut [String], prefix: &[String]) {
    let n = prefix.len();
    if n == 0 {
        return;
    }
    let name = map["name"].as_str().unwrap_or_default().to_string();
    let mut levels: Vec<serde_json::Value> = prefix
        .iter()
        .enumerate()
        .map(|(i, p)| serde_json::json!({"level": i + 1, "expression": literal(p)}))
        .collect();
    for mut s in map["subscripts"].as_array().cloned().unwrap_or_default() {
        if let Some(l) = s["level"].as_u64() {
            s["level"] = serde_json::json!(l + n as u64);
        }
        levels.push(s);
    }
    map["subscripts"] = serde_json::Value::Array(levels);
    let at = format!("/maps/{name}/subscripts/");
    for g in guesses.iter_mut() {
        let Some(rest) = g.strip_prefix(&at) else {
            continue;
        };
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(level) = digits.parse::<usize>() {
            *g = format!("{at}{}{}", level + n, &rest[digits.len()..]);
        }
    }
}

/// Marks a candidate `already_mapped`, with the map's name as the reason, when one of the class's
/// index maps already uses its global (and, for a sub-tree, its literal first subscripts). The
/// master's own data map is not an index map.
pub fn mark_already_mapped(candidates: &mut [Candidate], maps: &[super::model::SqlMap]) {
    let bare = |g: &str| g.strip_prefix('^').unwrap_or(g).to_string();
    for c in candidates
        .iter_mut()
        .filter(|c| c.status == Status::Examined)
    {
        let found = maps.iter().filter(|m| m.r#type == "index").find(|m| {
            bare(&m.global) == bare(&c.global)
                && c.sub_prefix.iter().enumerate().all(|(i, p)| {
                    m.subscripts
                        .get(i)
                        .is_some_and(|s| s.expression.trim() == literal(p))
                })
        });
        if let Some(m) = found {
            c.status = Status::AlreadyMapped;
            c.reason = Some(m.name.clone());
        }
    }
}

/// The global names from the listing's output. An `error` line is an error with IRIS's text, and
/// output that ends without its `done` line is an error, not a short list (FR-004).
pub fn parse_listing(out: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let mut done = false;
    for line in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let row: serde_json::Value = serde_json::from_str(line)
            .map_err(|_| format!("the listing wrote something unexpected: {line}"))?;
        if let Some(e) = row["error"].as_str() {
            return Err(e.to_string());
        }
        if row["done"] == 1 {
            done = true;
        } else if let Some(g) = row["g"].as_str() {
            names.push(g.to_string());
        }
    }
    if done {
        Ok(names)
    } else {
        Err("the listing stopped before it finished".into())
    }
}
