//! Lossless element tree for IRIS Storage blocks.
//!
//! The parser builds a tree of elements from the Storage body text, and the emitter writes it back
//! in canonical form (research R3). Every edit is applied to this tree, so unknown elements are
//! never lost.

use std::cmp::Ordering;

/// A parsed XML-like element from a Storage block.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub tag: String,
    pub name: Option<String>,
    pub value: Option<String>,
    pub children: Option<Vec<Node>>,
}

impl Node {
    pub fn is_leaf(&self) -> bool {
        self.children.is_none()
    }
}

/// Returns true if the string is a canonical number per IRIS $Order collation.
///
/// Canonical numbers:
/// - `0` (special case)
/// - Integers: `1`, `-1`, `123` (no leading zeros except `0` itself)
/// - Decimals: `1.5`, `.5`, `123.456` (no trailing zeros after decimal point)
///
/// NOT canonical: `01`, `1.0`, `1.50`, `00`, etc.
pub fn is_canonical_number(s: &str) -> bool {
    // Regex: ^-?(?:(?:[1-9]\d*)(?:\.\d*[1-9])?|\.\d*[1-9])$|^0$
    // Meaning:
    // - Optional minus sign
    // - Either:
    //   - Non-zero integer ([1-9]\d*) optionally followed by decimal with non-zero trailing digits
    //   - Or decimal starting with . followed by digits ending in non-zero
    // - Or exactly "0"

    if s == "0" {
        return true;
    }

    if s.is_empty() {
        return false;
    }

    let bytes = s.as_bytes();
    let mut i = 0;

    // Optional minus
    if bytes[i] == b'-' {
        i += 1;
        if i >= bytes.len() {
            return false;
        }
    }

    // Must start with digit or decimal point
    if bytes[i] == b'.' {
        // Decimal starting with . (like .5)
        i += 1;
        if i >= bytes.len() {
            return false;
        }
        // Must have at least one digit
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return false; // No digits after .
        }
        // Must not end with 0
        if bytes[i - 1] == b'0' {
            return false;
        }
        // Must be at end
        return i == bytes.len();
    }

    // Integer part: first digit must be 1-9 (we already handled "0" at the start)
    if !(bytes[i] >= b'1' && bytes[i] <= b'9') {
        return false;
    }

    i += 1;

    // Rest of integer digits
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }

    if i == bytes.len() {
        // Pure integer, we're done
        return true;
    }

    // Must have decimal point
    if bytes[i] != b'.' {
        return false;
    }

    i += 1;
    if i >= bytes.len() {
        return false; // Ends with . (like "1.")
    }

    // Decimal part
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }

    if i == start {
        return false; // No digits after decimal
    }

    // Must not end with 0
    if bytes[i - 1] == b'0' {
        return false;
    }

    // Must be at end
    i == bytes.len()
}

/// Key for $Order collation: none < canonical numbers (numerically) < strings (by code point).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrderKey {
    None,
    Number(OrderedFloat),
    String(String),
}

/// Wrapper for f64 that implements Ord by comparing byte representations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderedFloat(String); // Store as string for exact decimal comparison

impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> Ordering {
        // Parse both as f64 for numeric comparison
        let a: f64 = self.0.parse().unwrap_or(0.0);
        let b: f64 = other.0.parse().unwrap_or(0.0);

        match a.partial_cmp(&b) {
            Some(ord) => ord,
            None => Ordering::Equal,
        }
    }
}

/// Returns the $Order collation key for a name attribute value.
pub fn order_key(name: Option<&str>) -> OrderKey {
    match name {
        None => OrderKey::None,
        Some(s) => {
            if is_canonical_number(s) {
                OrderKey::Number(OrderedFloat(s.to_string()))
            } else {
                OrderKey::String(s.to_string())
            }
        }
    }
}

/// Sort key for a node: (tag case-insensitive, $Order collation of name).
pub fn sort_key(n: &Node) -> (String, OrderKey) {
    (n.tag.to_lowercase(), order_key(n.name.as_deref()))
}

/// Parse error with position and context.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub offset: usize,
    pub context: String,
    pub reason: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Parse error at offset {}: {} (context: {})",
            self.offset, self.reason, self.context
        )
    }
}

impl std::error::Error for ParseError {}

const MAX_BODY_BYTES: usize = 1024 * 1024; // 1 MiB
const MAX_DEPTH: usize = 32;

fn parse_error(offset: usize, text: &str, reason: String) -> ParseError {
    ParseError {
        offset,
        context: context_snippet(text, offset),
        reason,
    }
}

/// Parses the Storage body text (between `{` and `}`) into a tree of nodes.
///
/// Uses iterative parsing with an explicit stack to avoid recursion.
/// Enforces MAX_BODY_BYTES and MAX_DEPTH limits.
pub fn parse_body(text: &str) -> Result<Vec<Node>, ParseError> {
    if text.len() > MAX_BODY_BYTES {
        return Err(ParseError {
            offset: text.len(),
            context: "body size".to_string(),
            reason: format!("body exceeds {} bytes", MAX_BODY_BYTES),
        });
    }

    let mut nodes = Vec::new();
    let mut pos = 0;

    while pos < text.len() {
        let (node, new_pos) = parse_element(text, pos, 0)?;
        nodes.push(node);
        pos = new_pos;

        if pos < text.len() {
            if text.as_bytes()[pos] != b'\n' {
                return Err(parse_error(
                    pos,
                    text,
                    "expected newline between elements".to_string(),
                ));
            }
            pos += 1;
        }
    }

    Ok(nodes)
}

/// Returns a snippet of text around the given position for error context.
fn context_snippet(text: &str, pos: usize) -> String {
    let start = pos.saturating_sub(20);
    let end = (pos + 40).min(text.len());
    format!("{:?}", &text[start..end])
}

/// Parses a single element starting at `pos`, with current depth for MAX_DEPTH check.
fn parse_element(text: &str, pos: usize, depth: usize) -> Result<(Node, usize), ParseError> {
    if depth >= MAX_DEPTH {
        return Err(ParseError {
            offset: pos,
            context: "nesting depth".to_string(),
            reason: format!("depth exceeds {}", MAX_DEPTH),
        });
    }

    // Parse opening tag: <Tag name="value" />  or <Tag name="value">
    let (tag, name, self_close, pos) = parse_opening_tag(text, pos)?;

    if self_close {
        // Self-closing tag like <X/>
        return Ok((
            Node {
                tag,
                name,
                value: None,
                children: Some(Vec::new()),
            },
            pos,
        ));
    }

    let close_tag = format!("</{}>", tag);

    // Find next '<' to determine if this is a leaf, CDATA, or container
    let next_open = match text[pos..].find('<') {
        Some(offset) => pos + offset,
        None => return Err(parse_error(pos, text, "unterminated element".to_string())),
    };

    // Check if it's the closing tag (empty leaf)
    if text[next_open..].starts_with(&close_tag) {
        let value_text = &text[pos..next_open];
        return Ok((
            Node {
                tag,
                name,
                value: if value_text.is_empty() {
                    Some("".to_string())
                } else {
                    Some(value_text.to_string())
                },
                children: None,
            },
            next_open + close_tag.len(),
        ));
    }

    // Check for CDATA
    if text[next_open..].starts_with("<![CDATA[") {
        let _pre = &text[pos..next_open];
        let mut value_parts = Vec::new();
        let mut p = next_open;

        while text[p..].starts_with("<![CDATA[") {
            let cdata_start = p + 9;
            match text[cdata_start..].find("]]>") {
                Some(offset) => {
                    value_parts.push(&text[cdata_start..cdata_start + offset]);
                    p = cdata_start + offset + 3;
                }
                None => return Err(parse_error(p, text, "unterminated CDATA".to_string())),
            }
        }

        if !text[p..].starts_with(&close_tag) {
            return Err(parse_error(
                p,
                text,
                "CDATA not followed by close tag".to_string(),
            ));
        }

        return Ok((
            Node {
                tag,
                name,
                value: Some(value_parts.join("")),
                children: None,
            },
            p + close_tag.len(),
        ));
    }

    // Container element
    if &text[pos..next_open] != "\n" {
        return Err(parse_error(
            pos,
            text,
            format!("container {} has text before first child", tag),
        ));
    }

    let mut children = Vec::new();
    let mut p = next_open;

    loop {
        if text[p..].starts_with(&close_tag) {
            break;
        }
        // A close tag that is not this element's (`</SQLMap` with no `>`, or another tag's) is
        // reported as such, not as whatever the next character would make of it.
        if text[p..].starts_with("</") {
            return Err(parse_error(
                p,
                text,
                format!("unterminated or mismatched close tag: expected {close_tag}"),
            ));
        }

        let (child, new_p) = parse_element(text, p, depth + 1)?;
        children.push(child);
        p = new_p;

        if p >= text.len() || text.as_bytes()[p] != b'\n' {
            return Err(parse_error(
                p,
                text,
                format!("expected newline after child in {}", tag),
            ));
        }
        p += 1;
    }

    Ok((
        Node {
            tag,
            name,
            value: None,
            children: Some(children),
        },
        p + close_tag.len(),
    ))
}

/// Parses an opening tag and returns (tag, name_attr, self_close, pos_after).
fn parse_opening_tag(
    text: &str,
    pos: usize,
) -> Result<(String, Option<String>, bool, usize), ParseError> {
    if !text[pos..].starts_with('<') {
        return Err(parse_error(pos, text, "expected opening tag".to_string()));
    }

    let mut p = pos + 1;

    // Parse tag name
    let tag_start = p;
    while p < text.len() && text.as_bytes()[p].is_ascii_alphanumeric() {
        p += 1;
    }

    if p == tag_start {
        return Err(parse_error(pos, text, "empty tag name".to_string()));
    }

    let tag = text[tag_start..p].to_string();

    // Skip whitespace
    while p < text.len() && (text.as_bytes()[p] == b' ' || text.as_bytes()[p] == b'\t') {
        p += 1;
    }

    let mut name_attr = None;
    let mut attr_count = 0;

    // Parse attributes
    while p < text.len() && text.as_bytes()[p] != b'>' && text.as_bytes()[p] != b'/' {
        // Parse attribute: name="value"
        let attr_start = p;
        while p < text.len() && text.as_bytes()[p].is_ascii_alphanumeric() {
            p += 1;
        }

        if p == attr_start {
            return Err(parse_error(p, text, "expected attribute name".to_string()));
        }

        let attr_name = &text[attr_start..p];

        if p >= text.len() || text.as_bytes()[p] != b'=' {
            return Err(parse_error(
                p,
                text,
                "expected = after attribute name".to_string(),
            ));
        }
        p += 1;

        if p >= text.len() || text.as_bytes()[p] != b'"' {
            return Err(parse_error(p, text, "expected \" after =".to_string()));
        }
        p += 1;

        let value_start = p;
        while p < text.len() && text.as_bytes()[p] != b'"' {
            p += 1;
        }

        if p >= text.len() {
            return Err(parse_error(
                p,
                text,
                "unterminated attribute value".to_string(),
            ));
        }

        let value = text[value_start..p].to_string();
        p += 1;

        attr_count += 1;

        if attr_name == "name" {
            name_attr = Some(value);
        } else {
            return Err(parse_error(
                attr_start,
                text,
                format!("unknown attribute {} on {}", attr_name, tag),
            ));
        }

        // Skip whitespace
        while p < text.len() && (text.as_bytes()[p] == b' ' || text.as_bytes()[p] == b'\t') {
            p += 1;
        }
    }

    if attr_count > 1 {
        return Err(parse_error(
            pos,
            text,
            "multiple attributes not allowed".to_string(),
        ));
    }

    // Check for self-closing
    let self_close = p < text.len() && text.as_bytes()[p] == b'/';
    if self_close {
        p += 1;
    }

    if p >= text.len() || text.as_bytes()[p] != b'>' {
        return Err(parse_error(p, text, "expected > to close tag".to_string()));
    }
    p += 1;

    Ok((tag, name_attr, self_close, p))
}

/// Emits nodes as canonical XML text.
pub fn emit_body(nodes: &[Node]) -> String {
    let mut lines = Vec::new();
    emit_nodes(nodes, &mut lines, true);
    lines.join("\n")
}

/// Recursively emits nodes to a list of lines.
fn emit_nodes(nodes: &[Node], lines: &mut Vec<String>, sort: bool) {
    let mut sorted_nodes: Vec<&Node> = nodes.iter().collect();
    if sort {
        sorted_nodes.sort_by_key(|&a| sort_key(a));
    }

    for node in sorted_nodes {
        let attr = match &node.name {
            Some(n) => format!(" name=\"{}\"", n),
            None => String::new(),
        };

        if node.is_leaf() {
            let value = node.value.as_deref().unwrap_or("");
            let value_part = emit_value(value, &node.tag);
            lines.push(format!(
                "<{}{}>{}</{}>",
                node.tag, attr, value_part, node.tag
            ));
        } else {
            let children = node.children.as_ref().unwrap();
            if children.is_empty() {
                lines.push(format!("<{}{}/>", node.tag, attr));
            } else {
                lines.push(format!("<{}{}>", node.tag, attr));
                emit_nodes(children, lines, sort);
                lines.push(format!("</{}>", node.tag));
            }
        }
    }
}

/// Emits a value, using CDATA if needed.
fn emit_value(value: &str, tag: &str) -> String {
    let always_cdata = tag == "Description";
    let needs_cdata =
        always_cdata || value.contains('<') || value.contains('>') || value.contains('&');

    if needs_cdata {
        // Split on every "]]" and rejoin with the CDATA split marker
        let parts: Vec<&str> = value.split("]]").collect();
        let cdata_value = parts.join("]]]]><![CDATA[");
        format!("\n<![CDATA[{}]]>", cdata_value)
    } else {
        value.to_string()
    }
}

/// Normalizes a tree by removing empty leaves and default-valued keywords.
pub fn normalize(nodes: Vec<Node>) -> Vec<Node> {
    let mut result = Vec::new();

    for mut node in nodes {
        // Drop empty leaves
        if node.is_leaf() {
            let value = node.value.as_deref().unwrap_or("");
            if value.is_empty() {
                continue;
            }

            // Drop BlockCount 0 and ConditionalWithHostVars 0
            if (node.tag == "BlockCount" || node.tag == "ConditionalWithHostVars") && value == "0" {
                continue;
            }
        } else if let Some(children) = node.children {
            node.children = Some(normalize(children));
        }

        result.push(node);
    }

    result
}

/// A Storage block span in a class file.
#[derive(Debug, Clone, PartialEq)]
pub struct StorageSpan {
    pub name: String,
    pub start: usize,
    pub body_start: usize,
    pub body_end: usize,
    pub end: usize,
}

/// Splits a class file into Storage blocks by parsing elements to find body boundaries.
pub fn split_class(text: &str) -> Result<Vec<StorageSpan>, ParseError> {
    let mut spans = Vec::new();

    // Find all "\nStorage <name>\n{\n" markers (Storage at start of line)
    let mut search_pos = 0;
    while let Some(offset) = text[search_pos..].find("\nStorage ") {
        let storage_marker = search_pos + offset + 1; // Skip the leading \n

        // Extract storage name (up to newline)
        let name_start = storage_marker + "Storage ".len();
        let name_end = match text[name_start..].find('\n') {
            Some(offset) => name_start + offset,
            None => {
                search_pos = name_start;
                continue;
            }
        };

        let storage_name = text[name_start..name_end].trim().to_string();

        // Check for opening brace
        if !text[name_end..].starts_with("\n{\n") {
            search_pos = name_end;
            continue;
        }

        let body_start = name_end + 3; // Skip "\n{\n"

        // Parse elements to find the end of the Storage block
        let mut pos = body_start;
        loop {
            let (_, new_pos) = parse_element(text, pos, 0)?;

            pos = new_pos;

            if text[pos..].starts_with("\n}") {
                let body_end = pos;
                let end = pos + 2; // Include "\n}"

                spans.push(StorageSpan {
                    name: storage_name.clone(),
                    start: storage_marker,
                    body_start,
                    body_end,
                    end,
                });

                search_pos = end;
                break;
            }

            if !text[pos..].starts_with("\n<") {
                return Err(parse_error(
                    pos,
                    text,
                    "expected newline and element or close brace".to_string(),
                ));
            }

            pos += 1; // Skip '\n'
        }
    }

    Ok(spans)
}
