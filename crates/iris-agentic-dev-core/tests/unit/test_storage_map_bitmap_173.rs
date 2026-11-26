//! Spec 173 T023: bitmap decode, the bitmap index draft and the chunk data-map draft. The chunk
//! draft is held to spec 172's `IadSm172.Bits` through its golden `read` answer, so the computed
//! third level is the one IRIS accepted, not a shape iad invented.

use iris_agentic_dev_core::tools::storage_map::bitmap::{bitmap_map, chunk_map_draft, decode_id};
use iris_agentic_dev_core::tools::storage_map::model::map_node;
use serde_json::{json, Value};

#[test]
fn decode_id_is_chunk_minus_one_times_64000_plus_position_minus_one() {
    assert_eq!(decode_id(1, 2), 1);
    assert_eq!(decode_id(2, 1), 64000);
    assert_eq!(decode_id(1, 64000), 63999);
    assert_eq!(decode_id(3, 5), 2 * 64000 + 4);
}

#[test]
fn the_bitmap_map_has_one_subscript_the_field() {
    let m = bitmap_map("Status", "^IadSm173Bm.X", "{Status}");
    assert_eq!(m["name"], "Status");
    assert_eq!(m["type"], "bitmap");
    assert_eq!(m["global"], "^IadSm173Bm.X");
    let subs = m["subscripts"].as_array().unwrap();
    assert_eq!(subs.len(), 1, "the chunk is appended by IRIS: {m}");
    assert_eq!(subs[0]["expression"], "{Status}");
    assert_eq!(subs[0]["level"], 1);
    map_node(&m).expect("the model accepts the bitmap map");
}

#[test]
fn the_bitmap_map_keeps_a_collation_spelling() {
    let m = bitmap_map("Name", "^G.X", "$$SQLUPPER({Name})");
    assert_eq!(m["subscripts"][0]["expression"], "$$SQLUPPER({Name})");
    map_node(&m).expect("accepted");
}

fn golden_subscripts() -> Vec<Value> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/golden/Bits.read.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    v["storage"]["maps"][0]["subscripts"]
        .as_array()
        .unwrap()
        .clone()
}

/// A draft leaves a default out; `read` writes it. Fill the default so the two compare.
fn filled(sub: &Value, key: &str) -> Value {
    match (sub.get(key), key) {
        (Some(v), _) => v.clone(),
        (None, "access_type") => json!("Sub"),
        (None, "access_vars" | "invalid_conditions") => json!([]),
        (None, _) => Value::Null,
    }
}

#[test]
fn the_chunk_draft_has_the_computed_third_level_of_the_bits_fixture() {
    let (draft, guesses) = chunk_map_draft("^IadSm173Bits", "0", "Key1").unwrap();
    assert_eq!(draft["type"], "data");
    assert_eq!(draft["global"], "^IadSm173Bits");
    let mine = draft["subscripts"].as_array().unwrap();
    let gold = golden_subscripts();
    assert_eq!(mine.len(), 3);
    // Everything but the key's name equals what IRIS accepted for `IadSm172.Bits`.
    for (m, g) in mine.iter().zip(&gold) {
        for key in [
            "level",
            "access_type",
            "access_vars",
            "invalid_conditions",
            "next_code",
        ] {
            assert_eq!(filled(m, key), g[key], "level {}: {key}", g["level"]);
        }
    }
    assert_eq!(mine[0]["expression"], "0");
    assert_eq!(mine[1]["expression"], "{Key1}\\64000+1");
    assert_eq!(mine[2]["expression"], "{Key1}");
    // One access variable, one next-code, one invalid condition, on the levels that hold them.
    assert_eq!(mine[1]["access_vars"].as_array().unwrap().len(), 1);
    assert_eq!(mine[2]["access_type"], "Other");
    assert_eq!(mine[2]["invalid_conditions"].as_array().unwrap().len(), 1);
    assert!(mine[2]["next_code"].as_str().unwrap().contains("$bitfind"));
    // The draft goes back through the model.
    map_node(&draft).expect("the model accepts the chunk draft");
    // Every guessed element is named: the constant, the chunk size and the key name.
    for path in ["/maps/Master/subscripts/1", "/maps/Master/subscripts/2"] {
        assert!(guesses.iter().any(|g| g == path), "{path}: {guesses:?}");
    }
}

#[test]
fn the_constant_must_be_a_literal() {
    for good in ["0", "7", "-1", "\"x\"", "\"a\"\"b\""] {
        assert!(chunk_map_draft("^G", good, "Key1").is_ok(), "{good}");
    }
    for bad in ["", "{ID}", "$$X()", "1)_(2", "\"a\"b\"", "x y"] {
        assert!(chunk_map_draft("^G", bad, "Key1").is_err(), "{bad:?}");
    }
}

#[test]
fn the_key_name_must_be_a_property_name() {
    assert!(chunk_map_draft("^G", "0", "Key1").is_ok());
    for bad in ["", "1Key", "a b", "K}", "x\\y"] {
        assert!(chunk_map_draft("^G", "0", bad).is_err(), "{bad:?}");
    }
    let _ = json!(null);
}
