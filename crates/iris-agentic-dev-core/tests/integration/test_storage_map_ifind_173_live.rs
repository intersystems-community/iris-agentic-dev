//! Spec 173 T072: `propose`, `read`, `diagram` and `compare` over the globals a REAL `%iFind` index
//! files, and over Benjamin De Boe's `^bdb` / `^bdb.x` shape. Every earlier 173 fixture was invented;
//! none was an iFind index.
//!
//! The corpus is invented (`sentence`). A real iFind index (the basic form with `LANGUAGE = "en"`,
//! and Benjamin's analytic form made by DDL with `IFINDMAPPINGS = 1`) is built by IRIS itself, the
//! class deleted, and only the globals left for `propose`. Known answers come from the corpus
//! formulas below and from IRIS's own word dictionary, never from the answer under test.
//!
//! What a real iFind index files (measured on 2026.2, see `research.md`, "iFind real index (T072)"):
//! the master (`^<Class>D`) is a plain `$LIST` row; the index global (`^<Class>I`) holds `"$Doc"`
//! and `"Idx"` subtrees whose keys are word IDs, never words, and whose values are bit strings; the
//! words live in namespace-wide shared globals (`^IRIS.IF.WordD`, `^IRIS.IF.WordI`), and
//! `^IRIS.IF.TransfoB(0, chunk)` is the `%iFind.OriginalWord` map's `$BIT` global.
//!
//! T072 wrote the tests that stated what the user needs as `red_*` (they failed on purpose); T076
//! to T083 made them pass and dropped the prefix. Every test keeps `#[ignore]` like every live
//! test. The one assertion changed on the way is in
//! `bdb_the_sub_node_field_is_typed_from_its_own_values` (see its comment).
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 nice -n 15 cargo test --features testing -j 2 \
//!   --test integration test_storage_map_ifind_173 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::Rig;
use crate::storage173_rig::*;
use crate::test_storage_map_link_173_live::live;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const WORDS: [&str; 12] = [
    "amber", "harbor", "lantern", "orchard", "velvet", "copper", "meadow", "quill", "falcon",
    "tundra", "marble", "ember",
];

/// Row `i` (1-based) of the invented corpus: three words from `WORDS` around an `and`, the first
/// repeated at the end every fourth row, and a `ZephyrN!` word every tenth row.
fn sentence(i: usize) -> String {
    let mut s = format!(
        "{} {} and {}",
        WORDS[i % 12],
        WORDS[(i * 5 + 1) % 12],
        WORDS[(i * 7 + 2) % 12]
    );
    if i.is_multiple_of(4) {
        s.push(' ');
        s.push_str(WORDS[i % 12]);
    }
    if i.is_multiple_of(10) {
        s.push_str(&format!(" Zephyr{i}!"));
    }
    s
}

/// The 1-based positions of `word` in row `i` (a token `Zephyr20!` is the word `zephyr20`).
fn positions(i: usize, word: &str) -> Vec<u64> {
    sentence(i)
        .split_whitespace()
        .enumerate()
        .filter(|(_, t)| {
            let t = t.to_lowercase();
            t == word || t.trim_end_matches('!') == word
        })
        .map(|(p, _)| p as u64 + 1)
        .collect()
}

/// The rows (1..=60) whose sentence holds `word`.
fn rows_with(word: &str) -> Vec<u64> {
    (1..=60)
        .filter(|i| !positions(*i, word).is_empty())
        .map(|i| i as u64)
        .collect()
}

/// The token positions IRIS counts in row `i` (`P(row)`): the four tokens of the base sentence
/// (three words and `and`), the repeated word, and the `ZephyrN!` token (one position, filed under
/// two word IDs: `zephyrN` and `zephyrN!`).
fn word_count(i: usize) -> usize {
    4 + usize::from(i.is_multiple_of(4)) + usize::from(i.is_multiple_of(10))
}

/// Every distinct word the corpus gives the shared dictionary.
fn vocabulary() -> BTreeSet<String> {
    let mut v = BTreeSet::new();
    for i in 1..=60 {
        for t in sentence(i).split_whitespace() {
            let t = t.to_lowercase();
            v.insert(t.trim_end_matches('!').to_string());
            v.insert(t);
        }
    }
    v
}

const BASIC_CLASS: &str = "IadSm173.Ifind.Doc";
const BASIC_TEXT: &str = "Class IadSm173.Ifind.Doc Extends %Persistent\n{\n\nProperty Title As %String(MAXLEN = 60);\n\nProperty Txt As %String(MAXLEN = 400);\n\nIndex Idx On (Txt) As %iFind.Index.Basic(LANGUAGE = \"en\");\n\n}\n";
const BASIC_MASTER: &str = "^IadSm173.Ifind.DocD";
const BASIC_INDEX: &str = "^IadSm173.Ifind.DocI";
const DDL_CLASS: &str = "IadSm173Ifdd.IFind";

async fn delete_doc(r: &Rig, name: &str) {
    let _ = r
        .call(
            "iris_doc",
            json!({"mode": "delete", "name": format!("{name}.cls"), "namespace": "USER"}),
        )
        .await;
}

/// Refuse to touch iFind storage that holds a word this test did not insert, then kill every
/// `^IRIS.IF*` global (the namespace-wide iFind storage the test's own indexes created).
async fn kill_ifind_globals(r: &Rig) {
    let known: Vec<String> = vocabulary().into_iter().map(|w| os(&w)).collect();
    let code = format!(
        r#"Set ok=1,known=$ListBuild({known})
Set k="" For {{ Set k=$Order(^IRIS.IF.WordD(k),1,w) Quit:k=""  If '$ListFind(known,w) {{ Set ok=0 }} }}
If 'ok {{ Write "FOREIGN" Quit }}
Set rs=##class(%ResultSet).%New("%SYS.GlobalQuery:NameSpaceList")
Do rs.Execute($Namespace,"IRIS.IF*",1,,,0)
While rs.Next() {{ Set g="^"_rs.Get("Name") Kill @g }}
Write "KILLED"
"#,
        known = known.join(",")
    );
    let out = exec(r, &code).await;
    assert!(
        out.contains("KILLED"),
        "USER holds iFind words this test did not insert; it will not kill ^IRIS.IF*: {out}"
    );
}

/// Every global location the compiled class `name` declares in its storage.
async fn storage_globals(r: &Rig, name: &str) -> Vec<String> {
    let got = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": format!("{name}.cls"), "namespace": "USER"}),
        )
        .await;
    let text = got["content"].as_str().unwrap_or_default().to_string();
    let mut out = Vec::new();
    for tag in ["DataLocation", "Location", "IndexLocation"] {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let mut rest = text.as_str();
        while let Some(a) = rest.find(&open) {
            let from = a + open.len();
            let b = rest[from..].find(&close).expect("closing tag");
            let g = rest[from..from + b].to_string();
            if !out.contains(&g) {
                out.push(g);
            }
            rest = &rest[from + b..];
        }
    }
    out
}

async fn load_basic(r: &Rig) {
    put_class(r, BASIC_CLASS, BASIC_TEXT).await;
    let rows: Vec<Vec<Value>> = (1..=60)
        .map(|i| vec![json!(format!("t{i}")), json!(sentence(i))])
        .collect();
    insert_rows(r, "IadSm173_Ifind.Doc", &["Title", "Txt"], &rows).await;
    delete_doc(r, BASIC_CLASS).await;
}

/// Benjamin's recipe: a DDL table and an analytic iFind index with `IFINDMAPPINGS = 1`. Returns
/// `(master, index, ids)`: the globals IRIS chose (a hashed name, so they are read from the class)
/// and the row ID IRIS gave each sentence (`ids[i - 1]` is row `i`). A DDL table's IDs come from
/// `$SEQUENCE`, so they are not 1..60 and the first INSERT after the table is re-created over
/// killed globals can fail with `<ILLEGAL VALUE>` (measured, not iFind-related): the loader retries
/// that row and asks IRIS for the IDs.
async fn load_analytic(r: &Rig) -> (String, String, Vec<u64>) {
    let code = r#"Set st=##class(%SQL.Statement).%New()
Set rs=st.%ExecDirect(,"create table IadSm173Ifdd.IFind (str varchar(10000))")
Write "T",rs.%SQLCODE,!
Set rs=st.%ExecDirect(,"create index AnalyticIDX on IadSm173Ifdd.IFind(str) as %iFind.Index.Analytic(IFINDMAPPINGS = 1)")
Write "I",rs.%SQLCODE,!
"#;
    let out = exec(r, code).await;
    assert!(out.contains("T0") && out.contains("I0"), "ddl: {out}");
    let globals = storage_globals(r, DDL_CLASS).await;
    let base = globals
        .iter()
        .find(|g| g.ends_with(".1"))
        .map(|g| g.trim_end_matches(".1").to_string())
        .expect("rows global");
    exec(r, &format!("Set {MARKER}={}", os(&base))).await;
    for i in 1..=60 {
        let code = format!(
            r#"Set st=##class(%SQL.Statement).%New()
For try=1:1:3 {{ Set rs=st.%ExecDirect(,"INSERT INTO IadSm173Ifdd.IFind (str) VALUES (?)",{}) Quit:rs.%SQLCODE=0 }}
Write "SQLCODE ",rs.%SQLCODE
"#,
            os(&sentence(i))
        );
        let out = exec(r, &code).await;
        assert!(out.contains("SQLCODE 0"), "row {i}: {out}");
    }
    // The corpus repeats a sentence every twelve rows, so a row is found by its place in the order
    // of the IDs (one process inserted them, and `$SEQUENCE` only goes up).
    let table = select(
        r,
        "SELECT %ID AS id, str FROM IadSm173Ifdd.IFind ORDER BY %ID",
    )
    .await;
    assert_eq!(table.len(), 60, "rows in the DDL table");
    let mut ids = Vec::new();
    for (n, row) in table.iter().enumerate() {
        assert_eq!(row["str"], json!(sentence(n + 1)), "row {}", n + 1);
        ids.push(
            row["id"]
                .as_u64()
                .or_else(|| row["id"].as_str().and_then(|s| s.parse().ok()))
                .expect("numeric id"),
        );
    }
    delete_doc(r, DDL_CLASS).await;
    // The table's globals are `^<ext>.1` (rows), `.2` (extent bitmap) and `.3` (the iFind index).
    let pick = |suffix: &str| {
        globals
            .iter()
            .find(|g| g.ends_with(suffix))
            .cloned()
            .unwrap_or_else(|| panic!("no {suffix} global in {globals:?}"))
    };
    (pick(".1"), pick(".3"), ids)
}

/// The marker global (its name matches no `IadSm173*` mask, so `drop_all` leaves it) that records
/// the DDL table's hashed globals, so a run that died before its cleanup is cleaned by the next.
const MARKER: &str = "^IadIfT072";

/// Kill the DDL table's globals (`^<ext>`, `.1`, `.2`, `.3`, `.I`, `.S`) the marker names.
async fn kill_ddl_globals(r: &Rig) {
    let code = format!(
        r#"Set base=$Get({MARKER}) If base'="" {{ For s="",".1",".2",".3",".I",".S" {{ Set g=base_s Kill @g }} }} Kill {MARKER}
Write "DDLKILLED"
"#
    );
    assert!(exec(r, &code).await.contains("DDLKILLED"));
}

async fn clean(r: &Rig) {
    drop_all(r).await;
    delete_doc(r, DDL_CLASS).await;
    kill_ddl_globals(r).await;
    kill_ifind_globals(r).await;
}

async fn start() -> Rig {
    let r = live().await;
    clean(&r).await;
    r
}

/// The positions of the set bits of the bit string at `reference` (a full global reference).
async fn bits(r: &Rig, reference: &str) -> Vec<u64> {
    let code = format!(
        r#"Set v=$Get({reference}) Set p=0
For {{ Set p=$BitFind(v,1,p+1) Quit:'p  Write p," " }}
Write "END"
"#
    );
    exec(r, &code)
        .await
        .split_whitespace()
        .filter(|t| *t != "END")
        .map(|t| t.parse().unwrap_or_else(|_| panic!("bit position {t:?}")))
        .collect()
}

async fn value(r: &Rig, reference: &str) -> String {
    exec(r, &format!("Write $Get({reference})")).await
}

/// The ID IRIS's own word dictionary gave `word`.
async fn word_id(r: &Rig, word: &str) -> u64 {
    let v = value(r, &format!("^IRIS.IF.WordI({})", os(&format!(" {word}")))).await;
    v.trim()
        .parse()
        .unwrap_or_else(|_| panic!("no word id for {word}: {v:?}"))
}

/// The next subscripts under `global(prefix)` (`prefix` is ObjectScript subscripts, may be empty).
async fn subs(r: &Rig, global: &str, prefix: &str) -> Vec<String> {
    let at = if prefix.is_empty() {
        format!("{global}(k)")
    } else {
        format!("{global}({prefix},k)")
    };
    let code = format!(
        r#"Set k="" For {{ Set k=$Order({at}) Quit:k=""  Write k,"|" }}
Write "END"
"#
    );
    exec(r, &code)
        .await
        .split('|')
        .filter(|s| !s.is_empty() && *s != "END")
        .map(str::to_string)
        .collect()
}

/// The corpus words an answer must never repeat (`and` is a word of the language, so it is left out).
fn corpus_words() -> BTreeSet<String> {
    let mut v = vocabulary();
    v.remove("and");
    v
}

/// Every alphanumeric token of the answer, to look for a sampled word.
fn tokens(v: &Value) -> BTreeSet<String> {
    v.to_string()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// A word the corpus holds that the answer repeats.
fn leaked(v: &Value, words: &BTreeSet<String>) -> Vec<String> {
    tokens(v).intersection(words).cloned().collect()
}

/// What a read-only call must leave alone: the node count of each named global, the globals named
/// `IadSm173*`, and the classes of the two test packages.
async fn snapshot(r: &Rig, globals: &[&str]) -> String {
    let mut code = String::new();
    for g in globals {
        code.push_str(&format!(
            "Set g={q},n=0,x=g If $Data(@g)#2 {{ Set n=1 }}\nFor {{ Set x=$Query(@x) Quit:x=\"\"  Set n=n+1 }}\nWrite g,\"=\",n,!\n",
            q = os(g)
        ));
    }
    code.push_str(
        r#"Set rs=##class(%ResultSet).%New("%SYS.GlobalQuery:NameSpaceList")
Do rs.Execute($Namespace,"IadSm173*",1,,,0)
While rs.Next() { Write "global ",rs.Get("Name"),! }
"#,
    );
    let mut out = exec(r, &code).await;
    for pattern in ["IadSm173.*", "IadSm173Ifdd.*"] {
        let listed = r
            .call(
                "iris_doc",
                json!({"mode": "list", "pattern": pattern, "category": "CLS", "namespace": "USER"}),
            )
            .await;
        out.push_str(&format!("\n{pattern}: {}", listed["documents"]));
    }
    out
}

fn link<'a>(out: &'a Value, global: &str) -> &'a Value {
    out["indexes"]
        .as_array()
        .and_then(|a| a.iter().find(|l| l["global"] == json!(global)))
        .unwrap_or_else(|| panic!("no link for {global} in {}", out["indexes"]))
}

async fn propose(r: &Rig, args: Value) -> Value {
    let out = ask_propose(r, args.clone()).await;
    assert_eq!(out["success"], true, "propose {args}: {out}");
    out
}

// ---- the layout a real iFind index files ---------------------------------------------------------

/// The basic index files word IDs and bit strings, never words, and keeps the words in the shared
/// dictionary. Every number below comes from the corpus formulas and IRIS's own dictionary.
#[tokio::test]
#[ignore]
async fn the_basic_ifind_index_files_word_ids_and_bit_strings() {
    let r = start().await;
    load_basic(&r).await;
    // Both classes' globals and nothing else filed under the class prefix.
    let listed = exec(
        &r,
        r#"Set rs=##class(%ResultSet).%New("%SYS.GlobalQuery:NameSpaceList")
Do rs.Execute($Namespace,"IadSm173*",1,,,0)
While rs.Next() { Write rs.Get("Name"),"|" }
"#,
    )
    .await;
    let mut names: Vec<&str> = listed.split('|').filter(|s| !s.is_empty()).collect();
    names.sort();
    assert_eq!(
        names,
        ["IadSm173.Ifind.DocD", "IadSm173.Ifind.DocI"],
        "globals filed by the class"
    );
    // The master holds the ID counter and one $LIST row per ID.
    assert_eq!(value(&r, "^IadSm173.Ifind.DocD").await.trim(), "60");
    assert!(value(&r, "^IadSm173.Ifind.DocD(7)").await.contains("t7"));
    // Level 1 of the index global is "$Doc" and "Idx": no word is a subscript.
    assert_eq!(subs(&r, BASIC_INDEX, "").await, vec!["$Doc", "Idx"]);
    assert_eq!(subs(&r, BASIC_INDEX, "\"Idx\"").await, vec!["L", "P", "W"]);
    // "$Doc" and "L" hold one bit string whose set bits are the row IDs: bit n+1 is row n.
    let all_rows: Vec<u64> = (2..=61).collect();
    assert_eq!(bits(&r, "^IadSm173.Ifind.DocI(\"$Doc\",1)").await, all_rows);
    assert_eq!(
        bits(&r, "^IadSm173.Ifind.DocI(\"Idx\",\"L\",1,1)").await,
        all_rows
    );
    // W(wordID) is the count of rows holding the word, W(wordID, chunk) the bit string of the rows.
    for word in ["harbor", "ember", "zephyr30", "zephyr30!"] {
        let id = word_id(&r, word).await;
        let want: Vec<u64> = rows_with(word).iter().map(|row| row + 1).collect();
        assert!(!want.is_empty(), "{word}");
        assert_eq!(
            bits(&r, &format!("^IadSm173.Ifind.DocI(\"Idx\",\"W\",{id},1)")).await,
            want,
            "rows holding {word}"
        );
        assert_eq!(
            value(&r, &format!("^IadSm173.Ifind.DocI(\"Idx\",\"W\",{id})"))
                .await
                .trim(),
            want.len().to_string(),
            "row count of {word}"
        );
    }
    // P(row) is the row's word count, P(row, wordID, chunk) the bit string of the word's positions.
    for row in [7usize, 20] {
        assert_eq!(
            value(&r, &format!("^IadSm173.Ifind.DocI(\"Idx\",\"P\",{row})"))
                .await
                .trim(),
            word_count(row).to_string(),
            "word count of row {row}"
        );
    }
    let falcon = word_id(&r, "falcon").await;
    let want: Vec<u64> = positions(20, "falcon").iter().map(|p| p + 1).collect();
    assert_eq!(want, vec![2, 6]);
    assert_eq!(
        bits(
            &r,
            &format!("^IadSm173.Ifind.DocI(\"Idx\",\"P\",20,{falcon},1)")
        )
        .await,
        want,
        "positions of falcon in row 20"
    );
    clean(&r).await;
}

/// Benjamin's analytic form: the table's globals are hashed names that do not share a prefix with
/// each other beyond the extent, and the index global holds more subtrees than the basic one.
#[tokio::test]
#[ignore]
async fn the_analytic_ifind_index_of_a_ddl_table_files_the_same_kind_of_layout() {
    let r = start().await;
    let (master, index, ids) = load_analytic(&r).await;
    assert!(
        master.ends_with(".1") && index.ends_with(".3"),
        "{master} {index}"
    );
    assert!(
        !master.contains("IadSm173"),
        "the DDL globals are hashed: {master}"
    );
    // The master: one $LIST row per ID holding the text (the one piece).
    assert_eq!(
        exec(&r, &format!("Write $ListGet({master}({}),1)", ids[4])).await,
        sentence(5)
    );
    // The index global's level 1: the subtrees IRIS files for an analytic index.
    let top = subs(&r, &index, "").await;
    for want in ["E", "L", "P", "W", "c", "d", "p", "x", "xio"] {
        assert!(top.iter().any(|t| t == want), "no {want} in {top:?}");
    }
    let id = word_id(&r, "harbor").await;
    let mut want: Vec<u64> = rows_with("harbor")
        .iter()
        .map(|row| ids[*row as usize - 1] + 1)
        .collect();
    want.sort_unstable();
    assert_eq!(bits(&r, &format!("{index}(\"W\",{id},1)")).await, want);
    // `^IRIS.IF.TransfoB(0, chunk)`: bit n+1 is word ID n, for every word of the dictionary.
    let n = vocabulary().len() as u64;
    assert_eq!(n, 25);
    assert_eq!(value(&r, "^IRIS.IF.WordD").await.trim(), n.to_string());
    assert_eq!(
        bits(&r, "^IRIS.IF.TransfoB(0,1)").await,
        (2..=n + 1).collect::<Vec<_>>()
    );
    clean(&r).await;
}

// ---- what `propose` says about it ------------------------------------------------------------

/// Pointing `propose` at the master and at the iFind index global is safe: no error, no word of the
/// corpus in any answer, nothing written, and no map drawn for the iFind index.
#[tokio::test]
#[ignore]
async fn propose_over_a_basic_ifind_index_leaks_no_word_writes_nothing_and_draws_no_link() {
    let r = start().await;
    load_basic(&r).await;
    let watched = [
        BASIC_MASTER,
        BASIC_INDEX,
        "^IRIS.IF.WordD",
        "^IRIS.IF.WordI",
    ];
    let before = snapshot(&r, &watched).await;
    let words = corpus_words();
    let alone = propose(&r, json!({"global": BASIC_MASTER})).await;
    let linked = propose(
        &r,
        json!({"global": BASIC_MASTER, "index_globals": [BASIC_INDEX]}),
    )
    .await;
    let index_only = propose(&r, json!({"global": BASIC_INDEX})).await;
    for (what, out) in [
        ("master alone", &alone),
        ("master and index", &linked),
        ("index alone", &index_only),
    ] {
        assert_eq!(leaked(out, &words), Vec::<String>::new(), "{what}");
    }
    // The master is drawn from the `$LIST` rows: three pieces (the class name, Title, Txt).
    assert_eq!(
        alone["shape"]["formats"][1]["format"], "$LIST",
        "{}",
        alone["shape"]
    );
    assert_eq!(alone["shape"]["formats"][1]["pieces"]["max"], 3);
    assert_eq!(alone["shape"]["numeric_subscripts"]["1"], 60);
    // A real iFind index is not a link `propose` can draw: whatever it says, it is not "inferred"
    // and it carries no drafted map.
    let l = link(&linked, BASIC_INDEX);
    assert_ne!(l["verdict"], "inferred", "{l}");
    assert!(
        l.get("map").is_none(),
        "an iFind index got a drafted map: {l}"
    );
    assert_eq!(snapshot(&r, &watched).await, before, "propose wrote");
    clean(&r).await;
}

/// The linker's answer for the iFind index says nothing false about it. The known facts: every one
/// of the 60 rows is in the index ("$Doc" and "L" hold all 60 IDs), so no rule can say the index
/// holds "only the rows where one field has one value"; and subscript position 5 of the `"P"`
/// nodes is the chunk number, always 1, so it is not where the row ID sits.
#[tokio::test]
#[ignore]
async fn the_link_for_a_basic_ifind_index_makes_no_false_claim() {
    let r = start().await;
    load_basic(&r).await;
    let out = propose(
        &r,
        json!({"global": BASIC_MASTER, "index_globals": [BASIC_INDEX]}),
    )
    .await;
    let l = link(&out, BASIC_INDEX);
    let mut wrong = Vec::new();
    if l["missing_rows"]["rule"].is_object() {
        wrong.push(format!(
            "missing_rows claims a rule ({}) but all 60 rows are in the index: {}",
            l["missing_rows"]["rule"], l["missing_rows"]
        ));
    }
    if l["id"]["positions"] == json!([5]) {
        wrong.push(format!(
            "id.positions [5] with agreement {}: position 5 is the chunk number, 1 in every node",
            l["id"]["agreement"]
        ));
    }
    clean(&r).await;
    assert!(wrong.is_empty(), "{}\nlink: {l}", wrong.join("\n"));
}

/// `propose` of the iFind index global alone must not read its bit strings as `$c(1)`-delimited
/// fields of a master map. (A bit string holds `$c(1)` bytes; the map it drafts is nonsense.)
#[tokio::test]
#[ignore]
async fn the_ifind_index_global_alone_is_not_drafted_as_delimited_data() {
    let r = start().await;
    load_basic(&r).await;
    let out = propose(&r, json!({"global": BASIC_INDEX})).await;
    let draft = out["draft"].to_string();
    let delimited = out["draft"]["data"]
        .as_object()
        .map(|d| d.values().any(|f| f["delimiter"] == json!("\"\u{1}\"")))
        .unwrap_or(false);
    clean(&r).await;
    assert!(
        !delimited,
        "the draft splits bit strings on $c(1): {draft}\nshape formats: {}",
        out["shape"]["formats"]
    );
}

/// A default-storage class files its rows in `^<Class>D` and its indexes in `^<Class>I`, the same
/// stem. `propose` of the master alone offers `^<Class>I` as a candidate, or says it looked and
/// found none (it offers it, found by `stem_sibling`).
#[tokio::test]
#[ignore]
async fn the_master_alone_offers_the_index_global_beside_it() {
    let r = start().await;
    load_basic(&r).await;
    let out = propose(&r, json!({"global": BASIC_MASTER})).await;
    let offered = out["index_candidates"]["candidates"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|c| c["global"] == json!(BASIC_INDEX));
    let said = out["not_inferred_detail"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|d| d["what"].as_str().is_some_and(|w| w.contains("no index")));
    clean(&r).await;
    assert!(
        offered || said,
        "neither offered {BASIC_INDEX} nor said none was found: {}",
        out["index_candidates"]
    );
}

/// A master with no index global anywhere: the answer says it looked, and where.
#[tokio::test]
#[ignore]
async fn a_master_with_no_index_global_says_where_it_looked() {
    let r = start().await;
    exec(
        &r,
        "Set ^IadSm173Lone=2,^IadSm173Lone(1)=\"a^b\",^IadSm173Lone(2)=\"c^d\" Write \"SET\"",
    )
    .await;
    let out = propose(&r, json!({"global": "^IadSm173Lone"})).await;
    clean(&r).await;
    assert_eq!(out["index_candidates"]["listing"], "ok", "{out}");
    assert_eq!(out["index_candidates"]["candidates"], json!([]), "{out}");
    let said: Vec<&Value> = out["not_inferred_detail"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| d["what"] == "no index global found")
        .collect();
    assert_eq!(said.len(), 1, "{}", out["not_inferred_detail"]);
    let why = said[0]["why"].as_str().unwrap();
    assert!(
        why.contains("^IadSm173Lone") && why.contains("sub-trees inside the master"),
        "{why}"
    );
}

/// The analytic index of a DDL table (Benjamin's recipe): `propose` of the master and the index
/// global is safe, leaks no word, writes nothing, and never calls the link `inferred`.
#[tokio::test]
#[ignore]
async fn propose_over_the_analytic_index_of_a_ddl_table_is_safe_and_draws_no_link() {
    let r = start().await;
    let (master, index, _ids) = load_analytic(&r).await;
    let watched = [master.as_str(), index.as_str(), "^IRIS.IF.WordD"];
    let before = snapshot(&r, &watched).await;
    let words = corpus_words();
    let alone = propose(&r, json!({"global": master})).await;
    let linked = propose(&r, json!({"global": master, "index_globals": [index]})).await;
    assert_eq!(leaked(&alone, &words), Vec::<String>::new());
    assert_eq!(leaked(&linked, &words), Vec::<String>::new());
    // One piece in the `$LIST` master: the text column.
    assert_eq!(alone["shape"]["formats"][1]["pieces"]["max"], 1);
    assert_eq!(alone["shape"]["numeric_subscripts"]["1"], 60);
    let l = link(&linked, &index);
    assert_ne!(l["verdict"], "inferred", "{l}");
    assert!(l.get("map").is_none(), "{l}");
    assert_eq!(snapshot(&r, &watched).await, before, "propose wrote");
    clean(&r).await;
}

/// The analytic index is `^<ext>.3` and the master `^<ext>.1`: no name begins with the master's.
/// `propose` of the master alone offers the index global or says it found none.
#[tokio::test]
#[ignore]
async fn the_ddl_master_alone_offers_the_ifind_index_global() {
    let r = start().await;
    let (master, index, _ids) = load_analytic(&r).await;
    let out = propose(&r, json!({"global": master})).await;
    let offered = out["index_candidates"]["candidates"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|c| c["global"] == json!(index));
    let said = out["not_inferred_detail"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|d| d["what"].as_str().is_some_and(|w| w.contains("no index")));
    clean(&r).await;
    assert!(
        offered || said,
        "neither offered {index} nor said none was found: {}",
        out["index_candidates"]
    );
}

// ---- the shared `$BIT` global: ^IRIS.IF.TransfoB and the class %iFind.OriginalWord ---------------

/// `propose` of `^IRIS.IF.TransfoB` draws the chunk map, and its verification finds every word ID
/// IRIS's dictionary holds: 25 rows whose IDs sum to 1+2+...+25.
#[tokio::test]
#[ignore]
async fn propose_of_transfob_drafts_a_chunk_map_that_finds_every_word_id() {
    let r = start().await;
    load_basic(&r).await;
    let n = vocabulary().len() as u64;
    let out = propose(&r, json!({"global": "^IRIS.IF.TransfoB"})).await;
    let chunk = &out["chunk_map_draft"];
    assert_eq!(chunk["bits"]["first_id"], 1, "{chunk}");
    assert_eq!(chunk["bits"]["last_id"], n, "{chunk}");
    assert_eq!(chunk["bits"]["ids_read"], n, "{chunk}");
    assert_eq!(chunk["verified"]["compiled"], true, "{chunk}");
    assert_eq!(chunk["verified"]["rows"], n, "{chunk}");
    assert_eq!(chunk["verified"]["id_sum"], n * (n + 1) / 2, "{chunk}");
    // The map draws level 1 as the constant 0, level 2 as the chunk, level 3 by code.
    let subs = chunk["map"]["subscripts"].as_array().expect("subscripts");
    assert_eq!(subs[0]["expression"], "0");
    assert!(subs[1]["expression"]
        .as_str()
        .unwrap()
        .ends_with("\\64000+1"));
    assert_eq!(subs[2]["access_type"], "Other");
    clean(&r).await;
}

/// The chunk map is the answer for `^IRIS.IF.TransfoB`; the plain `draft` beside it must not call
/// the bit string two `$c(1)`-delimited fields.
#[tokio::test]
#[ignore]
async fn transfob_draft_beside_the_chunk_map_does_not_split_the_bit_string() {
    let r = start().await;
    load_basic(&r).await;
    let out = propose(&r, json!({"global": "^IRIS.IF.TransfoB"})).await;
    assert!(out.get("chunk_map_draft").is_some(), "{out}");
    let delimited = out["draft"]["data"]
        .as_object()
        .map(|d| d.values().any(|f| f["delimiter"] == json!("\"\u{1}\"")))
        .unwrap_or(false);
    let draft = out["draft"].to_string();
    clean(&r).await;
    assert!(
        !delimited,
        "the plain draft splits the bit string on $c(1): {draft}"
    );
}

/// `read` and `compare` of `%iFind.OriginalWord` carry the fact that level 3 is computed by code
/// and may not exist in the global, and `compare` does not report the map as broken.
#[tokio::test]
#[ignore]
async fn read_and_compare_of_original_word_say_that_level_3_is_computed() {
    let r = start().await;
    load_basic(&r).await;
    let read = r
        .call(
            "iris_storage_map",
            json!({"action": "read", "class": "%iFind.OriginalWord", "namespace": "USER"}),
        )
        .await;
    assert_eq!(read["success"], true, "{read}");
    let facts: Vec<String> = read["derived"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| d["path"] == "/maps/Data/subscripts/3")
        .filter_map(|d| d["fact"].as_str().map(str::to_string))
        .collect();
    assert!(
        facts
            .iter()
            .any(|f| f.contains("computed by code") && f.contains("may not exist")),
        "{facts:?}"
    );
    let compare = r
        .call(
            "iris_storage_map",
            json!({"action": "compare", "class": "%iFind.OriginalWord", "namespace": "USER"}),
        )
        .await;
    assert_eq!(compare["success"], true, "{compare}");
    assert_eq!(compare["mismatches"], json!([]), "{compare}");
    let note = compare["notes"].to_string();
    assert!(
        note.contains("level 3 is computed by code") && note.contains("depth 2"),
        "{note}"
    );
    clean(&r).await;
}

/// The picture of the map is what a reader looks at first: its level 3 node says the level is
/// computed (it read `sub 3: {OriginalWord} (Other)`, the same as a real subscript, and the fact
/// sat at the foot of the markdown).
#[tokio::test]
#[ignore]
async fn the_diagram_of_original_word_marks_level_3_as_computed() {
    let r = start().await;
    let out = r
        .call(
            "iris_storage_map",
            json!({"action": "diagram", "class": "%iFind.OriginalWord", "namespace": "USER"}),
        )
        .await;
    assert_eq!(out["success"], true, "{out}");
    let label = out["graph"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|n| n["label"].as_str().is_some_and(|l| l.starts_with("sub 3")))
        .map(|n| n["label"].to_string())
        .expect("a level 3 node");
    let mermaid = out["mermaid"].as_str().unwrap_or_default().to_string();
    clean(&r).await;
    assert!(
        label.contains("comput") || mermaid.contains("comput"),
        "level 3 is drawn as an ordinary subscript: {label}"
    );
}

// ---- a global with no data ---------------------------------------------------------------------

/// Benjamin wants "I looked and found no data". `compare` against a global with no nodes says so in
/// `notes`, and must not also list five `no value there` mismatches (it says the map was not
/// checked).
#[tokio::test]
#[ignore]
async fn compare_of_a_global_with_no_nodes_says_so_and_reports_no_mismatch() {
    let r = start().await;
    let text = person_class("IadSm173.Empty", "IadSm173IfEmpty", "", "");
    put_class(&r, "IadSm173.Empty", &text).await;
    assert_eq!(value(&r, "^IadSm173IfEmpty").await, "");
    assert_eq!(exec(&r, "Write $Data(^IadSm173IfEmpty)").await.trim(), "0");
    let out = r
        .call(
            "iris_storage_map",
            json!({"action": "compare", "class": "IadSm173.Empty", "namespace": "USER"}),
        )
        .await;
    clean(&r).await;
    assert_eq!(out["success"], true, "{out}");
    assert!(
        out["notes"].to_string().contains("no data found"),
        "{}",
        out["notes"]
    );
    assert_eq!(
        out["mismatches"],
        json!([]),
        "the map was not checked, yet it lists mismatches"
    );
}

/// `propose` over a global with no nodes says plainly that it found no data (T075; it had been a
/// successful draft with an empty `data`, and only `shape.nodes_scanned = 0` hinted at it).
#[tokio::test]
#[ignore]
async fn propose_of_a_global_with_no_nodes_says_it_found_no_data() {
    let r = start().await;
    assert_eq!(exec(&r, "Write $Data(^IadSm173IfEmpty)").await.trim(), "0");
    let out = propose(&r, json!({"global": "^IadSm173IfEmpty"})).await;
    let says = out["not_inferred_detail"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|d| d["what"].as_str().is_some_and(|w| w.contains("no data")))
        || out["draft_note"]
            .as_str()
            .is_some_and(|n| n.contains("no data"))
        || out["notes"].to_string().contains("no data");
    clean(&r).await;
    assert!(
        says,
        "no statement that the global holds no data; draft_note: {}, nodes_scanned: {}",
        out["draft_note"], out["shape"]["nodes_scanned"]
    );
    // And the empty draft is not offered as a map of the data.
    assert_eq!(out["shape"]["nodes_scanned"], 0);
}

// ---- Benjamin's ^bdb / ^bdb.x -----------------------------------------------------------------

/// `^<g>=<rows>` is the counter; row `i` is `Val<..>^<b>^<c>^<d>` with the indexed text first;
/// `(i,"sub")=99934.1` on rows 1 and 2 only; the index `^<g>.x(<piece 1>)=<i>`. The three rows are
/// Benjamin's own; the 40 rows are invented in the same shape.
fn bdb_rows(rows: usize) -> Vec<(String, String)> {
    (1..=rows)
        .map(|i| {
            if rows == 3 {
                let (a, b, c, d) = [
                    ("Val1", "x", "", "123"),
                    ("Val2", "", "yze", "32455"),
                    ("Val134", "qsfdqsf", "ERSR", "123"),
                ][i - 1];
                (a.to_string(), format!("{a}^{b}^{c}^{d}"))
            } else {
                let a = format!("Val{i}");
                let c = if i % 3 == 0 {
                    format!("abc{i}")
                } else {
                    String::new()
                };
                (a.clone(), format!("{a}^q{}^{c}^{}", i % 7, 100 + i * 3))
            }
        })
        .collect()
}

async fn load_bdb(r: &Rig, g: &str, rows: usize) {
    let mut code = format!("Set ^{g}={rows}\n");
    for (i, (key, line)) in bdb_rows(rows).iter().enumerate() {
        let i = i + 1;
        code.push_str(&format!(
            "Set ^{g}({i})={}\nSet ^{g}.x({})={i}\n",
            os(line),
            os(key)
        ));
        if i <= 2 {
            code.push_str(&format!("Set ^{g}({i},\"sub\")=99934.1\n"));
        }
    }
    code.push_str("Write \"SET\"\n");
    assert!(exec(r, &code).await.contains("SET"));
}

/// Benjamin's three rows: the link is `too_few_nodes`, no map is drafted for the index, the row
/// IDs are in the index value, and nothing sampled leaves IRIS.
#[tokio::test]
#[ignore]
async fn bdb_three_rows_is_too_few_nodes_with_no_drafted_map_and_no_value_leaked() {
    let r = start().await;
    load_bdb(&r, "IadSm173Bdb", 3).await;
    let out = propose(
        &r,
        json!({"global": "^IadSm173Bdb", "index_globals": ["^IadSm173Bdb.x"]}),
    )
    .await;
    let l = link(&out, "^IadSm173Bdb.x");
    assert_eq!(l["verdict"], "too_few_nodes", "{l}");
    assert!(l.get("map").is_none(), "{l}");
    // The row ID of each index node is its value (the index is `^bdb.x(text) = id`).
    assert_eq!(l["id"]["form"], "value", "{l}");
    assert_eq!(l["distinct_first_values"], 3, "{l}");
    assert_eq!(l["usable_nodes"], 3, "{l}");
    // Master: ID counter and one row per ID; the delimiter is ^ and there are four pieces + "sub".
    let draft = &out["draft"];
    assert_eq!(draft["global"], "^IadSm173Bdb");
    for (n, name) in ["Field1", "Field2", "Field3", "Field4"].iter().enumerate() {
        assert_eq!(draft["data"][name]["piece"], (n + 1).to_string(), "{draft}");
        assert_eq!(draft["data"][name]["delimiter"], "\"^\"", "{draft}");
    }
    assert_eq!(draft["data"]["Sub1"]["node"], "\"sub\"", "{draft}");
    // Types from the three rows: piece 4 is 123, 32455, 123 (all integers); piece 1 is at most
    // "Val134", 6 characters.
    let props = &out["class_draft"]["properties"];
    assert_eq!(props[0]["evidence"]["max_length"], 6, "{props}");
    assert_eq!(props[3]["type"], "%Integer", "{props}");
    assert_eq!(
        props[3]["evidence"],
        json!({"integer": 3, "of": 3}),
        "{props}"
    );
    // No sampled value leaves IRIS.
    let sampled: BTreeSet<String> = ["val1", "val2", "val134", "qsfdqsf", "yze", "ersr", "32455"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(leaked(&out, &sampled), Vec::<String>::new());
    clean(&r).await;
}

/// With three rows the answer says why it cannot link and what would make it infer: it names the
/// minimum (20 usable nodes); it had said only `the link is too_few_nodes`.
#[tokio::test]
#[ignore]
async fn bdb_three_rows_names_the_minimum_that_would_let_it_infer() {
    let r = start().await;
    load_bdb(&r, "IadSm173Bdb", 3).await;
    let out = propose(
        &r,
        json!({"global": "^IadSm173Bdb", "index_globals": ["^IadSm173Bdb.x"]}),
    )
    .await;
    let l = link(&out, "^IadSm173Bdb.x");
    let text = format!("{} {}", out["not_inferred_detail"], l);
    clean(&r).await;
    assert!(
        out["not_inferred_detail"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| d["why"].as_str())
            .any(|w| w.contains("20")),
        "the answer never names the 20-node minimum: {text}"
    );
}

/// `Sub1` is the `"sub"` node of rows 1 and 2, both 99934.1 (seven characters). The class draft's
/// evidence for it must count them. It had counted none (`of` 0, so `%String` with `max_length` 0:
/// the typing code looked for a node named `"sub"` with the quotes in it).
///
/// The first version of this test asked for `evidence.max_length == 7`. Two values, both numbers
/// with one fraction digit, are `%Numeric`, whose evidence is `{numeric, max_scale, of}` (pinned by
/// `test_storage_map_typing_173`, the corpus and the typing live file), so the test asks for what
/// the answer carries: the two values counted, typed `%Numeric` with `SCALE = 1`.
#[tokio::test]
#[ignore]
async fn bdb_the_sub_node_field_is_typed_from_its_own_values() {
    let r = start().await;
    load_bdb(&r, "IadSm173Bdb", 3).await;
    let out = propose(
        &r,
        json!({"global": "^IadSm173Bdb", "index_globals": ["^IadSm173Bdb.x"]}),
    )
    .await;
    let sub = out["class_draft"]["properties"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["name"] == "Sub1")
        .cloned()
        .expect("a Sub1 property");
    clean(&r).await;
    assert_eq!(sub["type"], "%Numeric", "{sub}");
    assert_eq!(sub["parameters"], json!(["SCALE = 1"]), "{sub}");
    assert_eq!(
        sub["evidence"],
        json!({"numeric": 2, "max_scale": 1, "of": 2}),
        "rows 1 and 2 hold 99934.1 and row 3 holds none: {sub}"
    );
}

/// With enough rows (40, the same shape) `propose` links `^bdb.x` to `^bdb`: the index subscript is
/// piece 1 (`Field1`) compared exactly, the row ID is the index value, and the drafted map
/// compiles, finds a row and is used by the planner.
#[tokio::test]
#[ignore]
async fn bdb_forty_rows_infer_the_link_and_the_draft_is_used() {
    let r = start().await;
    load_bdb(&r, "IadSm173Bdq", 40).await;
    let out = propose(
        &r,
        json!({"global": "^IadSm173Bdq", "index_globals": ["^IadSm173Bdq.x"]}),
    )
    .await;
    let l = link(&out, "^IadSm173Bdq.x");
    assert_eq!(l["verdict"], "inferred", "{l}");
    assert_eq!(l["id"]["form"], "value", "{l}");
    let sub = &l["subscripts"][0];
    assert_eq!(sub["field"], "Field1", "{sub}");
    assert_eq!(sub["transform"], "exact", "{sub}");
    assert_eq!(sub["verdict"], "inferred", "{sub}");
    assert_eq!(l["distinct_first_values"], 40, "{l}");
    assert_eq!(l["map"]["type"], "index", "{l}");
    assert_eq!(l["map"]["subscripts"][0]["expression"], "{Field1}", "{l}");
    assert_eq!(l["verified"]["compiled"], true, "{l}");
    assert_eq!(l["verified"]["row_found"], true, "{l}");
    assert_eq!(l["verified"]["usability"]["status"], "used", "{l}");
    // Types: piece 4 is 103, 106, ..., 220 (all integers).
    let props = &out["class_draft"]["properties"];
    assert_eq!(props[3]["type"], "%Integer", "{props}");
    assert_eq!(
        props[3]["evidence"],
        json!({"integer": 40, "of": 40}),
        "{props}"
    );
    // No sampled value leaves IRIS.
    let sampled: BTreeSet<String> = ["val1", "val17", "val40", "abc9", "abc39"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(leaked(&out, &sampled), Vec::<String>::new());
    clean(&r).await;
}
