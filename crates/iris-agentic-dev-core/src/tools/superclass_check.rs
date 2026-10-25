//! The superclass check behind every compile (spec 141, FR-015a).
//!
//! IRIS compiles more than the classes a compile names: a target whose superclass is out of date
//! gets that superclass compiled with it, and the superclass's `CodeMode = objectgenerator`
//! methods, projections and the like run in the process that serves the request. The stored-source
//! check ([`super::stored_check`]) reads the classes named in the compile and the `.inc` files they
//! include, nothing else. So a class stored by any path the gate did not see, with a generator,
//! runs unread when a subclass is compiled.
//!
//! Before a compile, this module walks the `Extends` chain of each class to be compiled, on the
//! target server, and finds every superclass outside the library packages that IRIS would
//! recompile. The compile is refused with `COMPILE_SUPERCLASS_STALE`, naming the superclass, so the
//! agent compiles it first and its own stored-source check runs. The superclass's compile goes
//! through the gate like any other.
//!
//! "IRIS would recompile it" is `$SYSTEM.OBJ.IsUpToDate(class)` returning 0: the compiler's own
//! test, with its default of ignoring changes to descriptions. It returns 1 for a class in a
//! read-only database, which is where the library lives.
//!
//! The check fails closed. A walk that cannot run, or whose reply is not a finished walk, is
//! refused (`SUPERCLASS_CHECK_FAILED`), never passed.
//!
//! One snippet of ObjectScript does the walk, run by the same two transports `stored_check` uses:
//! the generator class over HTTP and `iris session` over `docker exec` / SSH.

use std::collections::HashSet;

use crate::iris::connection::{is_generator_error, IrisConnection};
use crate::objectscript::os_str_expr;
use crate::tools::stored_check::{self, Outcome, MARK};

pub const ERR_SUPERCLASS_STALE: &str = "COMPILE_SUPERCLASS_STALE";
pub const ERR_SUPERCLASS_UNCHECKED: &str = "SUPERCLASS_CHECK_FAILED";

/// Class names that start with one of these are the library's. Each ends in the package dot (or is
/// the `%` prefix), so a user package that merely starts with the letters (`EnsembleDemo`,
/// `HSApp`) is not library. The spec writes `Ens*` and `HS*`; this is the conservative reading,
/// which checks more.
pub const LIBRARY_PREFIXES: &[&str] = &[
    "%",
    "Ens.",
    "EnsLib.",
    "EnsPortal.",
    "HS.",
    "HSLIB.",
    "HSMOD.",
];

/// Inside a library package, the part that is the customer's own code. HealthShare keeps local
/// customisations in `HS.Local`, in a database that can be written.
pub const USER_PREFIXES_IN_LIBRARY: &[&str] = &["HS.Local."];

/// How many classes one walk takes. The class list is a string literal on one line of generated
/// code, so it stays short; a larger compile is walked in chunks.
pub const WALK_CHUNK: usize = 250;

/// Is `name` a class of the library packages, which this check leaves alone?
pub fn is_library_class(name: &str) -> bool {
    LIBRARY_PREFIXES.iter().any(|p| name.starts_with(p))
        && !USER_PREFIXES_IN_LIBRARY.iter().any(|p| name.starts_with(p))
}

/// The class names among `docs`: a document with a `.cls` extension, extension removed, and a
/// name with no known extension (a class). Routines and includes have no superclass. A name with
/// no package is filed under `User`.
pub fn class_names(docs: &[String]) -> Vec<String> {
    docs.iter()
        .filter_map(|d| {
            let lower = d.to_ascii_lowercase();
            if lower.ends_with(".cls") {
                Some(d[..d.len() - 4].to_string())
            } else if [".mac", ".inc", ".int"].iter().any(|e| lower.ends_with(e)) {
                None
            } else {
                Some(d.clone())
            }
        })
        .filter(|n| !n.is_empty())
        .map(|n| {
            if n.contains('.') || n.starts_with('%') {
                n
            } else {
                format!("User.{n}")
            }
        })
        .collect()
}

/// A superclass IRIS would recompile, and the class that needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stale {
    pub superclass: String,
    pub needed_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Walk {
    Clear,
    Stale(Vec<Stale>),
    /// The walk did not finish. Carries what IRIS said, or that it said nothing.
    Failed(String),
}

/// The walk, as ObjectScript. `@NAME@` marks what [`walk_code`] fills in; the lines are joined
/// into one, because the terminal reads one line.
///
/// A name with no package is filed under `User`, or under a package the class imports: Atelier
/// does the first on a put, and the compiler does both when it resolves `Extends`. A name that
/// resolves to no stored definition is left to the compiler.
const WALK: &str = r#"
Set l=@LIST@,lib=@LIB@,ex=@USER@,n=0,h=0,bad=""
For i=1:1:$Length(l,",") { Set c=$Piece(l,",",i) If c'="",'$Data(s(c)) { Set s(c)="",n=n+1,q(n)=c } }
For {
Set h=h+1 If h>n { Quit }
Set c=q(h)
If ##class(%Dictionary.ClassDefinition).%ExistsId(c) {
Set d=##class(%Dictionary.ClassDefinition).%OpenId(c)
If '$IsObject(d) { Set bad="could not open the definition of "_c Quit }
Set sp=d.Super,im=$Translate(d.Import,"() ","")
For j=1:1:$Length(sp,",") {
Set p=$ZStrip($Piece(sp,",",j),"<>W")
If p'="",$Extract(p)'="%",p'["." {
Set r=""
For k=0:1:$Length(im,",") { Set pk=$Select(k=0:"User",1:$Piece(im,",",k)) If pk'="",r="",##class(%Dictionary.ClassDefinition).%ExistsId(pk_"."_p) { Set r=pk_"."_p } }
If r'="" { Set p=r }
}
If p'="",'$Data(s(p)) {
Set s(p)="",u=1
For k=1:1:$Length(lib,"|") { Set x=$Piece(lib,"|",k) If x'="",$Extract(p,1,$Length(x))=x { Set u=0 } }
If 'u { For k=1:1:$Length(ex,"|") { Set x=$Piece(ex,"|",k) If x'="",$Extract(p,1,$Length(x))=x { Set u=1 } } }
If u,##class(%Dictionary.ClassDefinition).%ExistsId(p) {
Set n=n+1,q(n)=p
If '$SYSTEM.OBJ.IsUpToDate(p) { Write !,"@MARK@-STALE~ ",p," ",c,! }
}
}
}
}
}
If bad'="" { Write !,"@MARK@-ERROR~ ",bad,! } Else { Write !,"@MARK@-DONE~",! }
"#;

/// ObjectScript for one walk over `classes`, on one line. It prints `~IAD-STALE~ <super> <by>` for
/// each superclass that is not library, exists on the server and is not up to date, then
/// `~IAD-DONE~`, or `~IAD-ERROR~ <text>` when it could not go on. A class with no stored
/// definition is skipped: the compiler reports a missing class better than this can.
pub fn walk_code(classes: &[String]) -> String {
    let body = WALK
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        " {}",
        body.replace("@LIST@", &os_str_expr(&classes.join(",")))
            .replace("@LIB@", &os_str_expr(&LIBRARY_PREFIXES.join("|")))
            .replace("@USER@", &os_str_expr(&USER_PREFIXES_IN_LIBRARY.join("|")))
            .replace("@MARK@", MARK)
    )
}

/// Read a walk's reply. A finding wins over a walk that broke after it: the compile is refused
/// either way, and the finding is what the agent can act on. A superclass that is library, or that
/// is itself compiled by this call (`targets`), is not a finding.
pub fn parse_walk(output: &str, targets: &[String]) -> Walk {
    const STALE: &str = "~IAD-STALE~ ";
    const ERROR: &str = "~IAD-ERROR~";
    const DONE: &str = "~IAD-DONE~";
    let mut seen: HashSet<String> = HashSet::new();
    let mut found = Vec::new();
    for line in output.lines() {
        let Some(at) = line.find(STALE) else { continue };
        let mut parts = line[at + STALE.len()..].split_whitespace();
        let (Some(sup), Some(by)) = (parts.next(), parts.next()) else {
            continue;
        };
        if is_library_class(sup) || targets.iter().any(|t| t == sup) {
            continue;
        }
        if seen.insert(sup.to_string()) {
            found.push(Stale {
                superclass: sup.to_string(),
                needed_by: by.to_string(),
            });
        }
    }
    if !found.is_empty() {
        return Walk::Stale(found);
    }
    if let Some(at) = output.find(ERROR) {
        let text = output[at + ERROR.len()..].lines().next().unwrap_or("");
        return Walk::Failed(text.trim().to_string());
    }
    if output.contains(DONE) {
        return Walk::Clear;
    }
    let head: String = output.trim().chars().take(160).collect();
    Walk::Failed(format!("IRIS gave no finished walk: {head}"))
}

/// The refusal for superclasses IRIS would compile with the target.
pub fn refusal_for(stale: &[Stale], namespace: &str) -> serde_json::Value {
    let names: Vec<&str> = stale.iter().map(|s| s.superclass.as_str()).collect();
    let described = stale
        .iter()
        .map(|s| format!("'{}' (superclass of '{}')", s.superclass, s.needed_by))
        .collect::<Vec<_>>()
        .join(", ");
    serde_json::json!({
        "success": false,
        "error_code": ERR_SUPERCLASS_STALE,
        "namespace": namespace,
        "superclasses": names,
        "message": format!(
            "IRIS would also compile {described} in {namespace}, because it is out of date. \
             What it runs while it compiles has not been checked, so the compile was refused. \
             Nothing was compiled."
        ),
        "remediation": format!(
            "Compile {} first with iris_compile (it is checked like any other class), then compile \
             the original target again.",
            names.join(", ")
        ),
    })
}

fn unchecked(reason: &str) -> serde_json::Value {
    serde_json::json!({
        "success": false,
        "error_code": ERR_SUPERCLASS_UNCHECKED,
        "reason": reason,
        "message": format!(
            "Could not check the superclasses of the classes to compile ({reason}), so it is not \
             known whether IRIS would compile an out-of-date one with them. Nothing was compiled."
        ),
        "remediation":
            "Fix the connection or the permissions on the class dictionary and compile again.",
    })
}

fn settle(walks: Vec<Walk>, namespace: &str) -> Outcome {
    let mut stale: Vec<Stale> = Vec::new();
    let mut failure: Option<String> = None;
    for w in walks {
        match w {
            Walk::Clear => {}
            Walk::Stale(list) => {
                for s in list {
                    if !stale.iter().any(|x| x.superclass == s.superclass) {
                        stale.push(s);
                    }
                }
            }
            Walk::Failed(reason) => failure = failure.or(Some(reason)),
        }
    }
    if !stale.is_empty() {
        return Outcome::Refused(refusal_for(&stale, namespace));
    }
    match failure {
        Some(reason) => Outcome::Refused(unchecked(&reason)),
        None => Outcome::Clear,
    }
}

/// The walk over Atelier's generator path. `docs` are the documents the compile names.
pub async fn check_http(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    docs: &[String],
) -> Outcome {
    let classes = class_names(docs);
    let mut walks = Vec::new();
    for chunk in classes.chunks(WALK_CHUNK) {
        let walk = match iris
            .execute_via_generator(&walk_code(chunk), namespace, client)
            .await
        {
            Ok(out) if is_generator_error(&out) => {
                Walk::Failed(out.trim().chars().take(200).collect())
            }
            Ok(out) => parse_walk(&out, &classes),
            Err(e) => Walk::Failed(format!("the request failed: {e}")),
        };
        walks.push(walk);
    }
    settle(walks, namespace)
}

/// The walk through `iris session`. `Outcome::Transport` means the exec channel itself failed, so
/// the caller can word it as the compile would have.
pub async fn check_exec(
    iris: &IrisConnection,
    ssh_host: Option<&str>,
    namespace: &str,
    docs: &[String],
) -> Outcome {
    let classes = class_names(docs);
    let mut walks = Vec::new();
    for chunk in classes.chunks(WALK_CHUNK) {
        match stored_check::exec(iris, ssh_host, namespace, &walk_code(chunk)).await {
            Ok(out) => walks.push(parse_walk(&out, &classes)),
            Err(msg) => return Outcome::Transport(msg),
        }
    }
    settle(walks, namespace)
}
