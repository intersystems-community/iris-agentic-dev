//! Write lifecycle for SQL storage maps: the scratch copy now; the real PUT, read-back, compile
//! and restore later (plan "Write path").

use super::tree::{emit_body, split_class, Node};

/// Class keywords the map can depend on; every other keyword in the real header is dropped.
/// `SqlTableName` and `StorageStrategy` are written by [`scratch_source`] itself.
const KEPT_CLASS_KEYWORDS: [&str; 2] = ["SqlRowIdName", "SqlRowIdPrivate"];

/// Members whose body is a `{ … }` block closed by a `}` line; skipped whole.
const BRACED_MEMBERS: [&str; 7] = [
    "Method ",
    "ClassMethod ",
    "ClientMethod ",
    "Query ",
    "XData ",
    "Trigger ",
    "Storage ",
];

/// The scratch copy of `real_text` with Storage body `storage` (FR-015a): `Class <scratch>
/// Extends <same superclasses>`, the `Import` lines, the properties and indices the map names,
/// `SqlRowIdName`/`SqlRowIdPrivate` if the class has them, `SqlTableName` set to the scratch
/// name, `StorageStrategy` naming the block, and the emitted Storage block. No method, query,
/// XData, parameter, projection, trigger or include from the class text.
///
/// Not carried: a `Relationship` (its inverse would name the real class), so a child map's
/// scratch compile fails on the missing field.
pub fn scratch_source(
    real_text: &str,
    storage: &[Node],
    scratch_class: &str,
) -> Result<String, String> {
    if !scratch_class.starts_with("IrisDevTmp.")
        || crate::objectscript::validate_class_name(scratch_class).is_err()
    {
        return Err(format!(
            "{scratch_class} is not a class name in the IrisDevTmp package"
        ));
    }
    let sql_maps = sql_map_names(storage);
    if sql_maps.is_empty() {
        // FR-002: default storage is read-only here. IRIS also refuses a second class on its
        // globals (#5564, measured), so a copy could not compile anyway.
        return Err(
            "the Storage block has no SQLMap: default storage is read-only in \
                    iris_storage_map, and has no scratch copy"
                .into(),
        );
    }
    let lines: Vec<&str> = real_text.lines().collect();
    let Some(header_at) = lines.iter().position(|l| l.starts_with("Class ")) else {
        return Err("the class text has no Class line".into());
    };
    let span = split_class(real_text)
        .map_err(|e| format!("cannot split the class text: {}", e.reason))?
        .into_iter()
        .next()
        .ok_or("the class text has no Storage block")?;

    let (supers, keywords) = parse_header(lines[header_at]);
    let short = scratch_class.rsplit('.').next().unwrap_or(scratch_class);
    let mut kws: Vec<String> = keywords
        .into_iter()
        .filter(|k| KEPT_CLASS_KEYWORDS.contains(&keyword_name(k)))
        .collect();
    kws.push(format!("SqlTableName = {short}"));
    kws.push(format!("StorageStrategy = {}", span.name));
    kws.sort_by(|a, b| keyword_name(a).cmp(keyword_name(b)));

    let (properties, indices) = members(&lines[header_at + 1..]);
    let kept_indices: Vec<&Member> = indices
        .iter()
        .filter(|i| sql_maps.contains(&i.name.as_str()) || is_idkey(&i.text))
        .collect();
    let mut wanted = map_names(storage);
    for i in &kept_indices {
        wanted.extend(index_properties(&i.text));
    }
    let kept_properties = properties
        .iter()
        .filter(|p| wanted.iter().any(|w| w == &p.name));

    let mut out = String::new();
    for import in lines[..header_at]
        .iter()
        .filter(|l| l.starts_with("Import "))
    {
        out.push_str(import);
        out.push_str("\n\n");
    }
    out.push_str(&format!("Class {scratch_class}"));
    if let Some(s) = supers {
        out.push_str(&format!(" Extends {s}"));
    }
    out.push_str(&format!(" [ {} ]\n{{\n\n", kws.join(", ")));
    for m in kept_properties.chain(kept_indices) {
        out.push_str(&m.text);
        out.push_str("\n\n");
    }
    out.push_str(&format!(
        "Storage {}\n{{\n{}\n}}\n\n}}\n",
        span.name,
        emit_body(storage)
    ));
    Ok(out)
}

struct Member {
    name: String,
    text: String,
}

/// The superclass list (as written) and the keyword list of a `Class` line.
fn parse_header(line: &str) -> (Option<String>, Vec<String>) {
    let (head, kw_text) = match top_level_find(line, '[') {
        Some(at) => {
            let close = line.rfind(']').unwrap_or(line.len());
            (&line[..at], line.get(at + 1..close).unwrap_or(""))
        }
        None => (line, ""),
    };
    let supers = head
        .split_once(" Extends ")
        .map(|(_, s)| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let keywords = split_top_level(kw_text, ',')
        .into_iter()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();
    (supers, keywords)
}

/// `SqlTableName` of `SqlTableName = X`, `Final` of `Final`.
fn keyword_name(k: &str) -> &str {
    k.split(|c: char| c == '=' || c.is_whitespace())
        .next()
        .unwrap_or(k)
}

/// The class body's `Property` and `Index` declarations, each with its full text. A braced
/// member is skipped to its closing `}` line, so nothing inside a method or query is read as a
/// declaration.
fn members(body: &[&str]) -> (Vec<Member>, Vec<Member>) {
    let (mut properties, mut indices) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < body.len() {
        let line = body[i];
        if BRACED_MEMBERS.iter().any(|k| line.starts_with(k)) {
            i += 1;
            while i < body.len() && body[i] != "}" {
                i += 1;
            }
            i += 1;
            continue;
        }
        let into = if line.starts_with("Property ") {
            &mut properties
        } else if line.starts_with("Index ") {
            &mut indices
        } else {
            i += 1;
            continue;
        };
        let start = i;
        let mut depth: i32 = 0;
        loop {
            for c in body[i].chars() {
                match c {
                    '{' | '(' => depth += 1,
                    '}' | ')' => depth -= 1,
                    _ => {}
                }
            }
            if (depth <= 0 && body[i].trim_end().ends_with(';')) || i + 1 >= body.len() {
                break;
            }
            i += 1;
        }
        let text = body[start..=i].join("\n");
        let name = line
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .trim_end_matches(';')
            .to_string();
        into.push(Member { name, text });
        i += 1;
    }
    (properties, indices)
}

/// The field names a Storage tree refers to: `Data` element names, `Field` leaves, and every
/// `{Name}` in a leaf value.
fn map_names(nodes: &[Node]) -> Vec<String> {
    let mut out = Vec::new();
    walk(nodes, &mut |n| {
        if n.tag == "Data" {
            out.extend(n.name.clone());
        }
        if let Some(v) = &n.value {
            if n.tag == "Field" {
                out.push(v.trim().to_string());
            }
            let mut rest = v.as_str();
            while let Some(open) = rest.find('{') {
                let after = &rest[open + 1..];
                let Some(close) = after.find('}') else { break };
                out.push(after[..close].to_string());
                rest = &after[close + 1..];
            }
        }
    });
    out
}

/// The `name` of every top-level `SQLMap`.
fn sql_map_names(nodes: &[Node]) -> Vec<&str> {
    nodes
        .iter()
        .filter(|n| n.tag == "SQLMap")
        .filter_map(|n| n.name.as_deref())
        .collect()
}

fn walk<'a>(nodes: &'a [Node], f: &mut impl FnMut(&'a Node)) {
    for n in nodes {
        f(n);
        if let Some(c) = &n.children {
            walk(c, f);
        }
    }
}

/// The `IdKey` keyword in an `Index` declaration's `[ … ]`.
fn is_idkey(text: &str) -> bool {
    text.find('[').is_some_and(|at| {
        split_top_level(&text[at + 1..], ',')
            .iter()
            .any(|k| keyword_name(k.trim().trim_end_matches([']', ';', ' '])) == "IdKey")
    })
}

/// The properties of an `Index … On X` or `On (A, B As Exact)` declaration.
fn index_properties(text: &str) -> Vec<String> {
    let Some((_, after)) = text.split_once(" On ") else {
        return Vec::new();
    };
    let end = after.find(['[', ';']).unwrap_or(after.len());
    let list = after[..end].trim().trim_start_matches('(');
    let list = list.strip_suffix(')').unwrap_or(list);
    list.split(',')
        .filter_map(|p| p.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

/// The first `c` outside quotes, parentheses and braces.
fn top_level_find(s: &str, c: char) -> Option<usize> {
    let (mut depth, mut quoted) = (0i32, false);
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '(' | '{' if !quoted => depth += 1,
            ')' | '}' if !quoted => depth -= 1,
            _ if ch == c && !quoted && depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// `s` split on `sep` outside quotes, parentheses and braces.
fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(at) = top_level_find(rest, sep) {
        out.push(&rest[..at]);
        rest = &rest[at + sep.len_utf8()..];
    }
    out.push(rest);
    out
}
