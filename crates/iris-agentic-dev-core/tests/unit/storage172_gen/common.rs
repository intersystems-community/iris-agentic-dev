//! The generator's shared types and readers (T102).

use std::path::PathBuf;

/// One dimension: its name and values.
#[derive(Debug, Clone, PartialEq)]
pub struct Dim {
    pub name: String,
    pub values: Vec<String>,
}

/// A pair of (dimension, value) choices that IRIS refuses together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub a: (String, String),
    pub b: (String, String),
}

pub fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/storage172")
}

/// The dimensions in `dimensions.toml`.
pub fn dims() -> Vec<Dim> {
    let text = std::fs::read_to_string(fixture_dir().join("dimensions.toml")).unwrap();
    let t: toml::Table = text.parse().unwrap();
    t["dim"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| Dim {
            name: d["name"].as_str().unwrap().to_string(),
            values: d["values"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect(),
        })
        .collect()
}

/// The pairs in `forbidden.toml` (absent: none).
pub fn forbidden() -> Vec<Pair> {
    let Ok(text) = std::fs::read_to_string(fixture_dir().join("forbidden.toml")) else {
        return Vec::new();
    };
    let t: toml::Table = text.parse().unwrap();
    t.get("pair")
        .and_then(|p| p.as_array())
        .into_iter()
        .flatten()
        .map(|p| {
            let s = |k: &str| p[k].as_str().unwrap_or_default().to_string();
            Pair {
                a: (s("dim_a"), s("value_a")),
                b: (s("dim_b"), s("value_b")),
            }
        })
        .collect()
}
