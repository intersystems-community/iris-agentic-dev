//! Parses BPL and DTL XData blocks into structured flow representations.
//!
//! Both BPL and DTL classes store their logic in XML inside an XData block.
//! `parse_bpl` and `parse_dtl` consume that XML (not the full class XML) and
//! return typed structs that callers can serialize into JSON.

use anyhow::{bail, Result};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde::Serialize;

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum XDataFlow {
    #[serde(rename = "bpl")]
    Bpl(BplFlow),
    #[serde(rename = "dtl")]
    Dtl(DtlFlow),
}

#[derive(Debug, Clone, Serialize)]
pub struct BplFlow {
    pub steps: Vec<BplStep>,
    pub has_dynamic_dispatch: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "step_kind", rename_all = "PascalCase")]
pub enum BplStep {
    Code {
        name: String,
    },
    Call {
        name: String,
        target: String,
        #[serde(rename = "async")]
        async_: bool,
    },
    If {
        name: String,
        condition: String,
        steps: Vec<BplStep>,
    },
    Other {
        name: String,
        step_type: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct DtlFlow {
    pub source_class: String,
    pub target_class: String,
    pub subtransforms: Vec<Subtransform>,
    pub assign_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Subtransform {
    pub class: String,
}

// ── BPL parser ───────────────────────────────────────────────────────────────

/// Parses the content of a BPL `<process>` XML element.
pub fn parse_bpl(xml: &str) -> Result<BplFlow> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut has_dynamic_dispatch = false;
    let mut steps = Vec::new();

    // Walk the XML; collect steps from the top-level <sequence> (or <process> body).
    collect_bpl_steps(&mut reader, xml, &mut steps, &mut has_dynamic_dispatch, 0)?;

    Ok(BplFlow {
        steps,
        has_dynamic_dispatch,
    })
}

/// Recursively collects BplStep entries from the current XML context.
/// `depth` tracks nesting so we know when to stop.
fn collect_bpl_steps(
    reader: &mut Reader<&[u8]>,
    _source: &str,
    steps: &mut Vec<BplStep>,
    dynamic: &mut bool,
    depth: u32,
) -> Result<()> {
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let tag = std::str::from_utf8(e.name().as_ref())
                    .unwrap_or("")
                    .to_lowercase();
                match tag.as_str() {
                    "sequence" => {
                        collect_bpl_steps(reader, _source, steps, dynamic, depth + 1)?;
                    }
                    "code" => {
                        let name = attr_value(e, "name");
                        let cdata = read_cdata_content(reader)?;
                        if cdata.contains("$classmethod") || cdata.contains("$ClassMethod") {
                            *dynamic = true;
                        }
                        steps.push(BplStep::Code { name });
                        // skip to </code>
                    }
                    "call" => {
                        let name = attr_value(e, "name");
                        let target = attr_value(e, "target");
                        let async_ = attr_value(e, "async") == "1";
                        steps.push(BplStep::Call {
                            name,
                            target,
                            async_,
                        });
                        skip_element(reader, "call")?;
                    }
                    "if" => {
                        let name = attr_value(e, "name");
                        let condition = attr_value(e, "condition");
                        let mut inner = Vec::new();
                        collect_bpl_steps(reader, _source, &mut inner, dynamic, depth + 1)?;
                        steps.push(BplStep::If {
                            name,
                            condition,
                            steps: inner,
                        });
                    }
                    "true" | "false" | "otherwise" | "case" | "when" => {
                        collect_bpl_steps(reader, _source, steps, dynamic, depth + 1)?;
                    }
                    "process" => {
                        // Top-level wrapper — descend into it.
                        collect_bpl_steps(reader, _source, steps, dynamic, depth + 1)?;
                    }
                    _ => {
                        // Other step kinds (assign, transform, break, etc.) — record as Other.
                        let name = attr_value(e, "name");
                        let step_type = tag.clone();
                        steps.push(BplStep::Other { name, step_type });
                        skip_element(reader, &tag)?;
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = std::str::from_utf8(e.name().as_ref())
                    .unwrap_or("")
                    .to_lowercase();
                match tag.as_str() {
                    "call" => {
                        let name = attr_value(e, "name");
                        let target = attr_value(e, "target");
                        let async_ = attr_value(e, "async") == "1";
                        steps.push(BplStep::Call {
                            name,
                            target,
                            async_,
                        });
                    }
                    "code" => {
                        let name = attr_value(e, "name");
                        steps.push(BplStep::Code { name });
                    }
                    "process" => {
                        // Self-closing <process/>: no steps, done.
                        return Ok(());
                    }
                    "assign" | "break" | "continue" | "trace" | "delay" | "sync" | "label"
                    | "milestone" | "reply" | "scope" => {
                        let name = attr_value(e, "name");
                        steps.push(BplStep::Other {
                            name,
                            step_type: tag,
                        });
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = std::str::from_utf8(e.name().as_ref())
                    .unwrap_or("")
                    .to_lowercase();
                if depth == 0 {
                    return Ok(());
                }
                match tag.as_str() {
                    "sequence" | "if" | "true" | "false" | "otherwise" | "case" | "when"
                    | "process" => {
                        return Ok(());
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => return Ok(()),
            Err(e) => bail!("XML error: {e}"),
            _ => {}
        }
    }
}

/// Reads content from within a <code> element, collecting CData and text until </code>.
fn read_cdata_content(reader: &mut Reader<&[u8]>) -> Result<String> {
    let mut content = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::CData(ref cd)) => {
                let text = std::str::from_utf8(cd.as_ref()).unwrap_or("").to_string();
                content.push_str(&text);
            }
            Ok(Event::Text(ref t)) => {
                let text = t.unescape().unwrap_or_default();
                content.push_str(&text);
            }
            Ok(Event::End(ref e)) => {
                let name_bytes = e.name().as_ref().to_vec();
                let tag = String::from_utf8_lossy(&name_bytes);
                if tag.eq_ignore_ascii_case("code") {
                    return Ok(content);
                }
            }
            Ok(Event::Eof) => return Ok(content),
            Err(e) => bail!("XML error reading code body: {e}"),
            _ => {}
        }
    }
}

/// Skips all children of the current element until the matching closing tag.
fn skip_element(reader: &mut Reader<&[u8]>, close_tag: &str) -> Result<()> {
    let mut depth = 1u32;
    loop {
        match reader.read_event() {
            Ok(Event::Start(_)) => depth += 1,
            Ok(Event::End(ref e)) => {
                depth -= 1;
                if depth == 0 {
                    let name_bytes = e.name().as_ref().to_vec();
                    let tag = String::from_utf8_lossy(&name_bytes).to_lowercase();
                    if tag.eq_ignore_ascii_case(close_tag) {
                        return Ok(());
                    }
                    return Ok(());
                }
            }
            Ok(Event::Empty(_)) => {}
            Ok(Event::Eof) => return Ok(()),
            Err(e) => bail!("XML error in skip_element: {e}"),
            _ => {}
        }
    }
}

// ── DTL parser ───────────────────────────────────────────────────────────────

/// Parses the content of a DTL `<transform>` XML element.
pub fn parse_dtl(xml: &str) -> Result<DtlFlow> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut source_class = String::new();
    let mut target_class = String::new();
    let mut subtransforms = Vec::new();
    let mut assign_count = 0u32;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                let tag = std::str::from_utf8(e.name().as_ref())
                    .unwrap_or("")
                    .to_lowercase();
                match tag.as_str() {
                    "transform" => {
                        source_class = attr_value(e, "sourceClass");
                        target_class = attr_value(e, "targetClass");
                    }
                    "subtransform" => {
                        let class = attr_value(e, "class");
                        if !class.is_empty() {
                            subtransforms.push(Subtransform { class });
                        }
                    }
                    "assign" => {
                        assign_count += 1;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => bail!("XML error parsing DTL: {e}"),
            _ => {}
        }
    }

    Ok(DtlFlow {
        source_class,
        target_class,
        subtransforms,
        assign_count,
    })
}

// ── Class XML XData extraction ───────────────────────────────────────────────

/// Extracts the CDATA content from the named XData block in a full class XML export.
///
/// IRIS class XML looks like:
/// ```xml
/// <Export>
///   <Class name="...">
///     <XData name="BPL"><Data><![CDATA[...]]></Data></XData>
///   </Class>
/// </Export>
/// ```
pub fn extract_xdata_content(class_xml: &str, xdata_name: &str) -> Option<String> {
    let mut reader = Reader::from_str(class_xml);
    reader.config_mut().trim_text(false);

    let mut inside_target = false;
    let mut inside_data = false;
    // Accumulate across multiple CData/Text events — IRIS exports BPL/DTL with
    // nested CDATA sections (]]]]><![CDATA[>) that quick_xml splits into fragments.
    let mut accumulated = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let tag = std::str::from_utf8(e.name().as_ref())
                    .unwrap_or("")
                    .to_lowercase();
                if tag == "xdata" {
                    let name = attr_value(e, "name");
                    inside_target = name.eq_ignore_ascii_case(xdata_name);
                } else if tag == "data" && inside_target {
                    inside_data = true;
                    accumulated.clear();
                }
            }
            Ok(Event::CData(ref cd)) if inside_data => {
                accumulated.push_str(&String::from_utf8_lossy(cd.as_ref()));
            }
            Ok(Event::Text(ref t)) if inside_data => {
                let text = t.unescape().unwrap_or_default().to_string();
                if !text.trim().is_empty() {
                    accumulated.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = std::str::from_utf8(e.name().as_ref())
                    .unwrap_or("")
                    .to_lowercase();
                if tag == "data" && inside_data {
                    inside_data = false;
                    if !accumulated.trim().is_empty() {
                        return Some(accumulated);
                    }
                } else if tag == "xdata" {
                    inside_target = false;
                }
            }
            Ok(Event::Eof) => return None,
            _ => {}
        }
    }
}

/// Detects whether the class XML represents a BPL class by checking for the BPL XData block.
pub fn is_bpl_class(super_classes: &str) -> bool {
    super_classes
        .split(',')
        .any(|s| s.trim().eq_ignore_ascii_case("Ens.BusinessProcessBPL"))
}

/// Detects whether the class XML represents a DTL class.
pub fn is_dtl_class(super_classes: &str) -> bool {
    super_classes
        .split(',')
        .any(|s| s.trim().eq_ignore_ascii_case("Ens.DataTransformDTL"))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn attr_value(e: &quick_xml::events::BytesStart, name: &str) -> String {
    e.attributes()
        .filter_map(|a| a.ok())
        .find(|a| a.key.as_ref().eq_ignore_ascii_case(name.as_bytes()))
        .and_then(|a| a.unescape_value().ok())
        .map(|v| v.to_string())
        .unwrap_or_default()
}
