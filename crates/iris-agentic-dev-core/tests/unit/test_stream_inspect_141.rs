//! Spec 141 T199: how `stream_inspect` reads a generator run back.
//!
//! The live half is `tests/integration/test_stream_inspect_141_live.rs`; these cover the shapes a
//! live run cannot be made to produce on demand (a runtime error mid-read, a cut reply).

use iris_agentic_dev_core::tools::admin_tools::{
    build_stream_inspect_code, parse_stream_inspect_output, StreamRead,
};

fn b64(text: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(text.as_bytes())
}

/// A run's output with `pieces` as its `B|` lines. `read` is the count the run declares.
fn reply_pieces(size: i64, truncated: bool, pieces: &[&str], read: usize) -> String {
    let body: String = pieces.iter().map(|p| format!("B|{}\n", b64(p))).collect();
    format!(
        "SIZE|{size}\nCLASS|%Stream.GlobalCharacter\n{body}READ|{read}\nTRUNCATED|{}\nEND|\n",
        u8::from(truncated)
    )
}

fn reply(size: i64, truncated: bool, content: &str) -> String {
    reply_pieces(size, truncated, &[content], content.chars().count())
}

#[test]
fn a_not_found_reply_is_not_found() {
    assert_eq!(
        parse_stream_inspect_output("ERROR:STREAM_NOT_FOUND\n"),
        StreamRead::NotFound
    );
}

/// The content comes from a stream the caller does not control. Text in it that looks like a
/// marker or an error is content.
#[test]
fn content_that_reads_like_a_marker_is_content() {
    let body = "ERROR:STREAM_NOT_FOUND\nB|x\nEND|\nERROR: <MAXSTRING>";
    match parse_stream_inspect_output(&reply(61, false, body)) {
        StreamRead::Read {
            size,
            content,
            truncated,
            ..
        } => {
            assert_eq!(size, 61);
            assert!(!truncated);
            assert_eq!(content, body);
        }
        other => panic!("expected the content back, got {other:?}"),
    }
}

#[test]
fn line_breaks_in_the_content_survive() {
    let body = "MSH|^~\\&|A\rPID|1||X\r\nNTE|1";
    match parse_stream_inspect_output(&reply(27, true, body)) {
        StreamRead::Read {
            content, truncated, ..
        } => {
            assert_eq!(content, body);
            assert!(truncated);
        }
        other => panic!("expected the content back, got {other:?}"),
    }
}

/// A runtime error is a failure, not an empty stream: the old code had no `CONTENT|` marker to find
/// and answered `size: 0, content: ""`.
#[test]
fn a_runtime_error_is_a_failure_with_its_message() {
    for out in [
        "ERROR: <MAXSTRING>zRead+3^IadStream.1\n",
        "ERROR($ZERROR): <MAXSTRING>\n",
        "ERROR($DEVICE): output went elsewhere\n",
    ] {
        match parse_stream_inspect_output(out) {
            StreamRead::Failed(msg) => {
                assert!(
                    !msg.is_empty() && !msg.starts_with("ERROR"),
                    "{out:?} -> {msg:?}"
                )
            }
            other => panic!("{out:?}: expected Failed, got {other:?}"),
        }
    }
}

#[test]
fn an_empty_or_cut_reply_is_a_failure_not_an_empty_stream() {
    let good = reply(3, false, "abc");
    let no_end = good.replace("END|\n", "");
    let no_truncated = good.replace("TRUNCATED|0\n", "");
    let no_read = good.replace("READ|3\n", "");
    // Characters were lost on the way: IRIS read 3 and 2 arrived.
    let short = reply_pieces(3, false, &["ab"], 3);
    // A piece that is not base64.
    let garbled = good.replace(&format!("B|{}", b64("abc")), "B|!!not base64!!");
    for (label, out) in [
        ("empty", String::new()),
        (
            "header only",
            "SIZE|10\nCLASS|%Stream.GlobalCharacter\n".to_string(),
        ),
        ("no end marker", no_end),
        ("no truncation line", no_truncated),
        ("no count line", no_read),
        ("short", short),
        ("garbled piece", garbled),
    ] {
        assert!(
            matches!(parse_stream_inspect_output(&out), StreamRead::Failed(_)),
            "{label}: {out:?} must not read as a stream"
        );
    }
}

/// The content survives as many pieces, in order, and a multi-byte character comes back whole.
#[test]
fn pieces_join_in_order_and_unicode_survives() {
    match parse_stream_inspect_output(&reply_pieces(7, false, &["ab", "é€", "cd"], 6)) {
        StreamRead::Read { content, .. } => assert_eq!(content, "abé€cd"),
        other => panic!("expected the content back, got {other:?}"),
    }
}

#[test]
fn the_program_tests_existence_before_opening_and_caps_the_read() {
    let code = build_stream_inspect_code("42", 777);
    let exists = code.find("%ExistsId").expect("%ExistsId decides not-found");
    let open = code.find("%OpenId").expect("the stream is opened");
    assert!(exists < open, "{code}");
    assert!(code.contains("tLeft=777"), "{code}");
    assert!(code.contains("tStream.Read("), "{code}");
    assert!(
        code.contains("Try {") && code.contains("Catch tEx"),
        "{code}"
    );
    assert!(
        !code.contains("Rewind"),
        "inspecting must not write to the stream: {code}"
    );
}

#[test]
fn a_hostile_oid_stays_inside_its_string_literal() {
    let code = build_stream_inspect_code("1\"\n Kill ^IadSentinel141\n Set x=\"", 10);
    assert!(
        code.lines()
            .all(|l| !l.trim_start().starts_with("Kill ^Iad")),
        "{code}"
    );
}
