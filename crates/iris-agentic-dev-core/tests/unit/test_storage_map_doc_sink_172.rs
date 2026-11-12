//! T079 (c): every `iris_doc` write, the SCM confirmation resume included, reaches the
//! storage-map safeguards, because they sit in `do_write` and `do_write` is the only place
//! `doc.rs` PUTs a document.

fn doc_rs() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/tools/doc.rs")).unwrap()
}

/// The text of `fn <name>` up to the next top-level `fn` or `async fn`.
fn body_of<'a>(src: &'a str, name: &str) -> &'a str {
    let start = src
        .find(&format!("async fn {name}("))
        .unwrap_or_else(|| panic!("no fn {name} in doc.rs"));
    let rest = &src[start + 1..];
    let end = [
        "\nasync fn ",
        "\nfn ",
        "\npub fn ",
        "\npub async fn ",
        "\n#[cfg(test)]",
    ]
    .iter()
    .filter_map(|m| rest.find(m))
    .min()
    .unwrap_or(rest.len());
    &src[start..start + 1 + end]
}

#[test]
fn test_storage_map_doc_sink_172_the_safeguards_sit_in_do_write() {
    let src = doc_rs();
    let sink = body_of(&src, "do_write");
    assert!(
        sink.contains("storage_map::write::doc_precheck("),
        "do_write does not call the storage-map safeguards"
    );
    assert!(
        sink.contains(".read_back("),
        "do_write does not read back a map write"
    );
    assert!(
        sink.contains(".restore("),
        "do_write does not restore after a failed compile"
    );
}

#[test]
fn test_storage_map_doc_sink_172_do_write_is_the_only_document_put() {
    let src = doc_rs();
    let code = src.split("\n#[cfg(test)]").next().unwrap();
    let sink = body_of(code, "do_write");
    let puts = code.matches(".put(&").count();
    let in_sink = sink.matches(".put(&").count();
    assert!(in_sink >= 1, "do_write has no PUT");
    assert_eq!(puts, in_sink, "doc.rs PUTs a document outside do_write");
    // Each write path calls it: put, surgical edits, and the confirmation resume.
    assert!(
        code.matches("do_write(").count() >= 4,
        "fewer do_write callers than expected"
    );
}
