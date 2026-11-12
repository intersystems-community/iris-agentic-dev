//! One generated class (T104): from a row of the covering array, the class text, the global's
//! nodes, and the known question with its answer. The node values come from this module's own
//! encoder (pieces joined with the delimiter, or `$LB` elements), never from `storage_map` code,
//! and the answer is the row model itself, so the known answer does not come from the system
//! under test.

use super::Dim;
use serde_json::{json, Value};

/// A node's value: delimited text, or a `$LIST` of strings.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeValue {
    Text(String),
    List(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct GenClass {
    pub name: String,
    pub global: String,
    pub cls_text: String,
    /// The table's rows in ID order: the keys, then F1, F2 (and F3 when computed).
    pub rows: Vec<Vec<Value>>,
    pub nodes: Vec<(Vec<String>, NodeValue)>,
    pub question: String,
    pub answer: Vec<Value>,
    pub build_indexes: Vec<String>,
}

fn pick<'a>(row: &[usize], dims: &'a [Dim], name: &str) -> &'a str {
    let i = dims.iter().position(|d| d.name == name).unwrap();
    &dims[i].values[row[i]]
}

/// The pieces of one node, encoded as the map reads them.
fn encode(values: &[&str], delimiter: &str) -> NodeValue {
    match delimiter {
        "list" => NodeValue::List(values.iter().map(|v| v.to_string()).collect()),
        "pipe" => NodeValue::Text(values.join("|")),
        "ctrl1" => NodeValue::Text(values.join("\u{1}")),
        _ => NodeValue::Text(values.join("^")),
    }
}

fn delimiter_element(delimiter: &str) -> Option<&'static str> {
    match delimiter {
        "caret" => Some("\"^\""),
        "pipe" => Some("\"|\""),
        "ctrl1" => Some("$C(1)"),
        _ => None,
    }
}

pub fn class_for(row: &[usize], dims: &[Dim], n: usize) -> GenClass {
    let depth: usize = pick(row, dims, "depth").parse().unwrap();
    let delimiter = pick(row, dims, "delimiter");
    let layout = pick(row, dims, "node_layout");
    let computed = pick(row, dims, "computed") == "level1";
    let access = pick(row, dims, "access_type");
    let skip = pick(row, dims, "next_code") == "skip_named";
    let index = pick(row, dims, "index") == "on_f1";

    let name = format!("IadSm172Gen.C{n:03}");
    let global = format!("^IadSm172Gen{n:03}");
    let keys: Vec<Vec<i64>> = match depth {
        1 => vec![vec![1], vec![2], vec![3]],
        2 => vec![vec![1, 1], vec![1, 2], vec![2, 1]],
        _ => vec![vec![1, 1, 1], vec![1, 1, 2], vec![2, 1, 1]],
    };
    let f1 = ["ada", "bob", "cy"];
    let f2 = ["x1", "y2", "z3"];

    // Nodes, per layout: which node holds which field, at which piece.
    let mut nodes = Vec::new();
    for (r, k) in keys.iter().enumerate() {
        let base: Vec<String> = k.iter().map(|x| x.to_string()).collect();
        let at = |sub: Option<&str>| {
            let mut s = base.clone();
            if let Some(x) = sub {
                s.push(x.to_string());
            }
            s
        };
        match layout {
            "row" => nodes.push((at(None), encode(&[f1[r], f2[r]], delimiter))),
            "sub" => nodes.push((at(Some("d")), encode(&[f1[r], f2[r]], delimiter))),
            "row_and_sub" => {
                nodes.push((at(None), encode(&[f1[r]], delimiter)));
                nodes.push((at(Some("d")), encode(&[f2[r]], delimiter)));
            }
            _ => {
                nodes.push((at(Some("a")), encode(&[f1[r]], delimiter)));
                nodes.push((at(Some("b")), encode(&[f2[r]], delimiter)));
            }
        }
    }
    if skip {
        // A named node at the walked level, which the NextCode must skip.
        let mut s: Vec<String> = keys[0][..depth - 1].iter().map(|x| x.to_string()).collect();
        s.push("zz".into());
        nodes.push((s, NodeValue::Text("junk".into())));
    }

    let (f1_node, f1_piece, f2_node, f2_piece) = match layout {
        "row" => (None, 1, None, 2),
        "sub" => (Some("\"d\""), 1, Some("\"d\""), 2),
        "row_and_sub" => (None, 1, Some("\"d\""), 1),
        _ => (Some("\"a\""), 1, Some("\"b\""), 1),
    };
    let data = |field: &str, node: Option<&str>, piece: usize| {
        let mut s = format!("<Data name=\"{field}\">\n");
        if let Some(d) = delimiter_element(delimiter) {
            s.push_str(&format!("<Delimiter>{d}</Delimiter>\n"));
        }
        if let Some(n) = node {
            s.push_str(&format!("<Node>{n}</Node>\n"));
        }
        s.push_str(&format!("<Piece>{piece}</Piece>\n</Data>\n"));
        s
    };
    let mut master = String::from("<SQLMap name=\"Master\">\n");
    master.push_str(&data("F1", f1_node, f1_piece));
    master.push_str(&data("F2", f2_node, f2_piece));
    if computed {
        master
            .push_str("<Data name=\"F3\">\n<RetrievalCode> s {*}={L1}</RetrievalCode>\n</Data>\n");
    }
    master.push_str(&format!("<Global>{global}</Global>\n"));
    for level in 1..=depth {
        master.push_str(&format!(
            "<Subscript name=\"{level}\">\n<Expression>{{K{level}}}</Expression>\n"
        ));
        if access != "none" {
            master.push_str(&format!("<AccessType>{access}</AccessType>\n"));
        }
        if skip && level == depth {
            let refs: Vec<String> = (1..=depth).map(|l| format!("{{L{l}}}")).collect();
            let r = refs.join(",");
            master.push_str(&format!(
                "<NextCode> s {{L{depth}}}=$o({global}({r}))\n f  {{\n\tq:{{L{depth}}}=\"\"\n\tq:{{L{depth}}}=+{{L{depth}}}\n\ts {{L{depth}}}=$o({global}({r}))\n }}</NextCode>\n"
            ));
        }
        master.push_str("</Subscript>\n");
    }
    master.push_str("<Type>data</Type>\n</SQLMap>\n");

    let mut maps = String::new();
    if index {
        maps.push_str(&format!(
            "<SQLMap name=\"IdxF1\">\n<Global>{global}I</Global>\n<Subscript name=\"1\">\n<Expression>{{F1}}</Expression>\n</Subscript>\n"
        ));
        for level in 1..=depth {
            maps.push_str(&format!(
                "<Subscript name=\"{}\">\n<Expression>{{K{level}}}</Expression>\n</Subscript>\n",
                level + 1
            ));
        }
        maps.push_str("<Type>index</Type>\n</SQLMap>\n");
    }
    maps.push_str(&master);

    let key_props: String = (1..=depth)
        .map(|l| format!("Property K{l} As %Integer;\n\n"))
        .collect();
    let key_list: Vec<String> = (1..=depth).map(|l| format!("K{l}")).collect();
    let idkey = if depth == 1 {
        "Index IDKEY On K1 [ IdKey, Unique ];\n\n".to_string()
    } else {
        format!(
            "Index IDKEY On ({}) [ IdKey, Unique ];\n\n",
            key_list.join(", ")
        )
    };
    let cls_text = format!(
        "Class {name} Extends %Persistent [ StorageStrategy = SQLStorage ]\n{{\n\n{key_props}\
         Property F1 As %String(COLLATION = \"EXACT\");\n\nProperty F2 As %String;\n\n{}{idkey}{}\
         Storage SQLStorage\n{{\n{maps}<StreamLocation>{global}S</StreamLocation>\n<Type>%Storage.SQL</Type>\n}}\n\n}}\n",
        if computed { "Property F3 As %Integer;\n\n" } else { "" },
        if index { "Index IdxF1 On F1;\n\n" } else { "" },
    );

    let rows: Vec<Vec<Value>> = keys
        .iter()
        .enumerate()
        .map(|(r, k)| {
            let mut v: Vec<Value> = k.iter().map(|x| json!(x)).collect();
            v.push(json!(f1[r]));
            v.push(json!(f2[r]));
            if computed {
                v.push(json!(k[0]));
            }
            v
        })
        .collect();
    let mut columns = key_list.clone();
    columns.push("F1".into());
    columns.push("F2".into());
    if computed {
        columns.push("F3".into());
    }
    let wanted: Vec<&Vec<Value>> = rows
        .iter()
        .filter(|r| !index || r[depth] == "bob")
        .collect();
    let answer = wanted
        .iter()
        .map(|r| {
            let o: serde_json::Map<String, Value> =
                columns.iter().cloned().zip(r.iter().cloned()).collect();
            Value::Object(o)
        })
        .collect();
    let question = format!(
        "SELECT {} FROM {name} {}ORDER BY {}",
        columns.join(", "),
        if index { "WHERE F1 = 'bob' " } else { "" },
        key_list.join(", ")
    );
    GenClass {
        name,
        global,
        cls_text,
        rows,
        nodes,
        question,
        answer,
        build_indexes: if index { vec!["IdxF1".into()] } else { vec![] },
    }
}

/// The seed file the rig reads (`storage172_rig::Fixture`), for `g`.
pub fn seed_json(g: &GenClass) -> Value {
    let globals: Vec<Value> = g
        .nodes
        .iter()
        .map(|(subs, v)| match v {
            NodeValue::Text(t) => json!({"global": g.global, "subscripts": subs, "value": t}),
            NodeValue::List(items) => json!({"global": g.global, "subscripts": subs, "lb": items}),
        })
        .collect();
    let mut seed = json!({
        "class": g.name,
        "globals": globals,
        "question": g.question,
        "answer": g.answer,
    });
    if !g.build_indexes.is_empty() {
        seed["build_indexes"] = json!(g.build_indexes);
        seed["kill"] = json!([format!("{}I", g.global)]);
    }
    seed
}
