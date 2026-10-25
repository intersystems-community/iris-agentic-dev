//! Spec 141 US3 (FR-012, FR-014, FR-016, FR-017): which compile-time constructs the gate refuses.
//!
//! The live proof that each one runs code on IRIS is `test_compile_gate_141_live`; these pin the
//! scanner. Every refusal test has a twin that must pass, so a scanner that refuses everything
//! cannot go green.

use iris_agentic_dev_core::policy::code_edit_gate::check_compile_time_code_mode;

fn class_with_body(body: &str) -> String {
    format!("Class My.X\n{{\n\nClassMethod M() As %Status\n{{\n{body}\n    Quit $$$OK\n}}\n\n}}\n")
}

fn refused(content: &str, doc: &str) -> serde_json::Value {
    check_compile_time_code_mode(content, doc)
        .unwrap_or_else(|| panic!("{doc} should be refused:\n{content}"))
}

/// Asserts the gate refuses `content` stored as `doc`.
macro_rules! assert_refused {
    ($content:expr, $doc:expr $(,)?) => {
        assert!(
            check_compile_time_code_mode($content, $doc).is_some(),
            "{} should be refused:\n{}",
            $doc,
            $content
        )
    };
}

/// Asserts the gate lets `content` stored as `doc` through.
macro_rules! assert_allowed {
    ($content:expr, $doc:expr $(,)?) => {
        if let Some(r) = check_compile_time_code_mode($content, $doc) {
            panic!("{} should be allowed, got {r}\n{}", $doc, $content);
        }
    };
}

// ── Preprocessor directives in .cls ──────────────────────────────────────

#[test]
fn execute_directive_is_refused_in_a_method_body() {
    let cls = class_with_body("    #execute Do ##class(My.Probe).Mark(\"exec\")");
    let r = refused(&cls, "My.X.cls");
    assert_eq!(r["error_code"], "COMPILE_TIME_EXEC_BLOCKED");
    assert_eq!(r["line"], 6, "the directive is on line 6: {r}");
    assert!(r["matched"].as_str().unwrap().contains("#execute"), "{r}");
}

#[test]
fn directives_are_matched_in_any_case() {
    for d in ["#EXECUTE Do x", "#Execute Do x", "    #eXeCuTe Do x"] {
        assert_refused!(&class_with_body(d), "My.X.cls");
    }
}

#[test]
fn expression_and_function_directives_are_refused() {
    assert_refused!(
        &class_with_body("    Set x = ##expression(##class(My.Probe).Mark(\"e\"))"),
        "My.X.cls",
    );
    assert_refused!(
        &class_with_body("    Set x = ##function(##class(My.Probe).Mark(\"f\"))"),
        "My.X.cls",
    );
    assert_refused!(&class_with_body("    Set x = ##EXPRESSION(1)"), "My.X.cls");
}

#[test]
fn a_directive_hidden_behind_a_macro_definition_is_refused() {
    let cls = "#define Hidden ##expression(##class(My.Probe).Mark(\"d\"))\nClass My.X\n{\n}\n";
    assert_refused!(cls, "My.X.cls");
}

#[test]
fn an_if_directive_that_calls_code_is_refused() {
    for cond in [
        "##class(My.Probe).Mark(\"if\")",
        "$$check^Rtn()",
        "$ZF(-1,\"ls\")",
        "$Get(^Gate)",
        "@x",
        "..Check()",
    ] {
        let body = format!("    #if {cond}\n    Set x = 1\n    #endif");
        assert_refused!(&class_with_body(&body), "My.X.cls");
    }
    let body = "    #if 0\n    Set x = 1\n    #elseif ##class(My.Probe).Mark(\"elif\")\n    Set x = 2\n    #endif";
    let r = refused(&class_with_body(body), "My.X.cls");
    assert!(r["matched"].as_str().unwrap().contains("#elseif"), "{r}");
}

#[test]
fn if_directives_that_only_test_literals_and_macros_pass() {
    for body in [
        "    #if 0\n    Set x = 1\n    #endif",
        "    #if $$$DEBUG\n    Set x = 1\n    #endif",
        "    #if ($$$A&&'$$$B)\n    Set x = 1\n    #else\n    Set x = 2\n    #endif",
        "    #ifdef MYFLAG\n    Set x = 1\n    #endif",
        "    #ifndef MYFLAG\n    Set x = 1\n    #endif",
        "    #define Local 1\n    Set x = $$$Local",
        "    #; a comment directive",
    ] {
        assert_allowed!(&class_with_body(body), "My.X.cls");
    }
}

#[test]
fn embedded_sql_directive_is_not_refused() {
    // Measured on 2026.2: `##sql(...)` is embedded SQL that runs at call time, not at compile time.
    assert_allowed!(&class_with_body("    ##sql(SELECT 1 INTO :x)"), "My.X.cls");
}

#[test]
fn a_construct_in_a_comment_or_string_is_not_a_construct() {
    for body in [
        "    // #execute Do x",
        "    /* ##expression(1) */",
        "    ; #execute Do x",
        "    Set note = \"#execute Do x\"",
        "    Set note = \"use ##expression(1) here\"",
        "    /// ##function(1)",
    ] {
        assert_allowed!(&class_with_body(body), "My.X.cls");
    }
    let doc = "/// Never write #execute in this class.\nClass My.X\n{\n}\n";
    assert_allowed!(doc, "My.X.cls");
}

// ── Braced parameters ────────────────────────────────────────────────────

#[test]
fn braced_parameter_values_are_refused() {
    for decl in [
        "Parameter P = {##class(My.Probe).Mark(\"p\")};",
        "Parameter P As %String = {1+1};",
        "Parameter P [ Final ] = {$Get(^X)};",
        "Parameter P As %String [ Internal ] =\n    {##class(My.Probe).Mark(\"p\")};",
        "parameter p = {1};",
    ] {
        let cls = format!("Class My.X\n{{\n\n{decl}\n\n}}\n");
        let r = refused(&cls, "My.X.cls");
        assert!(
            r["matched"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("parameter"),
            "{r}"
        );
    }
}

#[test]
fn ordinary_parameters_pass() {
    for decl in [
        "Parameter P = \"x\";",
        "Parameter P = 3;",
        "Parameter P As %String = \"{not braced}\";",
        "Parameter P;",
        "Parameter P [ Final ] = 1;",
    ] {
        let cls = format!("Class My.X\n{{\n\n{decl}\n\n}}\n");
        assert_allowed!(&cls, "My.X.cls");
    }
}

// ── Projections ──────────────────────────────────────────────────────────

#[test]
fn a_projection_member_with_a_user_class_is_refused() {
    let cls = "Class My.X\n{\n\nProjection P As My.Proj;\n\n}\n";
    let r = refused(cls, "My.X.cls");
    assert!(
        r["matched"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("projection"),
        "{r}"
    );
    assert_refused!(
        "Class My.X\n{\nProjection P As My.Proj(Key = 1);\n}\n",
        "My.X.cls",
    );
    assert_refused!(
        "Class My.X\n{\nprojection P\n    as My.Proj;\n}\n",
        "My.X.cls",
    );
}

#[test]
fn a_projection_to_a_system_projection_class_passes() {
    assert_allowed!(
        "Class My.X\n{\nProjection J As %Projection.Java;\n}\n",
        "My.X.cls",
    );
    assert_allowed!(
        "Class My.X\n{\nProjection X As %Projection.XML;\n}\n",
        "My.X.cls",
    );
}

// ── Method generators ────────────────────────────────────────────────────

#[test]
fn a_quoted_codemode_value_is_refused() {
    let cls =
        "Class My.X\n{\nClassMethod G() As %Status [ CodeMode = \"objectgenerator\" ]\n{\n}\n}\n";
    let r = refused(cls, "My.X.cls");
    assert_eq!(r["matched"], "CodeMode = objectgenerator");
    let cls = "Class My.X\n{\nClassMethod G() As %Status [ CodeMode = \"generator\" ]\n{\n}\n}\n";
    assert_eq!(refused(cls, "My.X.cls")["matched"], "CodeMode = generator");
}

#[test]
fn a_quoted_value_that_only_looks_like_a_generator_passes() {
    assert_allowed!(
        "Class My.X\n{\nClassMethod G() As %Status [ CodeMode = \"generators\" ]\n{\n}\n}\n",
        "My.X.cls",
    );
}

#[test]
fn expression_and_call_modes_are_accepted() {
    // FR-016: neither runs code at compile time (measured on 2026.2: the sentinel is set only
    // when the method is called).
    assert_allowed!(
        "Class My.X\n{\nClassMethod E() As %String [ CodeMode = expression ]\n{\n1+1\n}\n}\n",
        "My.X.cls",
    );
    assert_allowed!(
        "Class My.X\n{\nClassMethod C() [ CodeMode = call ]\n{\nEntry^Rtn()\n}\n}\n",
        "My.X.cls",
    );
    assert_allowed!(
        "Class My.X\n{\nClassMethod E() As %String [ CodeMode = \"expression\" ]\n{\n1+1\n}\n}\n",
        "My.X.cls",
    );
}

// ── .mac and .inc ────────────────────────────────────────────────────────

#[test]
fn routines_and_includes_with_directives_are_refused() {
    let mac = "ROUTINE My\nMy ;\n    #execute Do ##class(My.Probe).Mark(\"mac\")\n    Quit\n";
    let r = refused(mac, "My.mac");
    assert_eq!(r["error_code"], "COMPILE_TIME_EXEC_BLOCKED");
    assert_eq!(r["line"], 3);
    assert_refused!(
        "ROUTINE My\nMy ;\n    Set x = ##expression(1)\n    Quit\n",
        "My.mac",
    );
    assert_refused!(
        "ROUTINE My [Type=INC]\n#execute Do ##class(My.Probe).Mark(\"inc\")\n",
        "My.inc",
    );
    assert_refused!("#define Hidden ##expression(1)\n", "My.INC");
    assert_refused!(
        "My ;\n #if ##class(My.Probe).Mark(\"x\")\n Quit\n #endif\n",
        "My.mac",
    );
}

#[test]
fn clean_routines_and_includes_pass() {
    assert_allowed!("ROUTINE My\nMy ;\n    Write 1\n    Quit\n", "My.mac");
    assert_allowed!(
        "ROUTINE My [Type=INC]\n#define Debug 1\n#define Twice(%x) (%x*2)\n",
        "My.inc",
    );
    assert_allowed!(
        "My ;\n ; #execute is mentioned here\n Set x = \"##expression(1)\"\n Quit\n",
        "My.mac",
    );
}

#[test]
fn the_int_document_is_not_scanned() {
    // `.int` is the preprocessor's output: compiling it runs no preprocessor, so it has no
    // compile-time code. Documented in the gate, pinned here.
    assert_allowed!("My ;\n #execute Do x\n Quit\n", "My.int");
    assert_allowed!("ClassMethod G() [ CodeMode = objectgenerator ]", "My.csp");
}

// ── Class XML under a .cls name (FR-014) ─────────────────────────────────

fn xml_class(member: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Export generator=\"IRIS\" version=\"26\">\n\
         <Class name=\"My.X\">\n{member}\n</Class>\n</Export>\n"
    )
}

#[test]
fn class_xml_with_a_generator_is_refused() {
    let xml = xml_class(
        "<Method name=\"G\"><ClassMethod>1</ClassMethod><CodeMode>objectgenerator</CodeMode>\n\
         <Implementation><![CDATA[ Quit $$$OK ]]></Implementation></Method>",
    );
    let r = refused(&xml, "My.X.cls");
    assert_eq!(r["error_code"], "COMPILE_TIME_EXEC_BLOCKED");
    assert_eq!(r["line"], 4, "the element is on line 4: {r}");
    assert!(r["matched"].as_str().unwrap().contains("CodeMode"), "{r}");
    assert_refused!(
        &xml_class("<Method name=\"G\"><CodeMode> generator </CodeMode></Method>"),
        "My.X.cls",
    );
}

#[test]
fn class_xml_with_the_other_constructs_is_refused() {
    assert_refused!(
        &xml_class(
            "<Parameter name=\"P\"><Default>{##class(My.Probe).Mark(\"p\")}</Default></Parameter>",
        ),
        "My.X.cls",
    );
    assert_refused!(
        &xml_class("<Projection name=\"P\"><Type>My.Proj</Type></Projection>"),
        "My.X.cls",
    );
    assert_refused!(
        &xml_class(
            "<Method name=\"M\"><Implementation><![CDATA[\n #execute Do x\n Quit 1\n]]></Implementation></Method>",
        ),
        "My.X.cls",
    );
    // Not just `<?xml`: an export that starts at its root element.
    assert_refused!(
        "<Export generator=\"IRIS\" version=\"26\"><Class name=\"My.X\"><Method name=\"G\">\
         <CodeMode>objectgenerator</CodeMode></Method></Class></Export>",
        "My.X.cls",
    );
}

#[test]
fn class_xml_without_the_constructs_passes() {
    assert_allowed!(
        &xml_class(
            "<Method name=\"M\"><CodeMode>expression</CodeMode>\
             <Implementation><![CDATA[1+1]]></Implementation></Method>\n\
             <Parameter name=\"P\"><Default>3</Default></Parameter>",
        ),
        "My.X.cls",
    );
}

// ── The refusal (FR-017's body half; isError is asserted live and in the binary tests) ──

#[test]
fn the_refusal_names_the_construct_and_the_fix() {
    let r = refused(&class_with_body("    #execute Do x"), "My.X.cls");
    assert_eq!(r["code_edit_blocked"], true);
    assert_eq!(r["document"], "My.X.cls");
    let msg = r["message"].as_str().unwrap();
    assert!(msg.contains("#execute") && msg.contains("line 6"), "{msg}");
    let rem = r["remediation"].as_str().unwrap();
    assert!(
        rem.contains("ClassMethod") || rem.contains("method"),
        "{rem}"
    );
}
