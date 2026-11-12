//! Plain-words explanations of the IRIS compile errors that storage-map elements raise
//! (spec 172 FR-017). The IRIS message texts were read from `%ObjectErrors` on 2026.2; the
//! behaviour notes come from research R4 and R5.

/// Each error number `explain` knows, with its explanation.
pub const EXPLAINED: [(u32, &str); 24] = [
    (
        5002,
        "ObjectScript in a map element does not run: the generated routine failed to compile. \
            Check RetrievalCode, Condition, NextCode and the subscript expressions; `&gt;` or \
            `&amp;` written inside one is stored literally and breaks the routine (R4).",
    ),
    (
        5502,
        "The SQL table did not compile. When IRIS lists unmapped fields, a property of the class \
         is in no data map: add a Data element for it to the master map, or make it calculated \
         (measured 2026-11-11).",
    ),
    (
        5508,
        "A Data element of the map names a field that is not valid for that map. Each Data \
            name must be a property of the class.",
    ),
    (
        5509,
        "A RowIdSpec entry names a field IRIS cannot use for the row ID. Its Field must be a \
            property of the class, and its name an integer subscript level.",
    ),
    (
        5510,
        "A Subscript of the map is invalid. Subscript names must be integers 1, 2, 3 … in \
            order, and each needs an Expression.",
    ),
    (
        5511,
        "The map's Type is not one IRIS accepts. It must be one of data, index, bitmap, \
            bitmapextent or columnar, in lower case: the check is case-sensitive, so DATA \
            fails (R4).",
    ),
    (
        5513,
        "A Data element names something that is not a field of the class. Add the property, \
            or remove the Data element.",
    ),
    (
        5524,
        "A {Field} reference names a field that does not exist, or one this element may not \
            read. RetrievalCode may not name another field: read the global through {L1} \
            instead (R5).",
    ),
    (
        5538,
        "An access variable (Accessvar) of the map has no Code expression. Give it the \
            expression IRIS evaluates at that subscript level.",
    ),
    (
        5539,
        "An access variable (Accessvar) of the map has no name at one subscript level. \
            Each Accessvar needs a name such as {3D1}.",
    ),
    (
        5541,
        "A subscript Expression refers to a field IRIS does not know. Use a field of the \
            class, {ID}, or a {Li} reference to an earlier subscript.",
    ),
    (
        5542,
        "A DataAccess expression is invalid. It may only use {Li}, {Di} or {iDj} references \
            from an earlier subscript level.",
    ),
    (
        5543,
        "An Invalidcondition, NextCode, RowReference or StopExpression is invalid. It may \
            use {Li} or {Di} from this or an earlier subscript level, or {iDj} from an earlier \
            one.",
    ),
    (
        5544,
        "An access variable's Code expression is invalid. It may use {Li}, {Di} or {iDj} \
            references from this or an earlier subscript level.",
    ),
    (
        5545,
        "A RetrievalCode expression is invalid. It may use {Li}, {Di}, {iDj}, {%row}, \
            {%rowraw} or {*} (this field), not another field by name (R5).",
    ),
    (
        5546,
        "A RowIdSpec Expression is invalid. It must be a {Li} reference or a field from the \
            map's Data.",
    ),
    (
        5547,
        "A subscript Expression is not a valid field reference. In the master map (the first \
            data map) each subscript must be an IDKEY field or {ID}.",
    ),
    (
        5548,
        "A Data element's name is not a valid field reference. Use the property name as the \
            class defines it.",
    ),
    (
        5549,
        "A Data element's Node is invalid. It must be a {Di} or {iDj} reference.",
    ),
    (
        5583,
        "A Data element in an index map has a Node. Only data maps may store field values in \
            nodes; remove the Node, or move the field to a data map.",
    ),
    (
        5585,
        "IRIS cannot work out the row ID for this map. Add RowIdSpec entries that say which \
            subscript level holds each part of the ID.",
    ),
    (
        5596,
        "A map has no Global. Every SQLMap needs a Global element naming the global its rows \
            are stored in.",
    ),
    (
        15511,
        "The map is marked conditional but its Condition is empty. Give the Condition, or \
             make the map unconditional.",
    ),
    (
        16023,
        "IRIS stored the class but cannot read it back: an element it could not parse, such \
             as a Subscript name that is not an integer (R4). The class stays unreadable until a \
             valid version is saved.",
    ),
];

/// The explanation for the first error number in `iris_error` that [`EXPLAINED`] knows, read
/// from `ERROR #nnnn` or `ERROR#nnnn`. `None` when it names no known number.
pub fn explain(iris_error: &str) -> Option<String> {
    explain_code(iris_error).map(|(_, text)| text.to_string())
}

/// [`explain`] with the number it explained.
fn explain_code(iris_error: &str) -> Option<(u32, &'static str)> {
    iris_error.split('#').skip(1).find_map(|after| {
        let end = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        let n: u32 = after[..end].parse().ok()?;
        EXPLAINED.iter().find(|(k, _)| *k == n).copied()
    })
}

/// `{code, text}` for each error's first known number, in order, each number once
/// (`STORAGE_MAP_COMPILE_FAILED`'s `explanations`).
pub fn explanations(errors: &[String]) -> Vec<serde_json::Value> {
    let mut seen = std::collections::HashSet::new();
    errors
        .iter()
        .filter_map(|e| explain_code(e))
        .filter(|(n, _)| seen.insert(*n))
        .map(|(code, text)| serde_json::json!({"code": code, "text": text}))
        .collect()
}
