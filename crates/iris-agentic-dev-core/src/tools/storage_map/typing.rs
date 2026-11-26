//! Datatypes and collations for the class draft (spec 173, FR-020 to FR-022): the pure half. The
//! server counts what the sampled values of each field look like and returns only the counts
//! (`Evidence`); this module turns the counts into a property type, turns the linking evidence
//! into a `COLLATION` parameter, and builds the class source. `Evidence` has no field that could
//! hold a sampled value, so none can reach the source (FR-012, FR-021).

use super::master::is_valid_property_name;
use crate::objectscript::validate_class_name;
use serde_json::{json, Value};
use std::collections::HashSet;

/// What the server counted over the sampled values of one field. Counts only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Evidence {
    /// Non-empty values examined.
    pub of: u32,
    /// Values that are whole numbers.
    pub integer: u32,
    /// Values that are numbers (whole ones included).
    pub numeric: u32,
    /// The most fraction digits any value had.
    pub max_scale: u32,
    /// Whole-number values in a plausible date range.
    pub date_fit: u32,
    /// The longest value, in characters.
    pub max_length: u32,
}

/// A date guess and how many of the sampled values fit it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateGuess {
    pub fit: u32,
    pub of: u32,
}

/// The type a property gets.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyType {
    /// `%Integer`, `%Numeric`, `%Date` or `%String`.
    pub name: String,
    /// Type parameters as the class spells them (`SCALE = 2`, `MAXLEN = 64`).
    pub parameters: Vec<String>,
    /// Set when the type is a date guess.
    pub date_guess: Option<DateGuess>,
    /// The counts behind the guess, as the answer returns them.
    pub evidence: Value,
    /// A sentence for the class source: the counts in words.
    pub note: String,
}

/// The `MAXLEN` sizes, smallest first; a longer value rounds up to a multiple of the last.
const MAXLEN_STEPS: [u32; 5] = [32, 64, 128, 255, 1024];

fn maxlen_for(longest: u32) -> u32 {
    for step in MAXLEN_STEPS {
        if longest <= step {
            return step;
        }
    }
    let last = MAXLEN_STEPS[MAXLEN_STEPS.len() - 1];
    longest.div_ceil(last).saturating_mul(last)
}

/// The type the counts support, first match wins: a date guess (at least 95% of the values fit),
/// `%Integer` (every value is whole), `%Numeric` with the longest fraction as its scale (every
/// value is a number), else `%String` with a `MAXLEN` rounded up from the longest value.
pub fn property_type(e: &Evidence) -> PropertyType {
    let of = e.of;
    if of > 0 && u64::from(e.date_fit) * 100 >= u64::from(of) * 95 {
        return PropertyType {
            name: "%Date".into(),
            parameters: Vec::new(),
            date_guess: Some(DateGuess {
                fit: e.date_fit,
                of,
            }),
            evidence: json!({"date_fit": e.date_fit, "of": of}),
            note: format!(
                "Guessed: {} of {of} sampled values are whole numbers in a plausible date range.",
                e.date_fit
            ),
        };
    }
    if of > 0 && e.integer == of {
        return PropertyType {
            name: "%Integer".into(),
            parameters: Vec::new(),
            date_guess: None,
            evidence: json!({"integer": e.integer, "of": of}),
            note: format!(
                "Guessed: {} of {of} sampled values are whole numbers.",
                e.integer
            ),
        };
    }
    if of > 0 && e.numeric == of {
        return PropertyType {
            name: "%Numeric".into(),
            parameters: vec![format!("SCALE = {}", e.max_scale)],
            date_guess: None,
            evidence: json!({"numeric": e.numeric, "max_scale": e.max_scale, "of": of}),
            note: format!(
                "Guessed: {} of {of} sampled values are numbers with up to {} fraction digits.",
                e.numeric, e.max_scale
            ),
        };
    }
    PropertyType {
        name: "%String".into(),
        parameters: vec![format!("MAXLEN = {}", maxlen_for(e.max_length))],
        date_guess: None,
        evidence: json!({"max_length": e.max_length}),
        note: format!(
            "Guessed: the longest sampled value is {} characters.",
            e.max_length
        ),
    }
}

/// The `COLLATION` parameter for a field the linker found stored through `transform` (one of
/// `exact`, `sqlupper`, `sqlstring`, `string`, `alphaup`, `upper`, `mvr`, `truncate`), as the
/// class spells it: `COLLATION = "EXACT"`. `None` for plain `sqlupper`, the default. A length
/// gives the variant (`SQLUPPER(4)`); `truncate` needs one, and the transforms that take no
/// length refuse one. An unknown transform or a zero length is an error.
pub fn collation_parameter(transform: &str, length: Option<u32>) -> Result<Option<String>, String> {
    let (base, takes_length, needs_length) = match transform {
        "exact" => ("EXACT", false, false),
        "sqlupper" => ("SQLUPPER", true, false),
        "sqlstring" => ("SQLSTRING", true, false),
        "string" => ("STRING", true, false),
        "alphaup" => ("ALPHAUP", false, false),
        "upper" => ("UPPER", false, false),
        "mvr" => ("MVR", false, false),
        "truncate" => ("TRUNCATE", true, true),
        other => return Err(format!("no collation for the transform {other:?}")),
    };
    match length {
        Some(0) => Err(format!("a length of 0 is not a {transform} length")),
        Some(n) if takes_length => Ok(Some(format!("COLLATION = \"{base}({n})\""))),
        Some(_) => Err(format!("{transform} takes no length")),
        None if needs_length => Err(format!("{transform} needs a length")),
        None if transform == "sqlupper" => Ok(None),
        None => Ok(Some(format!("COLLATION = \"{base}\""))),
    }
}

/// A property to draft: its name, the type from `property_type`, and the collation parameter
/// text from `collation_parameter`, when there is one.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertySpec {
    pub name: String,
    pub ty: PropertyType,
    pub collation: Option<String>,
}

impl PropertySpec {
    /// Every parameter in the order the class lists them: the type's, then the collation.
    fn parameters(&self) -> Vec<String> {
        self.ty
            .parameters
            .iter()
            .cloned()
            .chain(self.collation.iter().cloned())
            .collect()
    }
}

/// The class draft: source, each property, and the paths of what was guessed.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassDraft {
    pub source: String,
    pub properties: Vec<PropertySpec>,
    /// One path per property: every property is a guess.
    pub guesses: Vec<String>,
}

impl ClassDraft {
    /// The `class_draft` answer (data-model.md): `compiled` is what the scratch compile said.
    pub fn to_json(&self, compiled: bool) -> Value {
        let properties: Vec<Value> = self
            .properties
            .iter()
            .map(|p| {
                json!({
                    "name": p.name,
                    "type": p.ty.name,
                    "parameters": p.parameters(),
                    "evidence": p.ty.evidence,
                })
            })
            .collect();
        json!({
            "source": self.source,
            "compiled": compiled,
            "properties": properties,
            "guesses": self.guesses,
        })
    }
}

/// Whether `text` is one `COLLATION = "<NAME>"` or `COLLATION = "<NAME>(<n>)"` parameter, the
/// only shape `collation_parameter` makes.
fn is_collation_parameter(text: &str) -> bool {
    let Some(value) = text
        .strip_prefix("COLLATION = \"")
        .and_then(|t| t.strip_suffix('"'))
    else {
        return false;
    };
    let (name, rest) = match value.split_once('(') {
        Some((n, r)) => (n, Some(r)),
        None => (value, None),
    };
    let name_ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase());
    let rest_ok = rest.is_none_or(|r| {
        r.strip_suffix(')')
            .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
    });
    name_ok && rest_ok
}

/// The class source for `properties`: one `Property <name> As <type>(<parameters>);` each, with
/// a comment saying what the guess rests on. The class carries `StorageStrategy = SQLStorage`;
/// the Storage block with the drafted maps is added by the caller before the final brace. The
/// class name, every property name and every collation parameter are checked first.
pub fn class_source(name: &str, properties: &[PropertySpec]) -> Result<ClassDraft, String> {
    if validate_class_name(name).is_err() {
        return Err(format!("{name:?} is not a class name"));
    }
    let mut seen = HashSet::new();
    for p in properties {
        if !is_valid_property_name(&p.name) {
            return Err(format!("{:?} is not a valid property name", p.name));
        }
        if !seen.insert(p.name.to_ascii_lowercase()) {
            return Err(format!("duplicate property name {:?}", p.name));
        }
        if let Some(c) = &p.collation {
            // IRIS refuses COLLATION on %Integer, %Numeric and %Date (ERROR #5480, measured).
            if p.ty.name != "%String" {
                return Err(format!(
                    "property {} is {}, which takes no COLLATION parameter (only %String does)",
                    p.name, p.ty.name
                ));
            }
            if !is_collation_parameter(c) {
                return Err(format!(
                    "{:?} is not a COLLATION parameter for property {}",
                    c, p.name
                ));
            }
        }
    }
    let mut source =
        format!("Class {name} Extends %Persistent [ StorageStrategy = SQLStorage ]\n{{\n\n");
    for p in properties {
        source.push_str(&format!("/// {}\n", p.ty.note));
        let params = p.parameters();
        let args = if params.is_empty() {
            String::new()
        } else {
            format!("({})", params.join(", "))
        };
        source.push_str(&format!("Property {} As {}{args};\n\n", p.name, p.ty.name));
    }
    source.push_str("}\n");
    let guesses = properties
        .iter()
        .map(|p| format!("/class_draft/properties/{}", p.name))
        .collect();
    Ok(ClassDraft {
        source,
        properties: properties.to_vec(),
        guesses,
    })
}
