//! Spec 131: each factual claim in PR 142's `iris-mdx` SKILL.md, measured on a cube built here.
//!
//! The fixture is `tests/fixtures/mdx131/`: eight hand-written `IadLive131.Sale` rows and two cubes
//! over them. Every test builds it, asserts what IRIS returned, and drops the package, so the order
//! the tests run in does not matter. Where IRIS disagrees with the claim the test asserts IRIS; the
//! verdicts are in `specs/131-mdx-cube-facts/research.md`.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --features testing --test integration test_mdx_131_live -- \
//!     --ignored --test-threads=1

use iris_agentic_dev_core::iris::connection::{
    is_generator_error, DiscoverySource, IrisConnection,
};

const NS: &str = "USER";
const PKG: &str = "IadLive131";
const SALES: &str = "IadLive131Sales";

const SOURCES: [(&str, &str); 3] = [
    (
        "IadLive131.Sale.cls",
        include_str!("../fixtures/mdx131/Sale.cls"),
    ),
    (
        "IadLive131.SalesCube.cls",
        include_str!("../fixtures/mdx131/SalesCube.cls"),
    ),
    (
        "IadLive131.OtherCube.cls",
        include_str!("../fixtures/mdx131/OtherCube.cls"),
    ),
];

/// Region, channel, sale date, doctor id, doctor name, amount. Europe has no 2023 row; doctors 11
/// and 13 share the caption Smith; row 3 has no amount. The README tabulates the same rows.
const ROWS: [(&str, u8, &str, u8, &str, &str); 8] = [
    ("Asia", 1, "2023-03-10", 11, "Smith", "100"),
    ("Asia", 2, "2023-07-04", 12, "Jones", "50"),
    ("Asia", 2, "2024-03-15", 11, "Smith", ""),
    ("Europe", 1, "2024-01-20", 13, "Smith", "200"),
    ("Europe", 1, "2024-03-02", 12, "Jones", "30"),
    ("Europe", 3, "2024-07-19", 13, "Smith", "70"),
    ("Americas", 2, "2024-11-11", 11, "Smith", "40"),
    ("Americas", 1, "2023-11-30", 12, "Jones", "10"),
];

/// The connection, or a panic naming what to set. `IAD_ALLOW_SKIP=1` opts into skipping.
fn conn() -> Option<(IrisConnection, reqwest::Client)> {
    let host = std::env::var("IRIS_HOST").unwrap_or_default();
    if host.is_empty() {
        if std::env::var("IAD_ALLOW_SKIP").is_ok() {
            eprintln!("SKIP (IAD_ALLOW_SKIP set): IRIS_HOST unset");
            return None;
        }
        panic!(
            "IRIS_HOST unset. This test measures an MDX claim against live IRIS; without IRIS it \
             asserts nothing, so it fails instead of passing quietly.\n\
             Set: IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS\n\
             Or opt into skipping deliberately: IAD_ALLOW_SKIP=1"
        );
    }
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    let conn = IrisConnection::new(
        format!("http://{host}:{port}"),
        NS,
        user,
        pass,
        DiscoverySource::EnvVar,
    );
    Some((conn, iris_agentic_dev_core::testing::live_client()))
}

/// Run ObjectScript and return its output; a generator error fails the test.
async fn run(c: &IrisConnection, client: &reqwest::Client, code: &str) -> String {
    let out = c
        .execute_via_generator(code, NS, client)
        .await
        .expect("execute request must reach IRIS");
    assert!(!is_generator_error(&out), "IRIS run failed: {out}");
    out.trim().to_string()
}

/// The text between the last `~[` and the `]~` after it.
fn marked(out: &str) -> String {
    let start = out
        .rfind("~[")
        .unwrap_or_else(|| panic!("no ~[ marker in {out}"));
    let end = out[start..]
        .find("]~")
        .unwrap_or_else(|| panic!("no ]~ marker in {out}"));
    out[start + 2..start + end].to_string()
}

async fn put(c: &IrisConnection, client: &reqwest::Client, doc: &str, src: &str) {
    let url = c.versioned_ns_url(
        NS,
        &format!("/doc/{}?ignoreConflict=1", urlencoding::encode(doc)),
    );
    let lines: Vec<&str> = src.lines().collect();
    let resp = client
        .put(&url)
        .basic_auth(&c.username, Some(&c.password))
        .json(&serde_json::json!({"enc": false, "content": lines}))
        .send()
        .await
        .expect("PUT must reach IRIS");
    let status = resp.status().as_u16();
    assert!(
        (200..300).contains(&status),
        "PUT {doc}: HTTP {status} {}",
        resp.text().await.unwrap_or_default()
    );
}

/// Kill both cubes' data, the source extent, and every class under the package. Safe to call when
/// nothing is there, which is how setup clears a killed run's leftovers.
async fn teardown(c: &IrisConnection, client: &reqwest::Client) {
    let _ = c
        .execute_via_generator(
            &format!(
                " Try {{ Do ##class(%DeepSee.Utils).%KillCube(\"{SALES}\") }} Catch {{ }}\n \
                 Try {{ Do ##class(%DeepSee.Utils).%KillCube(\"IadLive131Other\") }} Catch {{ }}\n \
                 Try {{ Do ##class(IadLive131.Sale).%KillExtent() }} Catch {{ }}\n \
                 Do $system.OBJ.DeletePackage(\"{PKG}\",\"-d\")"
            ),
            NS,
            client,
        )
        .await;
}

async fn classes_left(c: &IrisConnection, client: &reqwest::Client) -> String {
    marked(
        &run(
            c,
            client,
            &format!(
                " Set r=##class(%SQL.Statement).%ExecDirect(,\"SELECT COUNT(*) AS n FROM \
                 %Dictionary.ClassDefinition WHERE Name %STARTSWITH '{PKG}.'\")\n \
                 Do r.%Next() Write \"~[\",r.n,\"]~\""
            ),
        )
        .await,
    )
}

/// Drop leftovers, load and compile the three classes, write the rows, build both cubes.
async fn setup(c: &IrisConnection, client: &reqwest::Client) {
    teardown(c, client).await;
    for (doc, src) in SOURCES {
        put(c, client, doc, src).await;
        let r = c
            .compile_document(doc, NS, "cuk", client)
            .await
            .expect("compile request must reach IRIS");
        assert!(
            r.errors.is_empty(),
            "{doc} did not compile: {}\n{}",
            r.errors.join("\n"),
            r.console.join("\n")
        );
    }
    let mut code = String::new();
    for (region, channel, date, doctor, name, amount) in ROWS {
        code.push_str(&format!(
            " Set o=##class(IadLive131.Sale).%New(),o.Region=\"{region}\",o.Channel={channel},\
             o.SaleDate=$zdh(\"{date}\",3),o.Doctor={doctor},o.DoctorName=\"{name}\",\
             o.Amount=\"{amount}\",sc=o.%Save() If $$$ISERR(sc) {{ Write \"~[\",\
             $system.Status.GetErrorText(sc),\"]~\" Quit }}\n"
        ));
    }
    code.push_str(
        " For cube=\"IadLive131Sales\",\"IadLive131Other\" { Set sc=##class(%DeepSee.Utils)\
         .%BuildCube(cube,0,0) If $$$ISERR(sc) { Write \"~[\",cube,\": \",\
         $system.Status.GetErrorText(sc),\"]~\" Quit } }\n Write \"~[built]~\"",
    );
    let out = run(c, client, &code).await;
    assert_eq!(marked(&out), "built", "fixture build failed: {out}");
}

/// What `%DeepSee.ResultSet` returned for one query.
#[derive(Debug)]
struct Mdx {
    /// `prepare: <text>` or `execute: <text>` when IRIS refused the query.
    error: Option<String>,
    cols: Vec<String>,
    /// Row label and cells. A query with no row axis has one row labelled "". A null cell is None.
    rows: Vec<(String, Vec<Option<String>>)>,
}

impl Mdx {
    fn cell(&self, row: usize, col: usize) -> Option<&str> {
        self.rows[row].1[col].as_deref()
    }

    /// The single cell of a query with no axes.
    fn scalar(&self) -> Option<&str> {
        assert!(self.error.is_none(), "{self:?}");
        assert_eq!(self.rows.len(), 1, "{self:?}");
        self.cell(0, 0)
    }

    fn labels(&self) -> Vec<&str> {
        self.rows.iter().map(|(l, _)| l.as_str()).collect()
    }
}

/// `%PrepareMDX`, `%Execute`, then every label and cell, as JSON between the markers.
async fn mdx(c: &IrisConnection, client: &reqwest::Client, query: &str) -> Mdx {
    let q = query.replace('"', "\"\"");
    let code = format!(
        " Set out={{}},q=\"{q}\",rs=##class(%DeepSee.ResultSet).%New()\n \
         Set sc=rs.%PrepareMDX(q) If $$$ISERR(sc) {{ Set out.error=\"prepare: \"_$system.Status.GetErrorText(sc) Write \"~[\",out.%ToJSON(),\"]~\" Quit }}\n \
         Set sc=rs.%Execute() If $$$ISERR(sc) {{ Set out.error=\"execute: \"_$system.Status.GetErrorText(sc) Write \"~[\",out.%ToJSON(),\"]~\" Quit }}\n \
         Set ax=rs.%GetAxisCount(),n1=$s(ax>=1:rs.%GetAxisSize(1),1:0),n2=$s(ax>=2:rs.%GetAxisSize(2),1:0)\n \
         Set out.cols=[],out.rows=[]\n \
         For i=1:1:n1 {{ Kill lab Set k=rs.%GetOrdinalLabel(.lab,1,i) Do out.cols.%Push($g(lab(1))) }}\n \
         Set out.hasrows=(ax>=2)\n \
         For j=1:1:$s(ax>=2:n2,1:1) {{ Kill lab Set:ax>=2 k=rs.%GetOrdinalLabel(.lab,2,j) Set row={{\"label\":($g(lab(1))),\"cells\":[]}} For i=1:1:$s(n1:n1,1:1) {{ Do row.cells.%Push(\"\"_$s(ax>=2:rs.%GetOrdinalValue(i,j),1:rs.%GetOrdinalValue(i))) }} Do out.rows.%Push(row) }}\n \
         Write \"~[\",out.%ToJSON(),\"]~\""
    );
    let text = marked(&run(c, client, &code).await);
    let v: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
    let strs = |a: &serde_json::Value| -> Vec<String> {
        a.as_array()
            .map(|a| {
                a.iter()
                    .map(|x| x.as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    let rows = v["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|r| {
                    let cells = strs(&r["cells"])
                        .into_iter()
                        .map(|s| (!s.is_empty()).then_some(s))
                        .collect();
                    (r["label"].as_str().unwrap_or_default().to_string(), cells)
                })
                .collect()
        })
        .unwrap_or_default();
    // With NON EMPTY and nothing left, the rows axis has size 0 and the loop never runs.
    Mdx {
        error: v["error"].as_str().map(str::to_string),
        cols: strs(&v["cols"]),
        rows,
    }
}

/// Build the fixture, hand each claim test the connection, drop the fixture, check it is gone.
macro_rules! with_cube {
    (|$c:ident, $client:ident| $body:block) => {{
        let Some(($c, $client)) = conn() else { return };
        setup(&$c, &$client).await;
        let result = async { $body }.await;
        teardown(&$c, &$client).await;
        assert_eq!(
            classes_left(&$c, &$client).await,
            "0",
            "fixture left classes behind"
        );
        result
    }};
}

const AMOUNT_BY_REGION: &str =
    "SELECT MEASURES.[Amount] ON 0, [RegionD].[H1].[Region].MEMBERS ON 1 FROM [IadLive131Sales]";

// --- US2: the fixture itself --------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live IRIS"]
async fn fixture_builds_and_counts_its_rows() {
    with_cube!(|c, client| {
        let out = marked(
            &run(
                &c,
                &client,
                " Set sc=##class(%DeepSee.Utils).%GetCubeList(.list) Set s=\"\",k=\"\" \
                 For { Set k=$o(list(k)) Quit:k=\"\"  Set:$zcvt(k,\"U\")[\"IADLIVE131\" s=s_k_\",\" }\n \
                 Write \"~[\",s,\"]~\"",
            )
            .await,
        );
        assert!(out.contains("IADLIVE131SALES"), "{out}");
        assert!(out.contains("IADLIVE131OTHER"), "{out}");
        let dims = marked(
            &run(
                &c,
                &client,
                &format!(
                    " Set sc=##class(%DeepSee.Utils).%GetDimensionList(\"{SALES}\",.info) \
                     Set s=\"\",k=\"\" For {{ Set k=$o(info(k)) Quit:k=\"\"  Set a=\"\" \
                     For {{ Set a=$o(info(k,a)) Quit:a=\"\"  Set b=\"\" For {{ Set b=$o(info(k,a,b)) \
                     Quit:b=\"\"  Set s=s_$lg(info(k,a,b),2)_\"|\" }} }} }}\n \
                     Write \"~[\",s,\"]~\""
                ),
            )
            .await,
        );
        for d in ["RegionD", "ChannelD", "DateD", "DoctorD"] {
            assert!(
                dims.contains(d),
                "{d} missing from %GetDimensionList: {dims}"
            );
        }
        let all = mdx(&c, &client, "SELECT FROM [IadLive131Sales]").await;
        assert_eq!(all.scalar(), Some("8"));
    });
}

// --- US3: the claims -----------------------------------------------------------------------------

/// Claim: running MDX through `%DeepSee.ResultSet` and reading cells. Holds.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn result_set_reads_labels_and_cells() {
    with_cube!(|c, client| {
        let r = mdx(&c, &client, AMOUNT_BY_REGION).await;
        assert_eq!(r.cols, ["Amount"]);
        assert_eq!(r.labels(), ["Americas", "Asia", "Europe"]);
        assert_eq!(r.cell(0, 0), Some("50"));
        assert_eq!(r.cell(1, 0), Some("150"));
        assert_eq!(r.cell(2, 0), Some("300"));
    });
}

/// Claim: a wrong hierarchy path returns an empty-member row with a null value and no error. Holds.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn wrong_hierarchy_path_gives_one_empty_row() {
    with_cube!(|c, client| {
        let r = mdx(
            &c,
            &client,
            "SELECT MEASURES.[Amount] ON 0, [Region].[Region].MEMBERS ON 1 FROM [IadLive131Sales]",
        )
        .await;
        assert_eq!(r.error, None);
        assert_eq!(r.labels(), [""]);
        assert_eq!(r.cell(0, 0), None);
    });
}

/// Claim: without NON EMPTY every member comes back, including those with no data. Holds.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn non_empty_drops_the_member_with_no_data() {
    with_cube!(|c, client| {
        let q = format!("{AMOUNT_BY_REGION} WHERE [DateD].[Actual].[YearSold].&[2023]");
        let plain = mdx(&c, &client, &q).await;
        assert_eq!(plain.labels(), ["Americas", "Asia", "Europe"]);
        assert_eq!(plain.cell(2, 0), None, "Europe has no 2023 sale");
        let ne = mdx(&c, &client, &q.replace("[RegionD]", "NON EMPTY [RegionD]")).await;
        assert_eq!(ne.labels(), ["Americas", "Asia"]);
    });
}

/// Claim: WHERE and %FILTER give the same result. Holds for results; the "identical MDXText"
/// mechanism is not measured.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn where_and_filter_agree() {
    with_cube!(|c, client| {
        let slicer = "[DateD].[Actual].[YearSold].&[2023]";
        let w = mdx(&c, &client, &format!("{AMOUNT_BY_REGION} WHERE {slicer}")).await;
        let f = mdx(&c, &client, &format!("{AMOUNT_BY_REGION} %FILTER {slicer}")).await;
        assert_eq!(w.rows, f.rows);
        assert_eq!(w.cell(1, 0), Some("150"));
    });
}

/// Claim: two %FILTER on one level AND together and return null with no error; a set on the axis
/// gives the side-by-side. Holds.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn two_filters_on_one_level_and_to_nothing() {
    with_cube!(|c, client| {
        let both = format!(
            "{AMOUNT_BY_REGION} %FILTER [DateD].[Actual].[YearSold].&[2023] \
             %FILTER [DateD].[Actual].[YearSold].&[2024]"
        );
        let r = mdx(&c, &client, &both).await;
        assert_eq!(r.error, None);
        assert!(r.rows.iter().all(|(_, cells)| cells[0].is_none()), "{r:?}");
        let side = mdx(
            &c,
            &client,
            "SELECT MEASURES.[Amount] ON 0, NON EMPTY {[DateD].[Actual].[YearSold].&[2023],\
             [DateD].[Actual].[YearSold].&[2024]} ON 1 FROM [IadLive131Sales]",
        )
        .await;
        assert_eq!(side.labels(), ["2023", "2024"]);
        assert_eq!(side.cell(0, 0), Some("160"));
        assert_eq!(side.cell(1, 0), Some("340"));
    });
}

/// Claim: `WHERE {a,b}` can double-count, `%OR` does not. On IRIS both give the same count, and the
/// `%OR` row on an axis is labelled first...last, not "first+". Reworded.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn where_set_and_or_count_the_same() {
    with_cube!(|c, client| {
        let set = "{[RegionD].[H1].[Region].&[Asia],[RegionD].[H1].[Region].&[Europe]}";
        let w = mdx(&c, &client, &format!("SELECT FROM [{SALES}] WHERE {set}")).await;
        let o = mdx(
            &c,
            &client,
            &format!("SELECT FROM [{SALES}] %FILTER %OR({set})"),
        )
        .await;
        assert_eq!(w.scalar(), Some("6"));
        assert_eq!(o.scalar(), Some("6"));
        let axis = mdx(
            &c,
            &client,
            &format!("SELECT MEASURES.[Amount] ON 0, NON EMPTY %OR({set}) ON 1 FROM [{SALES}]"),
        )
        .await;
        assert_eq!(axis.labels(), ["Asia...Europe"]);
        assert_eq!(axis.cell(0, 0), Some("450"));
    });
}

/// Claim: `%MDX()` placed directly on an axis returns empty. False: it returns the subquery's value.
/// Inside WITH MEMBER it gives percent-of-total as the PR shows.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn mdx_function_on_an_axis_returns_its_value() {
    with_cube!(|c, client| {
        let direct = mdx(
            &c,
            &client,
            &format!(
                "SELECT %MDX(\"SELECT MEASURES.[Amount] ON 0 FROM [{SALES}]\") ON 0 FROM [{SALES}]"
            ),
        )
        .await;
        assert_eq!(direct.error, None);
        assert_eq!(direct.cell(0, 0), Some("500"));
        let pct = mdx(
            &c,
            &client,
            &format!(
                "WITH MEMBER MEASURES.[Pct] AS '100 * MEASURES.[Amount] / \
                 %MDX(\"SELECT MEASURES.[Amount] ON 0 FROM [{SALES}]\")' \
                 SELECT MEASURES.[Pct] ON 0, NON EMPTY [RegionD].[H1].[Region].MEMBERS ON 1 \
                 FROM [{SALES}]"
            ),
        )
        .await;
        assert_eq!(pct.labels(), ["Americas", "Asia", "Europe"]);
        assert_eq!(pct.cell(2, 0), Some("60"));
    });
}

/// Claim: `MEASURES.MEMBERS` excludes `%COUNT`. False: it includes it, headed "Count", so the PR's
/// `{MEASURES.[%COUNT], MEASURES.MEMBERS}` shows the count twice.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn measures_members_includes_count() {
    with_cube!(|c, client| {
        let r = mdx(
            &c,
            &client,
            &format!("SELECT MEASURES.MEMBERS ON 0 FROM [{SALES}]"),
        )
        .await;
        assert_eq!(r.cols, ["Count", "Amount"]);
        assert_eq!(r.cell(0, 0), Some("8"));
        let twice = mdx(
            &c,
            &client,
            &format!("SELECT {{MEASURES.[%COUNT], MEASURES.MEMBERS}} ON 0 FROM [{SALES}]"),
        )
        .await;
        assert_eq!(twice.cols, ["Count", "Count", "Amount"]);
    });
}

/// Claim: with no ON 0 the implicit column is `%COUNT` and its header is "". Holds.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn axis_skipping_counts_under_an_empty_header() {
    with_cube!(|c, client| {
        let r = mdx(
            &c,
            &client,
            &format!("SELECT [RegionD].[H1].[Region].MEMBERS ON ROWS FROM [{SALES}]"),
        )
        .await;
        assert_eq!(r.cols, [""]);
        assert_eq!(r.labels(), ["Americas", "Asia", "Europe"]);
        assert_eq!(r.cell(0, 0), Some("2"));
        assert_eq!(r.cell(1, 0), Some("3"));
    });
}

/// Claim: an integer-keyed level needs `&[key]`, and the caption as a key returns null with no
/// error. Holds on a level with a separate name property (Doctor). Reworded: on a level whose
/// `rangeExpression` maps the integers to names (Channel), the mapped name is the key and `&[2]`
/// is the one that returns null.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn member_keys_follow_the_level_definition() {
    with_cube!(|c, client| {
        let doctor =
            |k: &str| format!("SELECT FROM [{SALES}] %FILTER [DoctorD].[H1].[Doctor].&[{k}]");
        assert_eq!(mdx(&c, &client, &doctor("12")).await.scalar(), Some("3"));
        let caption = mdx(&c, &client, &doctor("Jones")).await;
        assert_eq!(caption.error, None);
        assert_eq!(caption.scalar(), None);

        let channel = |k: &str| {
            format!("SELECT FROM [{SALES}] %FILTER [ChannelD].[H1].[Channel Name].&[{k}]")
        };
        assert_eq!(
            mdx(&c, &client, &channel("Online")).await.scalar(),
            Some("3")
        );
        assert_eq!(mdx(&c, &client, &channel("2")).await.scalar(), None);

        let keys = mdx(
            &c,
            &client,
            &format!(
                "WITH MEMBER MEASURES.[K] AS '[DoctorD].[H1].[Doctor].CURRENTMEMBER.PROPERTIES(\"KEY\")' \
                 SELECT MEASURES.[K] ON 0, [DoctorD].[H1].[Doctor].MEMBERS ON 1 FROM [{SALES}]"
            ),
        )
        .await;
        assert_eq!(keys.labels(), ["Jones", "Smith", "Smith"]);
        assert_eq!(keys.cell(1, 0), Some("11"));
        assert_eq!(keys.cell(2, 0), Some("13"));
    });
}

/// Claim: a duplicate caption silently resolves to the first member. Holds.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn duplicate_caption_resolves_to_the_first_member() {
    with_cube!(|c, client| {
        let by_caption = mdx(
            &c,
            &client,
            &format!("SELECT FROM [{SALES}] %FILTER [DoctorD].[H1].[Doctor].[Smith]"),
        )
        .await;
        assert_eq!(by_caption.error, None);
        assert_eq!(by_caption.scalar(), Some("3"), "doctor 11, not 11+13");
        let other = mdx(
            &c,
            &client,
            &format!("SELECT FROM [{SALES}] %FILTER [DoctorD].[H1].[Doctor].&[13]"),
        )
        .await;
        assert_eq!(other.scalar(), Some("2"));
    });
}

/// Claim: measures on two axes are an error, so `MAX(set, measure)` belongs on the members' axis.
/// The error holds; the fix is false: `MAX` on the members' axis raises the same error.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn measures_on_two_axes_and_max_on_the_member_axis_both_fail() {
    with_cube!(|c, client| {
        let two = mdx(
            &c,
            &client,
            &format!("SELECT MEASURES.[Amount] ON 0, MEASURES.[%COUNT] ON 1 FROM [{SALES}]"),
        )
        .await;
        let e = two.error.expect("two measure axes must fail");
        assert!(e.contains("Measures cannot exist on multiple axes"), "{e}");
        let max = mdx(
            &c,
            &client,
            &format!(
                "SELECT MEASURES.[Amount] ON 0, {{[RegionD].[H1].[Region].MEMBERS, \
                 MAX([RegionD].[H1].[Region].MEMBERS, MEASURES.[Amount])}} ON 1 FROM [{SALES}]"
            ),
        )
        .await;
        let e = max.error.expect("the PR's MAX form fails too");
        assert!(e.contains("Measures cannot exist on multiple axes"), "{e}");
    });
}

/// Claim: a cross-cube dimension reference raises `Invalid Member spec`. Reworded: it does in
/// %FILTER, but on an axis IRIS drops the axis with no error.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn cross_cube_dimension_errors_in_a_filter_and_vanishes_on_an_axis() {
    with_cube!(|c, client| {
        let f = mdx(
            &c,
            &client,
            &format!("SELECT FROM [{SALES}] %FILTER [OtherD].[H1].[Region].&[Asia]"),
        )
        .await;
        let e = f.error.expect("cross-cube filter must fail");
        assert!(e.starts_with("prepare: "), "{e}");
        assert!(e.contains("Invalid Member spec"), "{e}");
        let axis = mdx(
            &c,
            &client,
            &format!(
                "SELECT MEASURES.[Amount] ON 0, [OtherD].[H1].[Region].MEMBERS ON 1 FROM [{SALES}]"
            ),
        )
        .await;
        assert_eq!(axis.error, None);
        assert_eq!(axis.cols, ["Amount"]);
        assert!(axis.rows.is_empty() || axis.labels() == [""], "{axis:?}");
    });
}

/// Claims: the error text for a nonexistent cube and a nonexistent measure. Both hold; the measure
/// error arrives at %Execute, not %PrepareMDX.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn nonexistent_cube_and_measure_error_text() {
    with_cube!(|c, client| {
        let cube = mdx(&c, &client, "SELECT FROM [IadLive131Nope]").await;
        let e = cube.error.expect("unknown cube must fail");
        assert!(e.starts_with("prepare: "), "{e}");
        assert!(e.contains("Cannot find Subject Area"), "{e}");
        let measure = mdx(
            &c,
            &client,
            &format!("SELECT MEASURES.[Nope] ON 0 FROM [{SALES}]"),
        )
        .await;
        let e = measure.error.expect("unknown measure must fail");
        assert!(e.starts_with("execute: "), "{e}");
        assert!(e.contains("Measure not found"), "{e}");
    });
}

/// Claim: COUNT counts every member, EXCLUDEEMPTY only those with data. Holds.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn count_and_excludeempty() {
    with_cube!(|c, client| {
        let r = mdx(
            &c,
            &client,
            &format!(
                "WITH MEMBER MEASURES.[N] AS 'COUNT([RegionD].[H1].[Region].MEMBERS)' \
                 MEMBER MEASURES.[NE] AS 'COUNT([RegionD].[H1].[Region].MEMBERS, EXCLUDEEMPTY)' \
                 SELECT {{MEASURES.[N],MEASURES.[NE]}} ON 0 FROM [{SALES}] \
                 %FILTER [DateD].[Actual].[YearSold].&[2023]"
            ),
        )
        .await;
        assert_eq!(r.cols, ["N", "NE"]);
        assert_eq!(r.cell(0, 0), Some("3"));
        assert_eq!(r.cell(0, 1), Some("2"));
    });
}

// --- US1 live: sa_schema -------------------------------------------------------------------------

async fn call_info(name: &str) -> serde_json::Value {
    let bin = iris_agentic_dev_core::testing::iad_binary_path();
    let args = serde_json::json!({"what": "sa_schema", "name": name}).to_string();
    let out = tokio::process::Command::new(bin)
        .args(["tool", "iris_info", "-n", NS, "-a", &args])
        .env("IRIS_WRITE_TOOLS_ENABLED", "0")
        .env("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "0")
        .output()
        .await
        .expect("iris-agentic-dev must run");
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(text.trim()).unwrap_or_else(|e| panic!("{e}: {text}"))
}

/// FR-004 and US1 scenario 2: the deepsee URL returns the grammar (it 404ed before 131, because the
/// slashes were encoded), and a URL with no grammar returns the guidance.
#[tokio::test]
#[ignore = "requires live IRIS and the built binary"]
async fn sa_schema_returns_the_grammar_and_refuses_an_unknown_url() {
    let Some(_) = conn() else { return };
    let ok = call_info("http://www.intersystems.com/deepsee").await;
    let grammar = ok["result"]
        .as_array()
        .unwrap_or_else(|| panic!("grammar lines: {ok}"));
    assert!(grammar.len() > 20, "{ok}");
    assert!(
        grammar
            .iter()
            .any(|l| l.as_str().unwrap_or("").contains("deepsee")),
        "{ok}"
    );
    let nope = call_info("http://example.com/nope").await;
    let text = nope.to_string();
    assert!(text.contains("SA_SCHEMA_NOT_FOUND"), "{text}");
    assert!(text.contains("%GetCubeList"), "{text}");
}

/// When the cached grammar is missing or stale (a fresh instance, or any cube class compiled or
/// dropped since), Atelier starts a background build and answers 202 with an empty result. That
/// read as `SA_SCHEMA_NOT_FOUND` until iad waited for the build. Clearing the cache gives the 202.
#[tokio::test]
#[ignore = "requires live IRIS and the built binary"]
async fn sa_schema_waits_for_a_grammar_that_is_still_building() {
    let Some((c, client)) = conn() else { return };
    let out = run(
        &c,
        &client,
        " Kill ^IRIS.SASchema(\"http://www.intersystems.com/deepsee\") Write \"~[ok]~\"",
    )
    .await;
    assert!(out.contains("~[ok]~"), "clearing the cache: {out}");
    let ok = call_info("http://www.intersystems.com/deepsee").await;
    let grammar = ok["result"]
        .as_array()
        .unwrap_or_else(|| panic!("grammar lines after a cleared cache: {ok}"));
    assert!(grammar.len() > 20, "{ok}");
}

/// PR 142 review: the discovery and query snippets proposed for the skill's §14, run as written
/// with the fixture's cube, level and measure names swapped in.
#[tokio::test]
#[ignore = "requires live IRIS"]
async fn the_proposed_skill_snippets_run_as_written() {
    with_cube!(|c, client| {
        let cubes = run(
            &c,
            &client,
            r#"Set sc = ##class(%DeepSee.Utils).%GetCubeList(.list)
Set k = "" For { Set k = $Order(list(k)) Quit:k=""  Write $ListGet(list(k),1),! }"#,
        )
        .await;
        assert!(cubes.to_uppercase().contains("IADLIVE131SALES"), "{cubes}");

        let dims = run(
            &c,
            &client,
            r#"Set sc = ##class(%DeepSee.Utils).%GetDimensionList("IadLive131Sales", .info)
Set d = "" For { Set d = $Order(info(d)) Quit:d=""
    Set h = "" For { Set h = $Order(info(d,h)) Quit:h=""
        Set l = "" For { Set l = $Order(info(d,h,l)) Quit:l=""  Write $ListToString(info(d,h,l)),! } } }"#,
        )
        .await;
        assert!(dims.contains("l,RegionD,H1,Region"), "{dims}");
        assert!(dims.lines().any(|l| l.starts_with("m,")), "{dims}");

        let grid = run(
            &c,
            &client,
            r#"Set rs = ##class(%DeepSee.ResultSet).%New()
Set sc = rs.%PrepareMDX("SELECT MEASURES.[Amount] ON 0, NON EMPTY [RegionD].[H1].[Region].MEMBERS ON 1 FROM [IadLive131Sales]")
If $$$ISERR(sc) { Write $System.Status.GetErrorText(sc),! Quit }
Set sc = rs.%Execute()
If $$$ISERR(sc) { Write $System.Status.GetErrorText(sc),! Quit }
For r = 1:1:rs.%GetAxisSize(2) {
    Kill lab Set n = rs.%GetOrdinalLabel(.lab, 2, r)
    Write $Get(lab(1)), ": ", rs.%GetOrdinalValue(1, r),!
}"#,
        )
        .await;
        assert!(grid.contains("Asia: "), "{grid}");
        assert!(grid.contains("Europe: "), "{grid}");
    });
}
