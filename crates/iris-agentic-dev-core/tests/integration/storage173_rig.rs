//! Live rig for the spec 173 tests (`tests/fixtures/storage173/`). Everything is invented for this
//! repository: classes under `IadSm173`, globals starting `^IadSm173`.
//!
//! A probe or fixture is a class text, rows to INSERT through SQL (so IRIS files the index globals
//! itself), and optional hand-set globals. `drop_all` kills every `^IadSm173*` global and deletes the
//! `IadSm173` package, so a test starts and ends clean.

use crate::live_rig_171::Rig;
use serde_json::{json, Value};

// Used by the Phase 3 fixtures.
#[allow(dead_code)]
pub const PKG: &str = "IadSm173";

/// `"text"` as an ObjectScript string literal.
pub fn os(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Run ObjectScript in USER and return its output (a failed call panics with the answer).
pub async fn exec(r: &Rig, code: &str) -> String {
    let out = r
        .call("iris_execute", json!({"code": code, "namespace": "USER"}))
        .await;
    assert_eq!(out["success"], true, "exec failed: {out}\ncode: {code}");
    out["output"].as_str().unwrap_or_default().to_string()
}

/// PUT and compile a class; panics with the answer when it fails.
pub async fn put_class(r: &Rig, name: &str, text: &str) {
    let put = r
        .call(
            "iris_doc",
            json!({"mode": "put", "name": format!("{name}.cls"), "content": text,
                "compile": true, "namespace": "USER"}),
        )
        .await;
    assert_eq!(put["success"], true, "{name}: put and compile: {put}");
}

/// PUT and compile a class and return whether it compiled, with the answer text.
pub async fn try_class(r: &Rig, name: &str, text: &str) -> (bool, String) {
    let put = r
        .call(
            "iris_doc",
            json!({"mode": "put", "name": format!("{name}.cls"), "content": text,
                "compile": true, "namespace": "USER"}),
        )
        .await;
    (put["success"] == true, put.to_string())
}

/// Every string in `v` that ends in `.cls`.
fn class_names(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) if s.ends_with(".cls") => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| class_names(x, out)),
        Value::Object(o) => o.values().for_each(|x| class_names(x, out)),
        _ => {}
    }
}

/// Kill every `^IadSm173*` global of USER and delete every `IadSm173.*` class.
pub async fn drop_all(r: &Rig) {
    let code = r#"Set rs=##class(%ResultSet).%New("%SYS.GlobalQuery:NameSpaceList")
Do rs.Execute($Namespace,"IadSm173*",1,,,0)
While rs.Next() { Set g="^"_rs.Get("Name") Kill @g }
Write "DROPPED"
"#;
    let out = exec(r, code).await;
    assert!(out.contains("DROPPED"), "drop_all: {out}");
    let listed = r
        .call(
            "iris_doc",
            json!({"mode": "list", "pattern": "IadSm173.*", "category": "CLS", "namespace": "USER"}),
        )
        .await;
    let mut names = Vec::new();
    class_names(&listed, &mut names);
    names.sort();
    names.dedup();
    for n in names.iter().filter(|n| n.starts_with("IadSm173.")) {
        let del = r
            .call(
                "iris_doc",
                json!({"mode": "delete", "name": n, "namespace": "USER"}),
            )
            .await;
        assert_eq!(del["success"], true, "delete {n}: {del}");
    }
}

/// INSERT `rows` (each a list of JSON values) into `table` through SQL, 100 rows a call.
pub async fn insert_rows(r: &Rig, table: &str, columns: &[&str], rows: &[Vec<Value>]) {
    assert!(!rows.is_empty(), "{table}: no rows to insert");
    for chunk in rows.chunks(100) {
        let mut code = String::from("Set st=##class(%SQL.Statement).%New() Set bad=0\n");
        for row in chunk {
            let vals: Vec<String> = row
                .iter()
                .map(|v| match v {
                    Value::Null => "NULL".to_string(),
                    Value::String(s) => format!("'{}'", s.replace('\'', "''")),
                    other => other.to_string(),
                })
                .collect();
            let sql = format!(
                "INSERT INTO {table} ({}) VALUES ({})",
                columns.join(","),
                vals.join(",")
            );
            code.push_str(&format!(
                "Set rs=st.%ExecDirect(,{}) If rs.%SQLCODE<0 {{ Write \"SQLCODE \",rs.%SQLCODE,\" \",rs.%Message,! Set bad=1 }}\n",
                os(&sql)
            ));
        }
        code.push_str("Write $Select(bad:\"INSERT-FAILED\",1:\"INSERTED\")\n");
        let out = exec(r, &code).await;
        assert!(out.contains("INSERTED"), "{table}: {out}");
    }
}

/// The rows of a SELECT (objects keyed by column name).
pub async fn select(r: &Rig, query: &str) -> Vec<Value> {
    let q = r
        .call("iris_query", json!({"query": query, "namespace": "USER"}))
        .await;
    assert_eq!(q["success"], true, "select {query}: {q}");
    q["rows"].as_array().cloned().unwrap_or_default()
}

/// `ZWRITE`-style dump of a global: one `ref=value` line per node in `$Query` order, `$c(0)` shown
/// as `<c0>`, and a value holding control characters (a bit string) as `<bits:1,3>` (the positions
/// Dump a global: one `ref=value` line per node in `$Query` order. A string subscript is quoted
/// with `$c(0)` shown as `<c0>` and another control character as `<n>`; a value of `$c(0)` is `<c0>`; a value holding control characters
/// (a bit string) is `<bits:1,3>`, the positions of its set bits.
pub async fn dump_global(r: &Rig, global: &str) -> Vec<String> {
    let g = os(&format!("^{}", global.trim_start_matches('^')));
    let code = format!(
        r#"Set g={g},n=g,first=1
For {{
  If first {{ Set first=0 If '($Data(@g)#2) {{ Set n=$Query(@g) }} }} Else {{ Set n=$Query(@n) }}
  Quit:n=""
  Set ref=$Name(@n,0)_"(" Set d=$QLength(n)
  For k=1:1:d {{ Set x=$QSubscript(n,k) Set e="" If '($IsValidNum(x)&&(x=+x)) {{ For i=1:1:$Length(x) {{ Set a=$Ascii(x,i) Set e=e_$Select(a=0:"<c0>",a<32:"<"_a_">",1:$Extract(x,i)) }} Set e=""""_e_"""" }} Else {{ Set e=x }} Set ref=ref_$Select(k>1:",",1:"")_e }}
  Set ref=ref_")"
  Set v=$Get(@n)
  If v=$c(0) {{ Set s="<c0>" }}
  ElseIf '(v?.E1C.E) {{ Set s=v }}
  Else {{ Set s="<bits:",p=0 For {{ Set p=$BitFind(v,1,p+1) Quit:'p  Set s=s_p_"," }} Set s=$Extract(s,1,*-1)_">" }}
  Write ref,"=",s,!
}}
Write "END",!
"#
    );
    let out = exec(r, &code).await;
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != "END")
        .map(str::to_string)
        .collect()
}

/// The planner's text for `query` (an `EXPLAIN` result, whitespace kept).
pub async fn plan(r: &Rig, query: &str) -> String {
    let code = format!(
        r#"Set st=##class(%SQL.Statement).%New()
Set rs=st.%ExecDirect(,{q})
While rs.%Next() {{ Write rs.%GetData(1),! }}
Write "END"
"#,
        q = os(&format!("EXPLAIN {query}"))
    );
    exec(r, &code).await
}

/// A class with the five columns the fixtures share (`Name`, `City`, `Age`, `Born`, `Status`), a
/// master data map over `^<global>` (pieces 1 to 5, `^` delimiter), and `extra_maps` appended inside
/// the Storage block. `extra` is anything added before `Storage` (an `Index` line, say).
pub fn person_class(class: &str, global: &str, extra: &str, extra_maps: &str) -> String {
    format!(
        r#"Class {class} Extends %Persistent [ StorageStrategy = SQLStorage ]
{{

Property Name As %String;

Property City As %String;

Property Age As %Integer;

Property Born As %Integer;

Property Status As %String;

{extra}
Storage SQLStorage
{{
<SqlIdExpression>$i(^{global})</SqlIdExpression>
<SQLMap name="Master">
<Data name="Age">
<Delimiter>"^"</Delimiter>
<Piece>3</Piece>
</Data>
<Data name="Born">
<Delimiter>"^"</Delimiter>
<Piece>4</Piece>
</Data>
<Data name="City">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Data name="Name">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="Status">
<Delimiter>"^"</Delimiter>
<Piece>5</Piece>
</Data>
<Global>^{global}</Global>
<Subscript name="1">
<Expression>{{ID}}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
{extra_maps}<StreamLocation>^{global}S</StreamLocation>
<Type>%Storage.SQL</Type>
}}

}}
"#
    )
}

/// One `SQLMap` of `Type index` over `global` with the given subscript expressions.
pub fn index_map(name: &str, global: &str, subs: &[&str], extra: &str) -> String {
    let mut s = format!("<SQLMap name=\"{name}\">\n{extra}<Global>^{global}</Global>\n");
    for (i, e) in subs.iter().enumerate() {
        s.push_str(&format!(
            "<Subscript name=\"{}\">\n<Expression>{}</Expression>\n</Subscript>\n",
            i + 1,
            e
        ));
    }
    s.push_str("<Type>index</Type>\n</SQLMap>\n");
    s
}

/// The shared rows: `(Name, City, Age, Born, Status)`. Names mix case, blanks, punctuation and
/// digits so each collation function gives a different subscript; none of Age or Born equals an ID.
#[allow(dead_code)]
pub fn people(n: usize) -> Vec<Vec<Value>> {
    let first = [
        "Smith, Anna",
        "jones, bob",
        "Lee  ",
        "O'Neil-Ray",
        "de la Cruz, Jo",
        "Zed2",
        "  Mc Kay, Al",
        "Brown, Cy",
        "brown, cy",
        "Ng",
        "Åberg, Eli",
        "Vu, Tran",
        "Quinn, 3rd",
        "Hart,Ed",
        "xu",
        "Patel, Raj",
        "Moore, Kim ",
        "Diaz, Lu",
        "Frost, Ann-Marie",
        "Gray, Sam",
    ];
    let cities = [
        "Boston", "boston", "Paris", "Lyon", "Oslo", "Rome", "Cork", "Bern", "Nice", "Kyiv",
    ];
    (0..n)
        .map(|i| {
            let name = if i < first.len() {
                first[i].to_string()
            } else {
                format!("{}{}", first[i % first.len()].trim(), i)
            };
            vec![
                json!(name),
                json!(cities[(i * 7 + 3) % cities.len()]),
                json!(18 + (i * 13) % 70 + 100),
                json!(40000 + (i as i64) * 37),
                json!(["A", "B", "C"][i % 3]),
            ]
        })
        .collect()
}
