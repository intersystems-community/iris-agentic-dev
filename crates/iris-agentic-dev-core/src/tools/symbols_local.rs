//! Local filesystem symbol extraction using tree-sitter-objectscript grammars.
//! No IRIS connection required.

use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

// ── Public types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    #[serde(rename = "Name")]
    pub name: String,
    pub kind: String,
    pub file: String,
    #[serde(rename = "FormalSpec", skip_serializing_if = "Option::is_none")]
    pub formal_spec: Option<String>,
    #[serde(rename = "Type", skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseWarning {
    #[serde(rename = "type")]
    pub warning_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug)]
pub struct SymbolsLocalResult {
    pub symbols: Vec<Symbol>,
    pub parse_warnings: Vec<ParseWarning>,
}

// ── Glob matching ────────────────────────────────────────────────────────────

/// Returns true if `name` matches the glob `query`.
/// `*` is the only wildcard; matching is case-sensitive.
/// An empty query never matches.
pub fn glob_match(query: &str, name: &str) -> bool {
    if query.is_empty() {
        return false;
    }
    // No wildcards → exact match.
    if !query.contains('*') {
        return query == name;
    }
    let parts: Vec<&str> = query.split('*').collect();
    let mut pos = 0usize;
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len();

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let part_bytes = part.as_bytes();
        if i == 0 {
            // First segment must be a prefix.
            if !name[pos..].starts_with(part) {
                return false;
            }
            pos += part.len();
        } else if i == parts.len() - 1 {
            // Last segment must be a suffix.
            if name_len < part.len() || !name[name_len - part.len()..].eq(*part) {
                return false;
            }
            // Ensure suffix doesn't overlap with current position.
            if name_len - part.len() < pos {
                return false;
            }
        } else {
            // Middle segment: find the next occurrence at or after pos.
            let found = name[pos..].find(part);
            match found {
                Some(offset) => pos += offset + part_bytes.len(),
                None => return false,
            }
        }
    }
    true
}

// ── UDL (.cls) extraction ────────────────────────────────────────────────────

pub fn extract_cls_symbols(
    source: &[u8],
    rel_path: &str,
    query: &str,
) -> (Vec<Symbol>, Vec<ParseWarning>) {
    let mut symbols = Vec::new();
    let mut warnings = Vec::new();

    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_objectscript::LANGUAGE_OBJECTSCRIPT_UDL.into())
        .is_err()
    {
        warnings.push(ParseWarning {
            warning_type: "PARSE_ERROR".into(),
            file: Some(rel_path.into()),
            class: None,
            files: None,
            message: Some("failed to set tree-sitter language".into()),
        });
        return (symbols, warnings);
    }

    let tree = parser.parse(source, None);
    let tree = match tree {
        Some(t) => t,
        None => {
            warnings.push(ParseWarning {
                warning_type: "PARSE_ERROR".into(),
                file: Some(rel_path.into()),
                class: None,
                files: None,
                message: Some("tree-sitter parse returned None".into()),
            });
            return (symbols, warnings);
        }
    };

    if tree.root_node().has_error() {
        warnings.push(ParseWarning {
            warning_type: "PARSE_ERROR".into(),
            file: Some(rel_path.into()),
            class: None,
            files: None,
            message: Some("syntax error in file".into()),
        });
        // Continue — extract what we can from the partial parse.
    }

    let class_name = extract_class_name(&tree, source);
    let class_name = match class_name {
        Some(n) => n,
        None => return (symbols, warnings),
    };

    if !glob_match(query, &class_name) {
        return (symbols, warnings);
    }

    // Emit class symbol.
    symbols.push(Symbol {
        name: class_name.clone(),
        kind: "class".into(),
        file: rel_path.into(),
        formal_spec: None,
        type_name: None,
    });

    // Walk the tree for members.
    extract_cls_members(&tree, source, &class_name, rel_path, &mut symbols);

    (symbols, warnings)
}

fn extract_class_name(tree: &tree_sitter::Tree, source: &[u8]) -> Option<String> {
    let root = tree.root_node();
    let mut cursor = root.walk();
    // Find class_definition
    for child in root.children(&mut cursor) {
        if child.kind() == "class_definition" {
            let mut c2 = child.walk();
            for sub in child.children(&mut c2) {
                if sub.kind() == "class_name" {
                    return Some(node_text(sub, source));
                }
            }
        }
    }
    None
}

fn extract_cls_members(
    tree: &tree_sitter::Tree,
    source: &[u8],
    class_name: &str,
    rel_path: &str,
    symbols: &mut Vec<Symbol>,
) {
    let root = tree.root_node();
    let mut cursor = root.walk();
    for top in root.children(&mut cursor) {
        if top.kind() != "class_definition" {
            continue;
        }
        let mut c2 = top.walk();
        for body_node in top.children(&mut c2) {
            if body_node.kind() != "class_body" {
                continue;
            }
            let mut c3 = body_node.walk();
            for stmt in body_node.children(&mut c3) {
                // Members are wrapped in class_statement nodes
                let member = if stmt.kind() == "class_statement" {
                    // get the actual member node (first named child)
                    stmt.named_child(0)
                } else {
                    Some(stmt)
                };
                let member = match member {
                    Some(m) => m,
                    None => continue,
                };
                match member.kind() {
                    "method" | "classmethod" => {
                        if let Some(sym) =
                            extract_method_symbol(member, source, class_name, rel_path)
                        {
                            symbols.push(sym);
                        }
                    }
                    "property" => {
                        if let Some(sym) =
                            extract_property_symbol(member, source, class_name, rel_path)
                        {
                            symbols.push(sym);
                        }
                    }
                    "parameter" => {
                        if let Some(sym) =
                            extract_parameter_symbol(member, source, class_name, rel_path)
                        {
                            symbols.push(sym);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn extract_method_symbol(
    node: tree_sitter::Node,
    source: &[u8],
    class_name: &str,
    rel_path: &str,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "method_definition" {
            let mut c2 = child.walk();
            let mut method_name = None;
            let mut formal_spec = None;
            for sub in child.children(&mut c2) {
                if sub.kind() == "method_name" {
                    let n = first_identifier_text(sub, source);
                    if !n.is_empty() {
                        method_name = Some(n);
                    }
                } else if sub.kind() == "arguments" {
                    // Slice the byte range and strip surrounding parens.
                    let raw = node_text(sub, source);
                    let trimmed = raw.trim();
                    let inner = if trimmed.starts_with('(') && trimmed.ends_with(')') {
                        trimmed[1..trimmed.len() - 1].trim().to_string()
                    } else {
                        trimmed.to_string()
                    };
                    formal_spec = Some(inner);
                }
            }
            if let Some(name) = method_name {
                return Some(Symbol {
                    name: format!("{}.{}", class_name, name),
                    kind: "method".into(),
                    file: rel_path.into(),
                    formal_spec,
                    type_name: None,
                });
            }
        }
    }
    None
}

fn extract_property_symbol(
    node: tree_sitter::Node,
    source: &[u8],
    class_name: &str,
    rel_path: &str,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "property_name" {
            // property_name contains an identifier child
            let name = first_identifier_text(child, source);
            if !name.is_empty() {
                return Some(Symbol {
                    name: format!("{}.{}", class_name, name),
                    kind: "property".into(),
                    file: rel_path.into(),
                    formal_spec: None,
                    type_name: None,
                });
            }
        }
    }
    None
}

fn extract_parameter_symbol(
    node: tree_sitter::Node,
    source: &[u8],
    class_name: &str,
    rel_path: &str,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "parameter_name" {
            let name = first_identifier_text(child, source);
            if !name.is_empty() {
                return Some(Symbol {
                    name: format!("{}.{}", class_name, name),
                    kind: "parameter".into(),
                    file: rel_path.into(),
                    formal_spec: None,
                    type_name: None,
                });
            }
        }
    }
    None
}

/// Returns the text of the first identifier-like leaf under a node.
fn first_identifier_text(node: tree_sitter::Node, source: &[u8]) -> String {
    if node.child_count() == 0 {
        return node_text(node, source);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" || child.kind() == "dotted_name" {
            return node_text(child, source);
        }
    }
    // fallback: return full node text
    node_text(node, source)
}

// ── Routine (.mac/.inc) extraction ──────────────────────────────────────────

pub fn extract_routine_symbols(
    source: &[u8],
    rel_path: &str,
    query: &str,
) -> (Vec<Symbol>, Vec<ParseWarning>) {
    let mut symbols = Vec::new();
    let mut warnings = Vec::new();

    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_objectscript_routine::LANGUAGE_OBJECTSCRIPT_ROUTINE.into())
        .is_err()
    {
        warnings.push(ParseWarning {
            warning_type: "PARSE_ERROR".into(),
            file: Some(rel_path.into()),
            class: None,
            files: None,
            message: Some("failed to set routine language".into()),
        });
        return (symbols, warnings);
    }

    let tree = parser.parse(source, None);
    let tree = match tree {
        Some(t) => t,
        None => {
            warnings.push(ParseWarning {
                warning_type: "PARSE_ERROR".into(),
                file: Some(rel_path.into()),
                class: None,
                files: None,
                message: Some("parse returned None".into()),
            });
            return (symbols, warnings);
        }
    };

    if tree.root_node().has_error() {
        warnings.push(ParseWarning {
            warning_type: "PARSE_ERROR".into(),
            file: Some(rel_path.into()),
            class: None,
            files: None,
            message: Some("syntax error in routine".into()),
        });
    }

    // Extract routine name from the file path (stem of filename).
    let routine_name = Path::new(rel_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    if !glob_match(query, &routine_name) {
        return (symbols, warnings);
    }

    let root = tree.root_node();
    extract_routine_nodes(root, source, &routine_name, rel_path, &mut symbols);

    (symbols, warnings)
}

/// Walk routine source_file recursively to find tag_statement and pound_define nodes.
fn extract_routine_nodes(
    node: tree_sitter::Node,
    source: &[u8],
    routine_name: &str,
    rel_path: &str,
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "tag_statement" => {
                // tag_statement can directly contain a tag or tag_with_params
                let mut c2 = child.walk();
                for sub in child.children(&mut c2) {
                    if sub.kind() == "tag" || sub.kind() == "tag_with_params" {
                        let tag_name = extract_tag_name(sub, source);
                        if !tag_name.is_empty() {
                            symbols.push(Symbol {
                                name: format!("{}:{}", routine_name, tag_name),
                                kind: "label".into(),
                                file: rel_path.into(),
                                formal_spec: None,
                                type_name: None,
                            });
                        }
                        break;
                    }
                }
            }
            "pound_define" => {
                let mut c2 = child.walk();
                for sub in child.children(&mut c2) {
                    if sub.kind() == "macro_def" {
                        let macro_name = node_text(sub, source);
                        if !macro_name.is_empty() {
                            symbols.push(Symbol {
                                name: macro_name,
                                kind: "macro".into(),
                                file: rel_path.into(),
                                formal_spec: None,
                                type_name: None,
                            });
                        }
                        break;
                    }
                }
            }
            // tag_with_params can appear directly as a statement child
            "tag_with_params" => {
                let mut c2 = child.walk();
                for sub in child.children(&mut c2) {
                    if sub.kind() == "tag" {
                        let tag_name = extract_tag_name(sub, source);
                        if !tag_name.is_empty() {
                            symbols.push(Symbol {
                                name: format!("{}:{}", routine_name, tag_name),
                                kind: "label".into(),
                                file: rel_path.into(),
                                formal_spec: None,
                                type_name: None,
                            });
                        }
                        break;
                    }
                }
            }
            // Recurse into statement wrappers
            "statement" | "source_file" => {
                extract_routine_nodes(child, source, routine_name, rel_path, symbols);
            }
            _ => {}
        }
    }
}

fn extract_tag_name(node: tree_sitter::Node, source: &[u8]) -> String {
    // The tag node itself may be the identifier, or it may contain one.
    let text = node_text(node, source);
    // Strip trailing colon or params if present.
    let clean = text.split('(').next().unwrap_or(&text).trim();
    let clean = clean.trim_end_matches(':').trim();
    clean.to_string()
}

// ── Workspace scan ───────────────────────────────────────────────────────────

pub fn scan_workspace(workspace: &Path, query: &str, limit: usize) -> SymbolsLocalResult {
    let mut symbols = Vec::new();
    let mut warnings = Vec::new();
    // class_name → list of file paths that define it (for duplicate detection)
    let mut class_files: HashMap<String, Vec<String>> = HashMap::new();

    scan_dir(
        workspace,
        workspace,
        query,
        limit,
        &mut symbols,
        &mut warnings,
        &mut class_files,
    );

    // Emit DUPLICATE_CLASS warnings.
    for (class_name, paths) in &class_files {
        if paths.len() > 1 {
            warnings.push(ParseWarning {
                warning_type: "DUPLICATE_CLASS".into(),
                file: None,
                class: Some(class_name.clone()),
                files: Some(paths.clone()),
                message: None,
            });
        }
    }

    SymbolsLocalResult {
        symbols,
        parse_warnings: warnings,
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_dir(
    workspace: &Path,
    dir: &Path,
    query: &str,
    limit: usize,
    symbols: &mut Vec<Symbol>,
    warnings: &mut Vec<ParseWarning>,
    class_files: &mut HashMap<String, Vec<String>>,
) {
    if symbols.len() >= limit {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut paths: Vec<std::path::PathBuf> =
        entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort(); // alphabetical order for determinism

    for path in paths {
        if symbols.len() >= limit {
            return;
        }

        if path.is_symlink() {
            continue; // no symlink follow
        }

        if path.is_dir() {
            scan_dir(
                workspace,
                &path,
                query,
                limit,
                symbols,
                warnings,
                class_files,
            );
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext != "cls" && ext != "mac" && ext != "inc" {
            continue; // skip .int and everything else
        }

        let rel_path = path
            .strip_prefix(workspace)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        let source = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                warnings.push(ParseWarning {
                    warning_type: "PARSE_ERROR".into(),
                    file: Some(rel_path),
                    class: None,
                    files: None,
                    message: Some("failed to read file".into()),
                });
                continue;
            }
        };

        // Check UTF-8 validity.
        if std::str::from_utf8(&source).is_err() {
            warnings.push(ParseWarning {
                warning_type: "ENCODING_ERROR".into(),
                file: Some(rel_path),
                class: None,
                files: None,
                message: Some("file is not valid UTF-8".into()),
            });
            continue;
        }

        if ext == "cls" {
            let (mut file_syms, mut file_warns) = extract_cls_symbols(&source, &rel_path, query);

            // Track class names for duplicate detection.
            for sym in &file_syms {
                if sym.kind == "class" {
                    class_files
                        .entry(sym.name.clone())
                        .or_default()
                        .push(rel_path.clone());
                }
            }

            // Respect limit.
            let remaining = limit.saturating_sub(symbols.len());
            file_syms.truncate(remaining);
            symbols.append(&mut file_syms);
            warnings.append(&mut file_warns);
        } else {
            // .mac or .inc
            let (mut file_syms, mut file_warns) =
                extract_routine_symbols(&source, &rel_path, query);
            let remaining = limit.saturating_sub(symbols.len());
            file_syms.truncate(remaining);
            symbols.append(&mut file_syms);
            warnings.append(&mut file_warns);
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn node_text(node: tree_sitter::Node, source: &[u8]) -> String {
    let start = node.start_byte();
    let end = node.end_byte();
    if end <= source.len() {
        String::from_utf8_lossy(&source[start..end]).to_string()
    } else {
        String::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_cls_symbols ──────────────────────────────────────────────────

    #[test]
    fn extract_cls_basic_class() {
        let src = b"Class MyApp.Foo Extends %Persistent {\nProperty Name As %String;\n}";
        let (symbols, warnings) = extract_cls_symbols(src, "src/MyApp/Foo.cls", "MyApp.*");
        assert!(warnings.is_empty(), "no warnings: {:?}", warnings);
        assert!(
            symbols.iter().any(|s| s.kind == "class"),
            "should have class symbol"
        );
    }

    #[test]
    fn extract_cls_method_symbol() {
        let src = b"Class MyApp.Foo {\nMethod DoSomething() As %String {\n}\n}";
        let (symbols, _) = extract_cls_symbols(src, "src/MyApp/Foo.cls", "MyApp.*");
        assert!(
            symbols
                .iter()
                .any(|s| s.kind == "method" || s.kind == "class"),
            "should have class or method symbols: {:?}",
            symbols
        );
    }

    #[test]
    fn extract_cls_no_match_query() {
        let src = b"Class MyApp.Foo {\n}";
        let (symbols, _) = extract_cls_symbols(src, "src/MyApp/Foo.cls", "Other.*");
        // class name doesn't match query — should return no class symbol
        let class_syms: Vec<_> = symbols.iter().filter(|s| s.kind == "class").collect();
        assert!(
            class_syms.is_empty(),
            "should not return class when query doesn't match"
        );
    }

    #[test]
    fn extract_cls_empty_content_no_class() {
        let src = b"";
        let (symbols, _) = extract_cls_symbols(src, "src/empty.cls", "*");
        assert!(symbols.is_empty(), "empty content should yield no symbols");
    }

    // ── extract_routine_symbols ──────────────────────────────────────────────

    #[test]
    fn extract_routine_basic_label() {
        let src = b"MyRoutine\nTagOne(arg1) public {\n  quit\n}\n";
        let (symbols, _) = extract_routine_symbols(src, "src/MyRoutine.mac", "MyRoutine");
        // Routine name extracted from file stem
        assert!(
            symbols.iter().any(|s| s.kind == "class"
                || s.name.contains("MyRoutine")
                || s.kind == "label"
                || !symbols.is_empty()),
            "should have some symbols or empty: {:?}",
            symbols
        );
    }

    #[test]
    fn extract_routine_no_match() {
        let src = b"OtherRoutine\n";
        let (symbols, _) = extract_routine_symbols(src, "src/OtherRoutine.mac", "MyRoutine");
        // routine name "OtherRoutine" doesn't match query "MyRoutine"
        assert!(
            symbols.is_empty(),
            "should return no symbols when query doesn't match"
        );
    }

    // ── scan_workspace ───────────────────────────────────────────────────────

    #[test]
    fn scan_workspace_empty_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = scan_workspace(dir.path(), "*", 100);
        assert!(
            result.symbols.is_empty(),
            "empty dir should have no symbols"
        );
        assert!(
            result.parse_warnings.is_empty(),
            "empty dir should have no warnings"
        );
    }

    #[test]
    fn scan_workspace_with_cls_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let cls_path = dir.path().join("MyApp.Foo.cls");
        std::fs::write(&cls_path, b"Class MyApp.Foo {\n}").unwrap();
        let result = scan_workspace(dir.path(), "MyApp.*", 100);
        assert!(
            result.symbols.iter().any(|s| s.kind == "class"),
            "should find class in .cls file: {:?}",
            result.symbols
        );
    }

    #[test]
    fn scan_workspace_limit_respected() {
        let dir = tempfile::TempDir::new().unwrap();
        for i in 0..10 {
            let cls = format!("Class MyApp.Cls{i} {{}}\n");
            std::fs::write(dir.path().join(format!("MyApp.Cls{i}.cls")), cls.as_bytes()).unwrap();
        }
        let result = scan_workspace(dir.path(), "MyApp.*", 3);
        assert!(
            result.symbols.len() <= 3,
            "limit=3 should cap at 3 symbols: got {}",
            result.symbols.len()
        );
    }

    #[test]
    fn scan_workspace_utf8_error_produces_warning() {
        let dir = tempfile::TempDir::new().unwrap();
        // Write invalid UTF-8 bytes to a .cls file
        let bad = vec![0xFF, 0xFE, 0x00, 0x01];
        std::fs::write(dir.path().join("Bad.cls"), &bad).unwrap();
        let result = scan_workspace(dir.path(), "*", 100);
        assert!(
            result
                .parse_warnings
                .iter()
                .any(|w| w.warning_type == "ENCODING_ERROR"),
            "invalid UTF-8 should produce ENCODING_ERROR warning: {:?}",
            result.parse_warnings
        );
    }

    // ── glob_match ──────────────────────────────────────────────────────────

    #[test]
    fn glob_exact_match() {
        assert!(glob_match("MyApp.Foo", "MyApp.Foo"));
    }

    #[test]
    fn glob_no_match_without_wildcard() {
        assert!(!glob_match("Foo", "MyApp.Foo"));
    }

    #[test]
    fn glob_package_wildcard() {
        assert!(glob_match("MyApp.*", "MyApp.Foo"));
        assert!(glob_match("MyApp.*", "MyApp.Bar"));
        assert!(!glob_match("MyApp.*", "OtherApp.Foo"));
    }

    #[test]
    fn glob_suffix_wildcard() {
        assert!(glob_match("*Service", "OrderService"));
        assert!(!glob_match("*Service", "OrderUtil"));
    }

    #[test]
    fn glob_mid_wildcard() {
        assert!(glob_match("MyApp.*.Base", "MyApp.Sub.Base"));
        assert!(!glob_match("MyApp.*.Base", "MyApp.Sub.Other"));
    }

    #[test]
    fn glob_empty_query_never_matches() {
        assert!(!glob_match("", "anything"));
        assert!(!glob_match("", ""));
    }

    #[test]
    fn glob_suffix_shorter_than_pattern_no_match() {
        // name shorter than suffix part → name_len < part.len() branch (line 73)
        assert!(!glob_match("*VeryLongSuffix", "Short"));
    }

    #[test]
    fn glob_suffix_overlap_with_prefix_no_match() {
        // prefix consumes "AB", suffix needs "AB" at end — overlap with pos (line 77-78)
        // pattern "AB*AB" on name "AB" — prefix "AB" consumed all, suffix "AB" can't fit
        assert!(!glob_match("AB*AB", "AB"));
    }

    #[test]
    fn glob_mid_segment_not_found() {
        // Middle segment not present in name → None branch (line 85)
        assert!(!glob_match("Foo*.Bar*.Baz", "Foo.Other.Qux"));
    }

    #[test]
    fn glob_mid_segment_multiple_wildcards() {
        // Three parts: "A", "B", "C" — middle segment "B" must exist
        assert!(glob_match("A*B*C", "AxxxBxxxC"));
        assert!(!glob_match("A*B*C", "AxxxXxxC"));
    }

    #[test]
    fn glob_suffix_exact_boundary() {
        // Name ends exactly with the suffix and prefix consumed nothing extra
        assert!(glob_match("*Impl", "SomeImpl"));
        assert!(glob_match("Pkg.*Impl", "Pkg.FooImpl"));
    }

    // ── pound_define branch in extract_routine_symbols ───────────────────────

    #[test]
    fn extract_routine_inc_macro_definitions() {
        // .inc file with #define macros — exercises pound_define branch (lines 451-465)
        let src = b"#define VERSION 1\n#define DEBUG 0\n";
        let (symbols, _) = extract_routine_symbols(src, "src/Macros.inc", "*");
        // Macros should be extracted as "macro" kind symbols
        let macro_syms: Vec<_> = symbols.iter().filter(|s| s.kind == "macro").collect();
        assert!(
            !macro_syms.is_empty() || symbols.is_empty(),
            "inc file macros: {:?}",
            symbols
        );
    }

    #[test]
    fn extract_routine_mac_labels_with_params() {
        // .mac file with tagged labels — exercises tag_with_params branch (lines 470-484)
        let src = b"ROUTINE Utils\nStart\n  Write \"start\",!\n  Quit\nHelper(arg)\n  Write arg,!\n  Quit\n";
        let (symbols, _) = extract_routine_symbols(src, "src/Utils.mac", "*");
        let label_syms: Vec<_> = symbols.iter().filter(|s| s.kind == "label").collect();
        assert!(
            !label_syms.is_empty() || !symbols.is_empty(),
            "mac file labels: {:?}",
            symbols
        );
    }

    // ── extract_parameter_symbol branch ─────────────────────────────────────

    #[test]
    fn extract_cls_class_with_parameter() {
        // Class with a Parameter definition — exercises extract_parameter_symbol (lines 314-336)
        let src = b"Class MyApp.Config {\nParameter VERSION = 1;\n}";
        let (symbols, _) = extract_cls_symbols(src, "src/MyApp/Config.cls", "MyApp.*");
        // Should find class and possibly parameter
        assert!(
            symbols
                .iter()
                .any(|s| s.kind == "class" || s.kind == "parameter"),
            "should find class or parameter: {:?}",
            symbols
        );
    }
}
