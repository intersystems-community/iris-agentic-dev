//! Sampling a global for `compare`, `preview` and `propose` (US5, FR-020, FR-030b). iad writes the
//! ObjectScript; the only caller text in it is the global name, validated and quoted by iad.

use super::globals::GlobalRef;
use super::shape::{Cap, ShapeReport};
use serde_json::{json, Value};

/// The candidate delimiters, in the order they are tried: `^`, `|`, `~`, `$C(1)`, `,`.
const DELIMITERS: &str = "$C(94),$C(124),$C(126),$C(1),$C(44)";

/// The sampler for `global`: walk its value nodes in `$Query` order from the root, stop after
/// `max_nodes` nodes or `seconds` seconds, and write one JSON line per node (depth, `$Data`,
/// literal last subscript, format, delimiter, piece count; the value only with
/// `include_values`), then a `done` line. A node holding only children (`$Data` 10) is reported
/// once when the walk first passes under it.
pub fn build_code(
    global: &GlobalRef,
    max_nodes: u32,
    seconds: u32,
    include_values: bool,
) -> Result<String, String> {
    if global.extended.is_some() || global.subscripts_prefix.is_some() {
        return Err(format!(
            "{}: only a plain global name can be sampled",
            global.name
        ));
    }
    let bare = global.name.strip_prefix('^').unwrap_or(&global.name);
    if crate::objectscript::validate_global_name(bare).is_err() {
        return Err(format!("{:?} is not a global name", global.name));
    }
    let name = crate::objectscript::os_str_expr(&format!("^{bare}"));
    let vals = u8::from(include_values);
    Ok(format!(
        "Set g={name},max={max_nodes},secs={seconds},vals={vals},t0=$ZH,c=0,capped=0,n=g,first=1
 For {{
  If first {{ Set first=0 If '($Data(@g)#2) {{ Set n=$Query(@g) }} }} Else {{ Set n=$Query(@n) }}
  Quit:n=\"\"
  If (c>=max)||(($ZH-t0)>secs) {{ Set capped=1 Quit }}
  Set d=$QLength(n)
  For k=1:1:d-1 {{ Set a=$Name(@n,k) If '$Data(seen(a)),$Data(@a)=10 {{ Set seen(a)=1,c=c+1,o={{}},o.d=k,o.s=10 Set:'$IsValidNum($QSubscript(a,k)) o.n=$QSubscript(a,k) Write o.%ToJSON(),! }} }}
  Set c=c+1,v=$Get(@n),o={{}},o.d=d,o.s=$Data(@n)
  Set:(d>0)&&'$IsValidNum($QSubscript(n,d)) o.n=$QSubscript(n,d)
  If v=\"\" {{ Set o.f=\"plain\",o.p=0 }}
  ElseIf $ListValid(v)&&($ListLength(v)>0) {{ Set o.f=\"$LIST\",o.p=$ListLength(v) }}
  Else {{
   Set isb=0 Try {{ Set bc=$BitCount(v),isb=1 }} Catch bx {{ }}
   If isb {{ Set o.f=\"bits\",o.p=1 }}
   Else {{ Set o.f=\"plain\",o.p=1 For dl={DELIMITERS} {{ If v[dl {{ Set o.f=\"delimited\",o.dl=$Ascii(dl),o.p=$Length(v,dl) Quit }} }} }}
  }}
  Set:vals o.v=$Extract(v,1,200)
  Write o.%ToJSON(),!
 }}
 Write {{\"done\":1,\"c\":(c),\"capped\":(capped)}}.%ToJSON(),!"
    ))
}

/// The report from the sampler's output: every node line folded in, and the `done` line
/// required, so an IRIS error or a cut-off answer is an error, not a short report.
pub fn parse(out: &str) -> Result<ShapeReport, String> {
    let mut report = ShapeReport::default();
    let mut done = None;
    for line in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let row: Value = serde_json::from_str(line)
            .map_err(|_| format!("the sampler wrote something unexpected: {line}"))?;
        if row["done"] == 1 {
            done = Some(row);
        } else {
            report.add(&row);
        }
    }
    let done = done.ok_or("the sampler stopped before it finished")?;
    report.nodes_scanned = done["c"].as_u64().unwrap_or(0) as u32;
    report.capped = done["capped"] == 1;
    Ok(report)
}

/// The caps after clamping: nodes 1..1000 (default 200), at most 5 seconds.
pub fn caps(max_nodes: Option<u64>) -> Cap {
    Cap {
        nodes: max_nodes.unwrap_or(200).clamp(1, 1000) as u32,
        seconds: 5,
    }
}

/// Sample `global` on `iris` (the call's execute identity: the service account when one is
/// configured, T036) and read the report. `Err` is the answer to send back.
pub async fn sample(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    ns: &str,
    global: &GlobalRef,
    cap: Cap,
    include_values: bool,
) -> Result<ShapeReport, Value> {
    let fail = |code: &str, msg: String| super::write::body(code, msg, json!({}));
    let code = build_code(global, cap.nodes, cap.seconds, include_values)
        .map_err(|e| fail("INVALID_PARAMS", e))?;
    let out = iris
        .execute_via_generator_prefixed(&code, ns, client, "IrisDevSm")
        .await
        .map_err(|e| {
            fail(
                "IRIS_EXECUTE_ERROR",
                format!("sampling {}: {e}", global.name),
            )
        })?;
    let mut report = parse(&out).map_err(|e| fail("IRIS_EXECUTE_ERROR", e))?;
    report.global = global.name.clone();
    report.cap = cap;
    Ok(report)
}

/// The refusal for a call that asks for the values a global holds (`include_values`, FR-021), or
/// `None`: under `allow` they need `acknowledgePhi`; under `block` and `redact` they are refused,
/// since iad cannot tell which nodes hold PHI. Without `include_values` there is nothing to refuse:
/// shapes, counts and masked rows carry no values.
pub fn values_refusal(
    include_values: bool,
    acknowledged: bool,
    data_policy: &str,
    server: &str,
) -> Option<Value> {
    if !include_values {
        return None;
    }
    match data_policy {
        "allow" if acknowledged => None,
        "allow" => Some(json!({
            "success": false,
            "error_code": "PHI_ACK_REQUIRED",
            "server_name": server,
            "tool_name": "iris_storage_map",
            "error": "dataPolicy=allow requires acknowledgePhi=true for iris_storage_map \
                      include_values: the answer holds the global's values, which may contain PHI.",
        })),
        other => Some(json!({
            "success": false,
            "error_code": "DATA_POLICY_BLOCKED",
            "data_policy_blocked": true,
            "server_name": server,
            "data_policy": other,
            "tool_name": "iris_storage_map",
            "error": format!(
                "iris_storage_map include_values returns the global's values, and dataPolicy is \
                 '{other}' for server '{server}'. The shape, counts and masked rows are still \
                 available without include_values."
            ),
        })),
    }
}
