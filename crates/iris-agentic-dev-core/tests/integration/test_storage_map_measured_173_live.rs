//! Spec 173 phase 0: the facts the design rests on, each measured on live IRIS (2026.2) with
//! classes invented for this repository. A test here is what `docs/storage-maps.md` cites for a
//! behaviour; a claim without its test is an assumption.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 nice -n 15 cargo test --features testing -j 2 \
//!   --test integration test_storage_map_measured_173 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage173_rig::*;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

async fn live() -> Rig {
    let r = rig(&Where::live(), Toolset::Merged).await;
    drop_all(&r).await;
    r
}

fn row(name: &str, city: &str, age: Value) -> Vec<Value> {
    vec![json!(name), json!(city), age, json!(40000), json!("A")]
}

const COLS: [&str; 5] = ["Name", "City", "Age", "Born", "Status"];

/// A class `IadSm173.<tag>` whose one index map files `^IadSm173<tag>X` by `exprs` (then `{ID}`).
fn indexed(tag: &str, exprs: &[&str], map_extra: &str) -> String {
    let mut subs: Vec<&str> = exprs.to_vec();
    subs.push("{ID}");
    person_class(
        &format!("IadSm173.{tag}"),
        &format!("IadSm173{tag}"),
        "",
        &index_map("M", &format!("IadSm173{tag}X"), &subs, map_extra),
    )
}

/// Compile a class with one map expression, INSERT `rows`, and dump the index global.
async fn filed(r: &Rig, tag: &str, exprs: &[&str], rows: &[Vec<Value>]) -> Vec<String> {
    put_class(r, &format!("IadSm173.{tag}"), &indexed(tag, exprs, "")).await;
    insert_rows(r, &format!("IadSm173.{tag}"), &COLS, rows).await;
    dump_global(r, &format!("IadSm173{tag}X")).await
}

fn has(lines: &[String], global_tag: &str, sub: &str) -> bool {
    let want = format!("^IadSm173{global_tag}X({},", sub);
    lines.iter().any(|l| l.starts_with(&want))
}

#[tokio::test]
#[ignore]
async fn fact_1_closed_set_of_collation_functions() {
    let r = live().await;
    let valid = [
        "$$SQLUPPER({Name})",
        "$$SQLSTRING({Name})",
        "$$STRING({Name})",
        "$$ALPHAUP({Name})",
        "$$UPPER({Name})",
        "$$MVR({Name})",
        "$$TRUNCATE({Name},3)",
    ];
    let invalid = [
        "$$EXACT({Name})",
        "$$PLUS({Name})",
        "$$MINUS({Name})",
        "$$SPACE({Name})",
    ];
    for (i, e) in valid.iter().enumerate() {
        let tag = format!("V{i}");
        let (ok, answer) =
            try_class(&r, &format!("IadSm173.{tag}"), &indexed(&tag, &[e], "")).await;
        assert!(ok, "{e} must compile: {answer}");
    }
    for (i, e) in invalid.iter().enumerate() {
        let tag = format!("W{i}");
        let (ok, answer) =
            try_class(&r, &format!("IadSm173.{tag}"), &indexed(&tag, &[e], "")).await;
        assert!(!ok, "{e} must not compile as a map expression: {answer}");
    }
    drop_all(&r).await;
}

#[tokio::test]
#[ignore]
async fn fact_2_stored_forms() {
    let r = live().await;
    let name = "  Smith, Anna-2  ";
    // (tag, expression, the subscript IRIS files for `name`, written out by hand from the
    // function's definition: one prefixed space for SQL collations, trailing blanks dropped).
    let cases: [(&str, &str, &str); 7] = [
        ("S1", "$$SQLUPPER({Name})", "   SMITH, ANNA-2"),
        ("S2", "$$SQLSTRING({Name})", "   Smith, Anna-2"),
        ("S3", "$$STRING({Name})", " SMITH,ANNA2"),
        ("S4", "$$ALPHAUP({Name})", "SMITH,ANNA2"),
        ("S5", "$$UPPER({Name})", "  SMITH, ANNA-2  "),
        ("S6", "$$TRUNCATE({Name},3)", "  S"),
        // MVR files a sortable form: the digit run becomes `$c(1)_"612"_$c(1)`. Pinned as measured; it
        // cannot be derived from the function's name, and fact_9 compares it with the function class.
        ("S7", "$$MVR({Name})", "   Smith, Anna-<1>612<1>  "),
    ];
    let mut seen = Vec::new();
    for (tag, expr, _) in &cases {
        let lines = filed(&r, tag, &[expr], &[row(name, "c", json!(1))]).await;
        assert_eq!(lines.len(), 1, "{expr}: {lines:?}");
        seen.push(lines[0].clone());
    }
    for ((tag, expr, want), got) in cases.iter().zip(&seen) {
        assert!(
            has(std::slice::from_ref(got), tag, &format!("\"{want}\"")),
            "{expr}: got {got}, want {want:?}"
        );
    }
    drop_all(&r).await;
}

#[tokio::test]
#[ignore]
async fn fact_3_length_variant() {
    let r = live().await;
    let lines = filed(
        &r,
        "L1",
        &["$$SQLUPPER({Name},4)"],
        &[row("Smith, Anna", "c", json!(1))],
    )
    .await;
    // The length counts the prefixed space: one space and three characters.
    assert!(has(&lines, "L1", "\" SMI\""), "{lines:?}");
    drop_all(&r).await;
}

#[tokio::test]
#[ignore]
async fn fact_4_name_piece() {
    let r = live().await;
    let lines = filed(
        &r,
        "P1",
        &["$p($$SQLUPPER({Name}),\",\",1)"],
        &[row("Smith, Anna", "c", json!(1))],
    )
    .await;
    assert!(has(&lines, "P1", "\" SMITH\""), "{lines:?}");
    drop_all(&r).await;
}

#[tokio::test]
#[ignore]
async fn fact_5_null_markers() {
    let r = live().await;
    // A map-compiled index: an empty string, an empty number, and an empty string under SQLUPPER.
    // Row 1 holds an empty string and an empty number (a NULL); row 2 holds a NULL in both.
    let rows = [
        row("n", "", Value::Null),
        vec![
            json!("m"),
            Value::Null,
            Value::Null,
            json!(40000),
            json!("A"),
        ],
    ];
    let city = filed(&r, "N1", &["{City}"], &rows).await;
    eprintln!("N1 {{City}}: {city:?}");
    assert!(
        has(&city, "N1", "\"<c0>\""),
        "empty string under {{City}}: {city:?}"
    );
    let age = filed(&r, "N2", &["{Age}"], &rows).await;
    eprintln!("N2 {{Age}}: {age:?}");
    assert!(has(&age, "N2", "-100000000000000"), "empty number: {age:?}");
    let upper = filed(&r, "N3", &["$$SQLUPPER({City})"], &rows).await;
    assert!(
        has(&upper, "N3", "\" <c0>\""),
        "empty string under SQLUPPER: {upper:?}"
    );
    // A class-compiled index (default storage) files a single space for an empty string.
    let cc = "Class IadSm173.Cc Extends %Persistent\n{\nProperty City As %String;\n\nIndex CityIdx On City;\n}\n";
    put_class(&r, "IadSm173.Cc", cc).await;
    insert_rows(
        &r,
        "IadSm173.Cc",
        &["City"],
        &[vec![json!("")], vec![Value::Null]],
    )
    .await;
    let dumped = dump_global(&r, "IadSm173.CcI").await;
    eprintln!("Cc: {dumped:?}");
    // An empty string files a space and `$c(0)`; a NULL files a lone space.
    assert!(
        dumped
            .iter()
            .any(|l| l.starts_with("^IadSm173.CcI(\"CityIdx\",\" <c0>\",1)")),
        "class-compiled empty string: {dumped:?}"
    );
    assert!(
        dumped
            .iter()
            .any(|l| l.starts_with("^IadSm173.CcI(\"CityIdx\",\" \",2)")),
        "class-compiled NULL: {dumped:?}"
    );
    // A subscript with its own NullMarker files that marker.
    let marked = person_class(
        "IadSm173.N4",
        "IadSm173N4",
        "",
        "<SQLMap name=\"M\">\n<Global>^IadSm173N4X</Global>\n<Subscript name=\"1\">\n<Expression>{City}</Expression>\n<NullMarker>\"~NIL~\"</NullMarker>\n</Subscript>\n<Subscript name=\"2\">\n<Expression>{ID}</Expression>\n</Subscript>\n<Type>index</Type>\n</SQLMap>\n",
    );
    put_class(&r, "IadSm173.N4", &marked).await;
    insert_rows(&r, "IadSm173.N4", &COLS, &rows).await;
    let own = dump_global(&r, "IadSm173N4X").await;
    assert!(has(&own, "N4", "\"~NIL~\""), "own marker: {own:?}");
    drop_all(&r).await;
}

#[tokio::test]
#[ignore]
async fn fact_6_planner_words() {
    let r = live().await;
    let rows = [row("bob", "c", json!(1)), row("Smith", "c", json!(2))];
    let q = |t: &str| format!("SELECT %ID FROM IadSm173.{t} WHERE Name = 'bob'");
    filed(&r, "Q1", &["{Name}"], &rows).await;
    filed(&r, "Q2", &["$$SQLUPPER({Name})"], &rows).await;
    filed(&r, "Q3", &["$$SQLUPPER({Name},4)"], &rows).await;
    let raw = plan(&r, &q("Q1")).await;
    let up = plan(&r, &q("Q2")).await;
    let len = plan(&r, &q("Q3")).await;
    eprintln!("RAW PLAN:\n{raw}\nUPPER PLAN:\n{up}\nLEN PLAN:\n{len}");
    assert!(raw.contains("no index subscript collation"), "{raw}");
    // "Used" is the planner reading the map WITH the given value; a plain `Read index map ...,
    // looping on` is a scan of the whole index and comes with the warning.
    assert!(
        up.contains("Read index map IadSm173.Q2.M, using the given %SQLUPPER(Name)"),
        "{up}"
    );
    assert!(!up.contains("cannot be used"), "{up}");
    assert!(
        raw.contains("Read index map IadSm173.Q1.M, looping on"),
        "{raw}"
    );
    assert!(!raw.contains("using the given"), "{raw}");
    assert!(len.contains("%SQLUPPER(4)"), "{len}");
    drop_all(&r).await;
}

/// The class's `Index` line and a bitmap map over `^IadSm173BmX` with `subs` as its subscripts.
fn bitmap_map(subs: &[&str]) -> (String, String) {
    let mut map = String::from("<SQLMap name=\"MBit\">\n<Global>^IadSm173BmX</Global>\n");
    for (i, e) in subs.iter().enumerate() {
        map.push_str(&format!(
            "<Subscript name=\"{}\">\n<Expression>{e}</Expression>\n</Subscript>\n",
            i + 1
        ));
    }
    map.push_str("<Type>bitmap</Type>\n</SQLMap>\n");
    ("Index MBit On Status [ Type = bitmap ];\n".to_string(), map)
}

fn status_row(status: &str) -> Vec<Value> {
    vec![
        json!("n"),
        json!("c"),
        json!(1),
        json!(40000),
        json!(status),
    ]
}

/// Compile a map-compiled bitmap class over Status with `subs`, insert one row per entry of
/// `statuses` (`"A@64000"` first sets the ID counter to 64000), and dump the bitmap global.
async fn bitmap_dump(r: &Rig, subs: &[&str], statuses: &[&str]) -> Vec<String> {
    let (idx, map) = bitmap_map(subs);
    put_class(
        r,
        "IadSm173.Bm",
        &person_class("IadSm173.Bm", "IadSm173Bm", &idx, &map),
    )
    .await;
    for st in statuses {
        let (status, jump) = match st.split_once('@') {
            Some((s, n)) => (s, Some(n)),
            None => (*st, None),
        };
        if let Some(n) = jump {
            exec(r, &format!("Set ^IadSm173Bm={n} Write \"SET\"")).await;
        }
        insert_rows(r, "IadSm173.Bm", &COLS, &[status_row(status)]).await;
    }
    dump_global(r, "IadSm173BmX").await
}

/// The IDs a dump of bit strings names: ID = (chunk - 1) * 64000 + position - 1, the chunk being
/// the last subscript. Worked out here, independently of the server.
fn decoded_ids(lines: &[String]) -> Vec<u64> {
    let mut ids = Vec::new();
    for l in lines {
        let (reference, value) = l.split_once('=').unwrap();
        let chunk: u64 = reference
            .trim_end_matches(')')
            .rsplit(',')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let bits = value.trim_start_matches("<bits:").trim_end_matches('>');
        for p in bits.split(',').filter(|p| !p.is_empty()) {
            ids.push((chunk - 1) * 64000 + p.parse::<u64>().unwrap() - 1);
        }
    }
    ids.sort_unstable();
    ids
}

#[tokio::test]
#[ignore]
async fn fact_7_bitmap_decode() {
    let r = live().await;
    // IDs 1, 2, 3 (A, B, A), then the counter jumps so the next row is ID 64001 (chunk 2).
    let lines = bitmap_dump(&r, &["{Status}"], &["A", "B", "A", "A@64000"]).await;
    assert_eq!(
        lines,
        vec![
            "^IadSm173BmX(\"A\",1)=<bits:2,4>",
            "^IadSm173BmX(\"A\",2)=<bits:2>",
            "^IadSm173BmX(\"B\",1)=<bits:3>",
        ],
        "{lines:?}"
    );
    assert_eq!(decoded_ids(&lines), vec![1, 2, 3, 64001]);
    drop_all(&r).await;
}

#[tokio::test]
#[ignore]
async fn fact_8_chunk_data_map() {
    use crate::storage172_rig::{ask, drop_fixture, load_fixture, Fixture};
    let r = live().await;
    let f = Fixture::read("Bits");
    load_fixture(&r, &f).await;
    let rows = ask(&r, &f).await;
    let problems = drop_fixture(&r, &f).await;
    assert!(problems.is_empty(), "{problems:?}");
    // Bits 3, 5 and 8 of the seeded string are IDs 2, 4 and 7 (bit 1 is ID 0).
    let words: Vec<i64> = rows.iter().filter_map(|x| x["Word"].as_i64()).collect();
    assert_eq!(words, vec![2, 4, 7], "{rows:?}");
}

#[tokio::test]
#[ignore]
async fn fact_9_function_class_equals_the_filed_subscript() {
    let r = live().await;
    let names = ["  Smith, Anna-2  ", "jones, Bob", "Zed2", "Ng"];
    // (tag, map expression, the same call on the documented function class)
    let cases = [
        ("G1", "$$SQLUPPER({Name})", "SQLUPPER(v)"),
        ("G2", "$$SQLSTRING({Name})", "SQLSTRING(v)"),
        ("G3", "$$STRING({Name})", "STRING(v)"),
        ("G4", "$$ALPHAUP({Name})", "ALPHAUP(v)"),
        ("G5", "$$UPPER({Name})", "UPPER(v)"),
        ("G6", "$$MVR({Name})", "MVR(v)"),
        ("G7", "$$TRUNCATE({Name},3)", "TRUNCATE(v,3)"),
        ("G8", "$$SQLUPPER({Name},4)", "SQLUPPER(v,4)"),
        ("G9", "$$SQLSTRING({Name},4)", "SQLSTRING(v,4)"),
    ];
    let rows: Vec<Vec<Value>> = names.iter().map(|n| row(n, "c", json!(1))).collect();
    for (tag, expr, call) in cases {
        filed(&r, tag, &[expr], &rows).await;
        let code = format!(
            "For i=1:1:{n} {{ Set v=$Piece(^IadSm173{tag}(i),\"^\",1) Set k=##class(%SYSTEM.SQL.Functions).{call} Write $Data(^IadSm173{tag}X(k,i)),\",\" }} Write \"END\"",
            n = names.len()
        );
        let out = exec(&r, &code).await;
        assert!(
            out.contains("1,1,1,1,END"),
            "{expr}: the function class must give the subscript the map filed for every row: {out}"
        );
    }
    drop_all(&r).await;
}

#[tokio::test]
#[ignore]
async fn fact_10_bitmap_map_layout() {
    let r = live().await;
    // One subscript (the field): the layout is `(value, chunk)`, the shape of a class-compiled bitmap.
    let one = bitmap_dump(&r, &["{Status}"], &["A", "B", "A"]).await;
    assert_eq!(
        one,
        vec![
            "^IadSm173BmX(\"A\",1)=<bits:2,4>",
            "^IadSm173BmX(\"B\",1)=<bits:3>"
        ],
        "{one:?}"
    );
    drop_all(&r).await;
    // A second subscript `{ID}` files one node per row ID, the chunk (here 1) appended after it.
    let with_id = bitmap_dump(&r, &["{Status}", "{ID}"], &["A", "B", "A"]).await;
    assert_eq!(
        with_id,
        vec![
            "^IadSm173BmX(\"A\",1,1)=<bits:2>",
            "^IadSm173BmX(\"A\",3,1)=<bits:4>",
            "^IadSm173BmX(\"B\",2,1)=<bits:3>",
        ],
        "{with_id:?}"
    );
    assert_eq!(
        decoded_ids(&with_id),
        vec![1, 2, 3],
        "the last subscript is the chunk"
    );
    drop_all(&r).await;
    // A second subscript `{ID}\64000+1` repeats the chunk: `(value, chunk, chunk)`.
    let chunked = bitmap_dump(
        &r,
        &["{Status}", "{ID}\\64000+1"],
        &["A", "B", "A", "A@64000"],
    )
    .await;
    assert_eq!(
        chunked,
        vec![
            "^IadSm173BmX(\"A\",1,1)=<bits:2,4>",
            "^IadSm173BmX(\"A\",2,2)=<bits:2>",
            "^IadSm173BmX(\"B\",1,1)=<bits:3>",
        ],
        "{chunked:?}"
    );
    assert_eq!(decoded_ids(&chunked), vec![1, 2, 3, 64001]);
    drop_all(&r).await;
}

#[tokio::test]
#[ignore]
async fn fact_11_id_in_the_node_value() {
    let r = live().await;
    let extra = "<Data name=\"Id\"/>\n<RowIdSpec name=\"1\">\n<Expression>{Id}</Expression>\n<Field>Id</Field>\n</RowIdSpec>\n";
    let map = index_map("M", "IadSm173IdX", &["$$SQLUPPER({Name})"], extra);
    put_class(
        &r,
        "IadSm173.Id",
        &person_class("IadSm173.Id", "IadSm173Id", "", &map),
    )
    .await;
    let rows = [
        row("Smith, Anna", "Boston", json!(34)),
        row("bob", "Paris", json!(7)),
        row("Jones", "Rome", json!(40)),
    ];
    insert_rows(&r, "IadSm173.Id", &COLS, &rows).await;
    let lines = dump_global(&r, "IadSm173IdX").await;
    assert_eq!(
        lines,
        vec![
            "^IadSm173IdX(\" BOB\")=2",
            "^IadSm173IdX(\" JONES\")=3",
            "^IadSm173IdX(\" SMITH, ANNA\")=1",
        ],
        "{lines:?}"
    );
    let got = select(
        &r,
        "SELECT %ID AS rid, City FROM IadSm173.Id WHERE Name = 'bob'",
    )
    .await;
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0]["rid"], json!(2), "{got:?}");
    assert_eq!(got[0]["City"], json!("Paris"), "{got:?}");
    let p = plan(&r, "SELECT %ID FROM IadSm173.Id WHERE Name = 'bob'").await;
    assert!(p.contains("Read index map IadSm173.Id.M"), "{p}");
    drop_all(&r).await;
}

fn conditional(tag: &str, condition: &str, fields: &str) -> String {
    let extra = format!(
        "<Condition>{condition}</Condition>\n<ConditionFields>{fields}</ConditionFields>\n<PopulationType>conditional</PopulationType>\n"
    );
    let field = if fields == "City" {
        "{City}"
    } else {
        "{Status}"
    };
    let map = index_map("M", &format!("IadSm173{tag}X"), &[field, "{ID}"], &extra);
    person_class(
        &format!("IadSm173.{tag}"),
        &format!("IadSm173{tag}"),
        "",
        &map,
    )
}

#[tokio::test]
#[ignore]
async fn fact_12_conditional_filing() {
    let r = live().await;
    // Condition on a value: rows with status B are not filed.
    put_class(
        &r,
        "IadSm173.Cd1",
        &conditional("Cd1", "{Status}=\"A\"", "Status"),
    )
    .await;
    let rows = [
        row("a", "x", json!(1)),
        vec![json!("b"), json!("x"), json!(1), json!(40000), json!("B")],
        row("c", "x", json!(1)),
    ];
    insert_rows(&r, "IadSm173.Cd1", &COLS, &rows).await;
    let one = dump_global(&r, "IadSm173Cd1X").await;
    assert_eq!(
        one,
        vec!["^IadSm173Cd1X(\"A\",1)=", "^IadSm173Cd1X(\"A\",3)="],
        "{one:?}"
    );
    // `{City}'=""` skips a NULL city and does not skip an empty string (SQL stores that as $c(0)).
    put_class(
        &r,
        "IadSm173.Cd2",
        &conditional("Cd2", "{City}'=\"\"", "City"),
    )
    .await;
    let rows = [
        row("a", "Boston", json!(1)),
        vec![json!("b"), Value::Null, json!(1), json!(40000), json!("A")],
        row("c", "", json!(1)),
    ];
    insert_rows(&r, "IadSm173.Cd2", &COLS, &rows).await;
    let two = dump_global(&r, "IadSm173Cd2X").await;
    assert_eq!(
        two,
        vec!["^IadSm173Cd2X(\"<c0>\",3)=", "^IadSm173Cd2X(\"Boston\",1)="],
        "{two:?}"
    );
    drop_all(&r).await;
}

#[tokio::test]
#[ignore]
async fn r4_collate_forms() {
    let r = live().await;
    let rows = [
        row("Smith, Anna", "c", json!(1)),
        row("Jones", "c", json!(2)),
    ];
    let tr = filed(&r, "C1", &["$$COLLATE({Name},\"SQLUPPER\")"], &rows).await;
    assert!(
        has(&tr, "C1", "\"Jones\"") && has(&tr, "C1", "\"Smith, Anna\""),
        "{tr:?}"
    );
    let bare = filed(&r, "C2", &["$$COLLATE({Name})"], &rows).await;
    assert!(has(&bare, "C2", "\"Jones\""), "{bare:?}");
    let (ok, answer) = try_class(
        &r,
        "IadSm173.C3",
        &indexed("C3", &["$$COLLATE({Name},7)"], ""),
    )
    .await;
    assert!(!ok, "a numeric second argument must not compile: {answer}");
    assert!(
        answer.contains("Truncate-length") || answer.contains("COLLATE"),
        "{answer}"
    );
    let space = filed(&r, "C4", &["\" \"_{Name}"], &rows).await;
    assert!(
        has(&space, "C4", "\" Jones\"") && has(&space, "C4", "\" Smith, Anna\""),
        "{space:?}"
    );
    drop_all(&r).await;
}

#[tokio::test]
#[ignore]
async fn r6_multipart_key() {
    let r = live().await;
    let cls = r#"Class IadSm173.Key Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property A As %String;

Property B As %Integer;

Property Name As %String;

Index IDKEY On (A, B) [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="Name">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Global>^IadSm173Key</Global>
<RowIdSpec name="1">
<Expression>{L1}</Expression>
<Field>A</Field>
</RowIdSpec>
<RowIdSpec name="2">
<Expression>{L2}</Expression>
<Field>B</Field>
</RowIdSpec>
<Subscript name="1">
<Expression>{A}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{B}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<SQLMap name="MIdx">
<Global>^IadSm173KeyX</Global>
<Subscript name="1">
<Expression>$$SQLUPPER({Name})</Expression>
</Subscript>
<Subscript name="2">
<Expression>{A}</Expression>
</Subscript>
<Subscript name="3">
<Expression>{B}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<StreamLocation>^IadSm173KeyS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
"#;
    put_class(&r, "IadSm173.Key", cls).await;
    let rows = [
        vec![json!("x"), json!(1), json!("Smith")],
        vec![json!("x"), json!(2), json!("smith")],
        vec![json!("y"), json!(1), json!("Jones")],
    ];
    insert_rows(&r, "IadSm173.Key", &["A", "B", "Name"], &rows).await;
    let lines = dump_global(&r, "IadSm173KeyX").await;
    assert_eq!(
        lines,
        vec![
            "^IadSm173KeyX(\" JONES\",\"y\",1)=",
            "^IadSm173KeyX(\" SMITH\",\"x\",1)=",
            "^IadSm173KeyX(\" SMITH\",\"x\",2)=",
        ],
        "{lines:?}"
    );
    let got = select(
        &r,
        "SELECT A, B FROM IadSm173.Key WHERE Name = 'SMITH' ORDER BY B",
    )
    .await;
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(
        (got[0]["A"].clone(), got[0]["B"].clone()),
        (json!("x"), json!(1))
    );
    assert_eq!(
        (got[1]["A"].clone(), got[1]["B"].clone()),
        (json!("x"), json!(2))
    );
    drop_all(&r).await;
}

#[tokio::test]
#[ignore]
async fn open_listing_as_the_connection_user() {
    // The namespace listing is a documented class query that reads names only. This run measures it
    // as the rig's user; the answer for a user with no administrative role is recorded in
    // research.md (R5) and the failure path is tested with the candidates in phase 5.
    let r = live().await;
    exec(
        &r,
        "Set ^IadSm173OpenA=1,^IadSm173OpenA.x(\"k\")=2,^IadSm173Other=3 Write \"SET\"",
    )
    .await;
    let out = exec(
        &r,
        r#"Set rs=##class(%ResultSet).%New("%SYS.GlobalQuery:NameSpaceList")
Set sc=rs.Execute($Namespace,"IadSm173OpenA*",1,,,0)
If 'sc { Write "FAILED ",$System.Status.GetErrorText(sc),! Quit }
While rs.Next() { Write "NAME ",rs.Get("Name"),! }
Write "END"
"#,
    )
    .await;
    let mut names: Vec<&str> = out
        .lines()
        .filter_map(|l| l.trim().strip_prefix("NAME "))
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec!["IadSm173OpenA", "IadSm173OpenA.x"], "{out}");
    drop_all(&r).await;
}

#[tokio::test]
#[ignore]
async fn open_c0_condition() {
    let r = live().await;
    put_class(&r, "IadSm173.Cz", &conditional_on("Cz", "{City}'=$c(0)")).await;
    let rows = [row("a", "Rome", json!(1)), row("b", "", json!(1))];
    insert_rows(&r, "IadSm173.Cz", &COLS, &rows).await;
    let lines = dump_global(&r, "IadSm173CzX").await;
    // The empty-string row (ID 2) is skipped and the row with a city is filed.
    assert_eq!(lines, vec!["^IadSm173CzX(\"Rome\",1)="], "{lines:?}");
    drop_all(&r).await;
}

fn conditional_on(tag: &str, condition: &str) -> String {
    let extra = format!(
        "<Condition>{condition}</Condition>\n<ConditionFields>City</ConditionFields>\n<PopulationType>conditional</PopulationType>\n"
    );
    let map = index_map("M", &format!("IadSm173{tag}X"), &["{City}", "{ID}"], &extra);
    person_class(
        &format!("IadSm173.{tag}"),
        &format!("IadSm173{tag}"),
        "",
        &map,
    )
}

#[tokio::test]
#[ignore]
async fn open_spread_beats_prefix() {
    let r = live().await;
    // 1,000 unique names: the first 800 have no blank or hyphen (so UPPER and ALPHAUP agree on
    // them), the last 200 do. An index over $$UPPER can only be told from one over $$ALPHAUP by
    // the last 200.
    put_class(&r, "IadSm173.Sp", &indexed("Sp", &["$$UPPER({Name})"], "")).await;
    let rows: Vec<Vec<Value>> = (0..1000)
        .map(|i| {
            let name = if i < 800 {
                format!("Aaron{i:04}")
            } else {
                format!("Zed Quinn-{i:04}")
            };
            row(&name, "c", json!(i))
        })
        .collect();
    insert_rows(&r, "IadSm173.Sp", &COLS, &rows).await;
    let code = r#"Set g="^IadSm173SpX",n=0,k=""
For { Set k=$Order(@g@(k)) Quit:k=""  Set n=n+1,v(n)=k }
Set stride=n\200
For mode="prefix","spread" {
  Set c=0,u=0,a=0
  For j=1:1:200 {
    Set i=$Select(mode="prefix":j,1:(j-1)*stride+1)
    Set k=v(i),id=$Order(@g@(k,"")),name=$Piece(^IadSm173Sp(id),"^",1)
    Set c=c+1
    If ##class(%SYSTEM.SQL.Functions).UPPER(name)=k Set u=u+1
    If ##class(%SYSTEM.SQL.Functions).ALPHAUP(name)=k Set a=a+1
  }
  Write mode,":",c,",",u,",",a,!
}
Write "N=",n,!
"#;
    let out = exec(&r, code).await;
    assert!(out.contains("N=1000"), "{out}");
    // A prefix walk sees only the first 200 names: both functions explain every node, a tie.
    assert!(out.contains("prefix:200,200,200"), "{out}");
    // A walk with a stride sees the whole key range: only UPPER explains every node.
    let spread = out
        .lines()
        .find(|l| l.trim().starts_with("spread:"))
        .unwrap()
        .trim();
    let parts: Vec<u32> = spread["spread:".len()..]
        .split(',')
        .map(|x| x.parse().unwrap())
        .collect();
    assert_eq!(parts[0], 200, "{out}");
    assert_eq!(parts[1], 200, "UPPER explains every spread node: {out}");
    assert!(
        parts[2] < 200,
        "ALPHAUP must fail somewhere in the spread: {out}"
    );
    drop_all(&r).await;
}
