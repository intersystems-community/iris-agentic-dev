//! The usability line of `check` (spec 173): does the SQL planner use an index map for a query on
//! its field? Advisory, read from the planner's own `EXPLAIN` text, never from a rule of iad's.
//!
//! Measured on IRIS 2026.2 (`fact_6_planner_words`, plans in `tests/fixtures/storage173/captured`):
//! - a map whose subscript collation does not match the query's warns `Index map <X> cannot be used
//!   to satisfy the equal condition on <expr> because <reason>.` and the plan may still say `Read
//!   index map <X>, looping on ...` (a scan of the whole index), so the warning decides first;
//! - a map the planner uses says `Read index map <X>, using the given <expr>, and looping on ID.`

/// What the planner said about one index map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Usability {
    /// The planner reads the map with the given value (`map` is its full name, `Class.Map`).
    Used { map: String },
    /// The planner warned it cannot use the map; `text` is that warning line, as the planner wrote it.
    Unused { map: String, text: String },
    /// The plan says nothing about the map.
    Unknown,
}

/// The SQL table of a class: its package (dots to underscores) and class name, `A.B.C` is `A_B.C`.
pub fn sql_table(class: &str) -> Result<String, String> {
    crate::objectscript::validate_class_name(class).map_err(|e| e.to_string())?;
    let (package, name) = class
        .rsplit_once('.')
        .ok_or_else(|| format!("{class:?} has no package, so no SQL schema"))?;
    Ok(format!("{}.{name}", package.replace('.', "_")))
}

/// `EXPLAIN SELECT %ID FROM <table> WHERE <field> = 'x'`: the equal condition the planner judges an
/// index map by. The literal is replaced by a parameter in the plan, so its value never matters.
pub fn explain_query(class: &str, field: &str) -> Result<String, String> {
    let table = sql_table(class)?;
    if !is_field_name(field) {
        return Err(format!("{field:?} is not a field name"));
    }
    Ok(format!(
        "EXPLAIN SELECT %ID FROM {table} WHERE {field} = 'x'"
    ))
}

fn is_field_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '%')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `EXPLAIN SELECT %ID FROM <table> WHERE a = 'x' AND b = 'x'`: an equality on each of `fields`,
/// the question a map whose first subscripts are those fields answers.
pub fn explain_prefix_query(class: &str, fields: &[String]) -> Result<String, String> {
    let table = sql_table(class)?;
    if fields.is_empty() {
        return Err("no field to ask about".into());
    }
    if let Some(bad) = fields.iter().find(|f| !is_field_name(f)) {
        return Err(format!("{bad:?} is not a field name"));
    }
    let conditions: Vec<String> = fields.iter().map(|f| format!("{f} = 'x'")).collect();
    Ok(format!(
        "EXPLAIN SELECT %ID FROM {table} WHERE {}",
        conditions.join(" AND ")
    ))
}

/// The most questions `check` puts to the planner in one call; the rest are `unknown` and say so.
pub const MAX_QUESTIONS: usize = 20;

/// What one subscript expression of an index map names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubKind {
    /// One field, bare (`{City}`) or under a collation function (`$$SQLUPPER({Name},4)`).
    Field(String),
    /// The row ID, a row-ID part or a quoted constant: nothing to ask about.
    Skip,
    /// Anything else (the reason is the text), which no equality query on a field reaches.
    Other(String),
}

/// `Name` of `{Name}`, with no other brace in the text.
fn braced(s: &str) -> Option<&str> {
    let inner = s.strip_prefix('{')?.strip_suffix('}')?;
    (!inner.contains(['{', '}'])).then_some(inner)
}

pub fn subscript_kind(expr: &str) -> SubKind {
    let e = expr.trim();
    let not_field = || SubKind::Other(format!("{e:?} is not a field or a collation of one"));
    if e.len() >= 2 && e.starts_with('"') && e.ends_with('"') && !e[1..e.len() - 1].contains('"') {
        return SubKind::Skip;
    }
    // `$$FN({F})` or `$$FN({F},n)`: a collation function over one field.
    let inner = match e.strip_prefix("$$") {
        Some(call) => {
            let Some((name, rest)) = call.split_once('(') else {
                return not_field();
            };
            let Some(args) = rest.strip_suffix(')') else {
                return not_field();
            };
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
                return not_field();
            }
            let first = match args.split_once(',') {
                Some((a, n))
                    if !n.trim().is_empty() && n.trim().chars().all(|c| c.is_ascii_digit()) =>
                {
                    a
                }
                Some(_) => return not_field(),
                None => args,
            };
            first.trim()
        }
        None => e,
    };
    let Some(name) = braced(inner) else {
        return not_field();
    };
    if name.eq_ignore_ascii_case("ID") || name == "%ID" {
        return SubKind::Skip;
    }
    let row_part = name
        .strip_prefix('L')
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    if row_part {
        return SubKind::Skip;
    }
    if is_field_name(name) {
        SubKind::Field(name.to_string())
    } else {
        not_field()
    }
}

/// One index map as `check` sees it: its name, its `Type`, and its subscripts (level, expression).
#[derive(Debug, Clone)]
pub struct MapSubs {
    pub name: String,
    pub kind: String,
    pub subscripts: Vec<(u32, String)>,
}

/// A question for the planner: does an equality on `fields` use map `map`? `field` is the one at
/// `level`; `unanswerable` is why the planner is not asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub map: String,
    pub level: u32,
    pub field: String,
    pub fields: Vec<String>,
    pub unanswerable: Option<String>,
}

/// Every question about `maps`: one per field subscript of each index map, asking for the fields up
/// to it. The data map and extent maps are not asked; a bitmap map is one `unanswerable` question.
pub fn questions(maps: &[MapSubs]) -> Vec<Question> {
    let mut out = Vec::new();
    for m in maps {
        let kind = m.kind.to_ascii_lowercase();
        if kind == "bitmap" || kind == "bitslice" {
            let field = m
                .subscripts
                .iter()
                .find_map(|(_, e)| match subscript_kind(e) {
                    SubKind::Field(f) => Some(f),
                    _ => None,
                })
                .unwrap_or_default();
            out.push(Question {
                map: m.name.clone(),
                level: 1,
                field,
                fields: Vec::new(),
                unanswerable: Some(format!(
                    "a {kind} map is not asked: iad does not read the planner's words for bitmap access"
                )),
            });
            continue;
        }
        if kind != "index" {
            continue;
        }
        let mut fields: Vec<String> = Vec::new();
        // The first subscript that is neither a field nor skippable: nothing after it is reached.
        let mut blocked: Option<u32> = None;
        for (level, expr) in &m.subscripts {
            match subscript_kind(expr) {
                SubKind::Skip => {}
                SubKind::Other(why) => {
                    out.push(Question {
                        map: m.name.clone(),
                        level: *level,
                        field: String::new(),
                        fields: fields.clone(),
                        unanswerable: Some(format!("subscript {level} is not a field: {why}")),
                    });
                    blocked.get_or_insert(*level);
                }
                SubKind::Field(f) => {
                    if !fields.contains(&f) {
                        fields.push(f.clone());
                    }
                    let unanswerable = blocked.map(|b| {
                        format!(
                            "subscript {b} is not a field, so an equality on this field does not reach the map"
                        )
                    });
                    out.push(Question {
                        map: m.name.clone(),
                        level: *level,
                        field: f,
                        fields: fields.clone(),
                        unanswerable,
                    });
                }
            }
        }
    }
    out
}

/// One element of `usability`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub map: String,
    pub subscript_level: u32,
    pub field: String,
    pub status: &'static str,
    pub text: String,
}

impl Line {
    /// The line as a `warnings` string: which question, then the planner's words.
    pub fn warning(&self) -> String {
        let field = if self.field.is_empty() {
            String::new()
        } else {
            format!(" ({})", self.field)
        };
        format!(
            "index map {}, subscript {}{field}, {}: {}",
            self.map, self.subscript_level, self.status, self.text
        )
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "map": self.map,
            "subscript_level": self.subscript_level,
            "field": self.field,
            "status": self.status,
            "text": self.text,
        })
    }
}

/// The line for question `q` given the planner's answer: its plan text, or why it was not read.
/// The scratch copy's name in the planner's words (`scratch`) is the real class's (`class`).
pub fn line_for(q: &Question, plan: Result<&str, &str>, scratch: &str, class: &str) -> Line {
    let make = |status: &'static str, text: String| Line {
        map: q.map.clone(),
        subscript_level: q.level,
        field: q.field.clone(),
        status,
        text: text.replace(scratch, class),
    };
    if let Some(why) = &q.unanswerable {
        return make("unknown", why.clone());
    }
    let plan = match plan {
        Ok(p) => p,
        Err(why) => return make("unknown", format!("the planner could not be asked: {why}")),
    };
    match parse_plan(plan, &q.map) {
        Usability::Unused { text, .. } => make("unused", text),
        Usability::Used { map } => {
            let prefix = format!("{map},");
            let read = plan
                .lines()
                .map(str::trim)
                .find(|l| {
                    l.strip_prefix("Read index map ")
                        .is_some_and(|r| r.starts_with(&prefix))
                })
                .unwrap_or_default();
            make("used", read.to_string())
        }
        Usability::Unknown => make(
            "unknown",
            format!(
                "the plan for an equality on {} does not name index map {}: the planner read the rows another way",
                q.fields.join(" and "),
                q.map
            ),
        ),
    }
}

/// What `plan` (an `EXPLAIN` result) says about the map named `map` (its short name, `M`).
pub fn parse_plan(plan: &str, map: &str) -> Usability {
    let is_map = |full: &str| full.rsplit('.').next() == Some(map);
    let mut used = None;
    for line in plan.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("Index map ") {
            if let Some((full, _)) = rest.split_once(" cannot be used") {
                if is_map(full) {
                    return Usability::Unused {
                        map: full.to_string(),
                        text: line.to_string(),
                    };
                }
            }
        }
        if let Some(rest) = line.strip_prefix("Read index map ") {
            if let Some((full, tail)) = rest.split_once(',') {
                if is_map(full) && tail.trim_start().starts_with("using the given") {
                    used = Some(full.to_string());
                }
            }
        }
    }
    match used {
        Some(map) => Usability::Used { map },
        None => Usability::Unknown,
    }
}
