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
        && chars.all(|c| c.is_ascii_alphanumeric())
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
