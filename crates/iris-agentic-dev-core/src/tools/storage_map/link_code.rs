//! The ObjectScript the linker runs on the server (spec 173), and the one contract between that
//! code and `link.rs`: the lines it writes.
//!
//! The code runs through `execute_via_generator_prefixed(.., "IrisDevSm")` as the service account,
//! once per candidate. It reads the candidate and the master, scores each position against each
//! master field under each transform of the closed set (calling `%SYSTEM.SQL.Functions`, never a
//! collation of its own), and writes only counts, positions, field numbers and transform names.
//! It never writes a field value or a subscript value, with one exception: a discovered null marker
//! is described by its length, and a fixed-value rule's literal appears only when the caller passed
//! `include_values` under the data policy (`with_values`).
//!
//! # Output: one JSON object per line
//!
//! A property name with an underscore (`distinct_first`, `chunk_position`, `ids_checked`,
//! `ids_found`) must be quoted in the generated code (`o."distinct_first"=..`): bare, the `_` is the
//! concatenation operator and the line is a `<SYNTAX>` error (measured).
//!
//! A field is its 1-based number in the master draft (`Field1` is 1). A position is 1-based among
//! the candidate's subscripts. A `null` below is JSON null. `transform` is one of `exact`,
//! `sqlupper`, `sqlstring`, `string`, `alphaup`, `upper`, `mvr`, `truncate`; `length` is set for
//! `truncate` and for the length variants of `sqlupper`, `sqlstring` and `string`; `piece` is 1 or 2
//! for the name-piece forms and null otherwise.
//!
//! ```text
//! {"t":"sample","nodes":40,"distinct_first":38,"usable":40,"stopped":"none","depth":2}
//! {"t":"id","form":"subscript","positions":[2],"resolved":40,"total":40}
//! {"t":"id","form":"value","positions":[],"resolved":0,"total":40}
//! {"t":"pos","position":1,"total":40,"distinct":38,"scores":[
//!    {"field":1,"transform":"sqlupper","length":null,"piece":null,"matches":40}]}
//! {"t":"null","position":1,"field":2,"kind":"c0","empty":"empty_string","count":3}
//! {"t":"data","field":2,"element":1,"matches":40,"total":40}
//! {"t":"missing","share":0.14,"rule":"null","field":2,"literal":null}
//! {"t":"bitmap","chunk_position":2,"ids_checked":1000,"ids_found":1000}
//! {"t":"error","text":"<IRIS error text>"}
//! {"t":"done"}
//! ```
//!
//! - `sample`: nodes read, distinct first-subscript values seen, nodes whose ID resolved to a
//!   master row (`usable`), and why the walk stopped (`none`, `nodes` for the node cap, `time` for
//!   the time cap, `count_cap` when the counting pass reached its cap), and `depth`, how many
//!   subscripts the index nodes read have. At most six non-ID positions get a `pos` line; the rest
//!   are named as not examined.
//! - `id`: one line per way the row ID was tried, best first. `resolved` counts nodes whose ID
//!   found a master row. Form `subscript` names the run of `k` positions holding the key (k = 1
//!   for a single key part); form `value` reads the ID from the node value. A way that reads one
//!   value from every node (three or more nodes) is not written: a constant (a chunk number, a
//!   branch name) finds a master row by coincidence and is not a row ID. Ways are ranked by rows
//!   found, then by distinct IDs (a word ID that happens to be a row number finds fewer distinct
//!   rows than the row ID does), then by the later position.
//! - `pos`: for every non-ID position, up to five best `(field, transform)` scores by `matches`
//!   out of `total` usable nodes, and how many distinct values the position held (`distinct` 1
//!   means a constant). A position no pair explains still gets its best scores, so `link.rs` can
//!   report `not_explained` with the best agreement.
//! - `null`: a position whose empty-field nodes share one marker, with `kind` one of `c0`,
//!   `minus1e14`, `space`, `space_c0`, `discovered`, the empty kind (`empty_string` or `null`) in
//!   `empty`, and `count` of nodes it covers. A discovered marker adds `length`. A discovered marker adds `literal`
//!   only under `with_values`.
//! - `data`: the node value is a `$LIST`; element `element` (1-based) equals master field `field`
//!   on `matches` of `total` nodes.
//! - `missing`: the share of sampled master rows the index lacks, and the rule that explains every
//!   missing row and no present row: `null`, `empty_string`, `fixed` (with `field`, and `literal`
//!   only under `with_values`) or `none`.
//! - `bitmap`: a node value that is a bit string; `chunk_position` is the last subscript; the IDs
//!   decoded from up to 1,000 set bits, and how many of them found a master row.
//! - `error`: any IRIS error. A run that ends without a `done` line is a failed run.

use super::master::MasterField;
use crate::objectscript::{os_str_expr, validate_global_name};

/// How far one `propose` goes on one candidate (spec FR-004a, SC-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// Nodes read from the candidate (default 200, at most 1,000).
    pub node_cap: u32,
    /// First-subscript values the counting pass may walk: 10 times the node cap.
    pub count_cap: u32,
    /// Seconds for the whole run.
    pub time_cap_secs: u32,
}

impl Caps {
    /// The caps for a node cap: the counting pass gets ten times as many values.
    pub fn for_node_cap(node_cap: u32) -> Self {
        Self {
            node_cap,
            count_cap: node_cap.saturating_mul(10),
            time_cap_secs: 5,
        }
    }
}

/// What the linker needs to know about one candidate and its master.
#[derive(Debug, Clone)]
pub struct LinkRequest<'a> {
    /// The master global, with its `^`.
    pub master: &'a str,
    /// The candidate global, with its `^`.
    pub candidate: &'a str,
    /// Literal first subscripts of the candidate to step into (a candidate found inside the
    /// master global): `["idx"]` makes the candidate `^Master("idx")`.
    pub sub_prefix: &'a [String],
    /// Parts of the master's row ID (1 for a single key).
    pub key_parts: u32,
    /// Fields of the master draft, by number (`Field1` is entry 0).
    pub fields: &'a [FieldAccess],
    pub caps: Caps,
    /// Whether a fixed-value rule's literal may be returned (`include_values` under the policy).
    pub with_values: bool,
}

/// How to read one master field from a master row's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldAccess {
    /// A named sub-node under the row (`^M(id,"EV")`), if the field lives there.
    pub node: Option<String>,
    /// The delimiter's character code; `None` means the value is a `$LIST`.
    pub delimiter: Option<u32>,
    /// The 1-based piece or `$LIST` element.
    pub piece: u32,
}

/// The linker's table entry for a master field, or why the linker cannot read it. A delimiter is
/// one character, quoted (`"^"`) or a `$c(n)` call with a constant; nothing else is evaluated, so
/// an expression in a map never reaches the generated code. `$c(0)` is refused, since delimiter 0
/// means a `$LIST` in the table.
pub fn field_access(f: &MasterField) -> Result<FieldAccess, String> {
    let name = &f.name;
    let delimiter = f
        .delimiter
        .as_deref()
        .map(str::trim)
        .map(|text| {
            delimiter_code(text).map_err(|why| format!("field {name}: the delimiter {text}: {why}"))
        })
        .transpose()?;
    let piece = f
        .piece
        .as_deref()
        .ok_or_else(|| format!("field {name} has no piece or element number"))?
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|p| *p >= 1)
        .ok_or_else(|| format!("field {name}: the piece {:?} is not 1 or more", f.piece))?;
    Ok(FieldAccess {
        node: f.node.as_deref().map(node_subscript),
        delimiter,
        piece,
    })
}

/// The subscript a field's `node` names: the map spells it as a literal (`"sub"`, a doubled quote
/// is a quote) or a number (`2`), and the code subscripts with the text without its quotes. Left
/// quoted, the code looked for a node named `"sub"` with the quotes in it, so every such field
/// read as empty (measured on a three-row global whose `"sub"` nodes hold 99934.1).
fn node_subscript(node: &str) -> String {
    let t = node.trim();
    match t.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        Some(inner) => inner.replace("\"\"", "\""),
        None => t.to_string(),
    }
}

/// The character code of a delimiter written as `"x"` (a doubled quote is a quote) or `$c(n)`.
fn delimiter_code(text: &str) -> Result<u32, String> {
    if let Some(inner) = text.strip_prefix('"') {
        let body = inner
            .strip_suffix('"')
            .ok_or("it does not end at its closing quote")?;
        // A lone quote inside would have been doubled.
        if body.replace("\"\"", "").contains('"') {
            return Err("it has text after the closing quote".into());
        }
        let chars: Vec<char> = body.replace("\"\"", "\"").chars().collect();
        return match chars.as_slice() {
            [c] if *c != '\0' => Ok(u32::from(*c)),
            _ => Err("it is not one character".into()),
        };
    }
    let lower = text.to_ascii_lowercase();
    let args = lower
        .strip_prefix("$char(")
        .or_else(|| lower.strip_prefix("$c("))
        .and_then(|r| r.strip_suffix(')'))
        .ok_or("it is neither a quoted character nor $c(n)")?;
    match args.trim().parse::<u32>() {
        Ok(n) if (1..=0x10FFFF).contains(&n) && char::from_u32(n).is_some() => Ok(n),
        _ => Err("it is not a character code of 1 or more".into()),
    }
}

/// The `%SYSTEM.SQL.Functions` methods the transforms call, by name, in the order of the
/// `transform` field of the output (`exact` calls none).
pub const FUNCTION_METHODS: [&str; 7] = [
    "SQLUPPER",
    "SQLSTRING",
    "STRING",
    "ALPHAUP",
    "UPPER",
    "MVR",
    "TRUNCATE",
];

/// The linker for one candidate, or the reason it cannot be built. Names reach the code only
/// through `os_str_expr`, after `validate_global_name`; a refused name builds nothing.
pub fn build_code(req: &LinkRequest<'_>) -> Result<String, String> {
    let master = global_literal(req.master)?;
    let candidate = global_literal(req.candidate)?;
    if req.key_parts == 0 {
        return Err("a master has at least one key part".into());
    }
    if req.fields.is_empty() {
        return Err("a master draft has at least one field".into());
    }
    let mut code = prelude(&master, &candidate, req);
    code.push_str(&fields_table(req.fields)?);
    code.push_str(&transform_table());
    code.push_str(TRY_OPEN);
    code.push_str(SPREAD_WALK);
    code.push_str(BITMAP_DETECT);
    code.push_str(" If isbm {\n");
    code.push_str(BITMAP_ROWS);
    code.push_str(" } Else {\n");
    code.push_str(LINK_BODY);
    code.push_str(" }\n");
    code.push_str(LINK_SCORE);
    code.push_str(MISSING_BODY);
    code.push_str(BITMAP_LINE);
    code.push_str(TRY_CLOSE);
    code.push_str(EPILOGUE);
    Ok(code)
}

/// `^name` quoted as an ObjectScript string, or the reason it is not a global name.
pub(super) fn global_literal(name: &str) -> Result<String, String> {
    let bare = name.strip_prefix('^').unwrap_or(name);
    validate_global_name(bare).map_err(|_| format!("{name:?} is not a global name"))?;
    Ok(os_str_expr(&format!("^{bare}")))
}

fn prelude(master: &str, candidate: &str, req: &LinkRequest<'_>) -> String {
    let mut s = format!(
        "Set mst={master},cand={candidate},t0=$ZH\n Set nodecap={},ccap={},secs={},vals={},kp={}\n",
        req.caps.node_cap,
        req.caps.count_cap,
        req.caps.time_cap_secs,
        u8::from(req.with_values),
        req.key_parts,
    );
    for lit in req.sub_prefix {
        s.push_str(&format!(" Set cand=$Name(@cand@({}))\n", os_str_expr(lit)));
    }
    s
}

/// One `fa(n)` entry per master field: node, delimiter code (0 for a `$LIST`), piece.
pub(super) fn fields_table(fields: &[FieldAccess]) -> Result<String, String> {
    let mut s = format!(" Set nf={}\n", fields.len());
    for (i, f) in fields.iter().enumerate() {
        if f.piece == 0 {
            return Err(format!("field {} has piece 0", i + 1));
        }
        let node = f.node.as_deref().map_or("\"\"".to_string(), os_str_expr);
        s.push_str(&format!(
            " Set fa({n})=$ListBuild({node},{d},{p})\n",
            n = i + 1,
            d = f.delimiter.unwrap_or(0),
            p = f.piece
        ));
    }
    Ok(s)
}

/// The transforms, applied only through the server's own documented function class.
fn transform_table() -> String {
    let mut s = String::from(" Set fc=\"%SYSTEM.SQL.Functions\"\n Set tn(1)=\"exact\"\n");
    for (i, m) in FUNCTION_METHODS.iter().enumerate() {
        // splice-ok: m is a name from the FUNCTION_METHODS constant, never caller text
        s.push_str(&format!(
            " Set tn({n})=\"{low}\",tm({n})=\"{m}\"\n",
            n = i + 2,
            low = m.to_lowercase()
        ));
    }
    s
}

/// The spread sample (FR-004a): a counting pass over the first subscript, then every k-th value
/// with up to three nodes under it; fewer than seven values means an ordered walk to the node cap.
/// Fills `nd(1..nn)` with node references and sets `distinct`, `stopped`; the sample line is
/// written once the IDs are resolved, because it carries the count of usable nodes. No value is written.
const SPREAD_WALK: &str = " Set k=\"\",nk=0,stopped=\"none\"
 For { Set k=$Order(@cand@(k)) Quit:k=\"\"  Set nk=nk+1,keys(nk)=k
   If nk>=ccap { Set stopped=\"count_cap\" Quit }
   If ($ZH-t0)>secs { Set stopped=\"time\" Quit } }
 Set distinct=nk,nn=0,base=$QLength(cand)
 Set want=nodecap\\3,stride=$Select(nk<7:1,want<1:1,nk\\want<1:1,1:nk\\want),per=$Select(nk<7:nodecap,1:3)
 For j=1:stride:nk { Quit:nn>=nodecap  Quit:($ZH-t0)>secs
   Set key=keys(j),n=$Name(@cand@(key)),got=0
   If $Data(@n)#2 { Set nn=nn+1,nd(nn)=n,got=1 }
   For { Set n=$Query(@n) Quit:n=\"\"  Quit:$QSubscript(n,base+1)'=key  Quit:got>=per  Quit:nn>=nodecap
     Set nn=nn+1,nd(nn)=n,got=got+1 } }
 If (nn>=nodecap)&&(stopped=\"none\") { Set stopped=\"nodes\" }
 If ($ZH-t0)>secs { Set stopped=\"time\" }
";

const TRY_OPEN: &str = " Set failed=0,ol=0\n Try {\n";

/// The catch writes the error's name and location, never its data: a run that fails writes an
/// `error` line and no `done` line.
const TRY_CLOSE: &str = " For i=0:1:ol { Set o=ob(i) Write o.%ToJSON(),! }
 } Catch ex {
 Set failed=1,o={},o.t=\"error\",o.text=ex.Name_\" \"_ex.Location
 Write o.%ToJSON(),!
 }
";

const EPILOGUE: &str = " If 'failed { Set o={},o.t=\"done\" Write o.%ToJSON(),! }\n";

/// The linker proper, between the walk and the flush. Lines are buffered in `ob(0..ol)` (the
/// sample line is `ob(0)`) because the time cap can still trip while scoring, and `stopped` is
/// part of the first line. A master field's value is read into `fv(i,j)` and compared on the
/// server; no value is ever written.
const LINK_BODY: &str = r#" Set tot=0,bestd=0,bestn=0
 For i=1:1:nn { Set d=$QLength(nd(i))-base,dc(d)=$Get(dc(d))+1 }
 Set d="" For { Set d=$Order(dc(d)) Quit:d=""  If (d>0)&&(dc(d)>bestn) { Set bestn=dc(d),bestd=d } }
 If bestd>0 { For i=1:1:nn { If ($QLength(nd(i))-base)=bestd { Set tot=tot+1,ln(tot)=nd(i) For p=1:1:bestd { Set sb(tot,p)=$QSubscript(nd(i),base+p) } } } }
 Set bw=-1,bres=-1
 For st=1:1:(bestd-kp+1) { Quit:($ZH-t0)>secs
   Set res=0,dn=0 Kill dd
   For i=1:1:tot { Set mr=mst,dk="" For q=0:1:kp-1 { Set mr=$Name(@mr@(sb(i,st+q))),dk=dk_$Char(1)_$Extract(sb(i,st+q),1,80) }
     If '$Data(dd(dk)) { Set dd(dk)=1,dn=dn+1 }
     If $Data(@mr) { Set res=res+1,hit(st,i)=1 } }
   If '((tot>=3)&&(dn=1)) { Set way(st)=res,ws(st)=res*10000+dn } }
 If (kp=1)&&(bestd>0) { Set res=0,vtried=0,dn=0 Kill dd
   For i=1:1:tot { Set vv=$Get(@ln(i)) If (vv'="")&&($Length(vv)<=200) { Set vtried=1,mr=$Name(@mst@(vv))
     If '$Data(dd(vv)) { Set dd(vv)=1,dn=dn+1 }
     If $Data(@mr) { Set res=res+1,hit(0,i)=1 } } }
   If vtried&&'((tot>=3)&&(dn=1)) { Set way(0)=res,ws(0)=res*10000+dn } }
 Set st="" For { Set st=$Order(ws(st)) Quit:st=""  If ws(st)>=bres { Set bres=ws(st),bw=st } }
 Set nu=0
 If bw>=0 { For i=1:1:tot { If $Get(hit(bw,i)) { Set nu=nu+1,ok(i)=1
   If bw=0 { Set mr=$Name(@mst@($Get(@ln(i)))) } Else { Set mr=mst For q=0:1:kp-1 { Set mr=$Name(@mr@(sb(i,bw+q))) } }
   Set mrow(i)=mr } } }
 Set npos=0
 If nu>0 { For p=1:1:bestd { If (npos<6)&&((bw=0)||'((p>=bw)&&(p<(bw+kp)))) { Set npos=npos+1,pp(npos)=p } } }
 Merge w2=ws
 For { Set st="",bs=-1,br=-1 For { Set st=$Order(w2(st)) Quit:st=""  If w2(st)>=br { Set br=w2(st),bs=st } } Quit:bs<0
   Set o={},o.t="id",pa=[]
   If bs=0 { Set o.form="value" } Else { Set o.form="subscript" For q=0:1:kp-1 { Do pa.%Push(bs+q) } }
   Do o.%Set("positions",pa) Set o.resolved=way(bs),o.total=tot,ol=ol+1,ob(ol)=o
   Kill w2(bs) }
"#;

/// The scoring of the rows `LINK_BODY` (or `BITMAP_ROWS`) chose: each position against each master
/// field under each transform, the null markers, the covering data, and the sample line.
const LINK_SCORE: &str = r#" For j=1:1:nf { Set fnode=$List(fa(j),1),fd=$List(fa(j),2),fp=$List(fa(j),3)
   For i=1:1:tot { If $Get(ok(i)) { Set r=mrow(i) Set:fnode'="" r=$Name(@r@(fnode))
     Set raw=$Get(@r)
     If fd=0 { Set fv(i,j)=$Select($ListValid(raw)&&($ListLength(raw)>=fp):$ListGet(raw,fp),1:"") } Else { Set fv(i,j)=$Piece(raw,$Char(fd),fp) }
     Set em(i,j)=$Select(fv(i,j)="":1,fv(i,j)=$Char(0):2,1:0) } } }
 For a=1:1:npos { Set p=pp(a),np(p)=0,dn=0 Kill dv
   For i=1:1:tot { If $Get(ok(i)) { Set s=sb(i,p) Set:$Length(s)>np(p) np(p)=$Length(s)
     If '$Data(dv($Extract(s,1,300))) { Set dv($Extract(s,1,300))=1,dn=dn+1 } } }
   Set dist(p)=dn
   For j=1:1:nf { Set ex(p,j)=0
     For i=1:1:tot { If $Get(em(i,j)) { Set g=em(i,j),s=sb(i,p)
       If '$Data(mkv(p,j,g)) { Set mkv(p,j,g)=s,mkc(p,j,g)=1 } Else { If mkv(p,j,g)=s { Set mkc(p,j,g)=mkc(p,j,g)+1 } Else { Set mkx(p,j,g)=1 } } } }
     For g=1,2 { If $Data(mkv(p,j,g))&&'$Data(mkx(p,j,g)) { Set ex(p,j)=ex(p,j)+mkc(p,j,g) } } } }
 Set seq=0
 If npos>0 { For j=1:1:nf { For tt=1:1:8 {
   If ($ZH-t0)>secs { Set stopped="time" Quit }
   Kill cb,cp1,cp2,cl
   For i=1:1:tot { If $Get(ok(i))&&'$Get(em(i,j)) { Set v=fv(i,j)
     If tt=1 { Set b=v } ElseIf tt<8 { Set b=$ClassMethod(fc,tm(tt),v) } Else { Set b="" }
     Set b1=$Piece(b,",",1),b2=$Piece(b,",",2)
     For a=1:1:npos { Set p=pp(a),s=sb(i,p)
       If (tt<8)&&(b=s) { Set cb(p)=$Get(cb(p))+1 }
       If (tt>1)&&(tt<8) { If b1=s { Set cp1(p)=$Get(cp1(p))+1 } If b2=s { Set cp2(p)=$Get(cp2(p))+1 } }
       If (tt=2)||(tt=3)||(tt=4)||(tt=8) { If $ClassMethod(fc,tm(tt),v,np(p))=s { Set cl(p)=$Get(cl(p))+1 } } } } }
   For a=1:1:npos { Set p=pp(a),mb=$Get(cb(p)),xx=ex(p,j)
     If tt=1 { Set exm(p,j)=mb }
     If tt<8 { If mb>0 { Set seq=seq+1,rk(p,-(mb+xx),seq)=$ListBuild(j,tn(tt),"","") } }
     If (tt>1)&&(tt<8) {
       If $Get(cp1(p))>mb { Set seq=seq+1,rk(p,-(cp1(p)+xx),seq)=$ListBuild(j,tn(tt),"",1) }
       If $Get(cp2(p))>mb { Set seq=seq+1,rk(p,-(cp2(p)+xx),seq)=$ListBuild(j,tn(tt),"",2) } }
     Set bm=$Select(tt=8:$Get(exm(p,j)),1:mb)
     If $Get(cl(p))>bm { Set seq=seq+1,rk(p,-(cl(p)+xx),seq)=$ListBuild(j,tn(tt),np(p),"") } } } } }
 For a=1:1:npos { Set p=pp(a)
   Set o={},o.t="pos",o.position=p,o.total=nu,o.distinct=dist(p),sa=[],k2=0
   Set mk="" For { Set mk=$Order(rk(p,mk)) Quit:mk=""  Quit:k2>=5
     Set sq="" For { Set sq=$Order(rk(p,mk,sq)) Quit:sq=""  Quit:k2>=5
       Set e=rk(p,mk,sq),x={},x.field=+$List(e,1),x.transform=$List(e,2)
       If $List(e,3)="" { Do x.%Set("length","","null") } Else { Set x.length=+$List(e,3) }
       If $List(e,4)="" { Do x.%Set("piece","","null") } Else { Set x.piece=+$List(e,4) }
       Set x.matches=-mk Do sa.%Push(x) Set k2=k2+1 } }
   Do o.%Set("scores",sa) Set ol=ol+1,ob(ol)=o }
 For a=1:1:npos { Set p=pp(a),top=0,mk=$Order(rk(p,""))
   If mk'="" { Set sq=$Order(rk(p,mk,"")),top=+$List(rk(p,mk,sq),1) }
   If top>0 { For g=1,2 { If $Data(mkv(p,top,g))&&'$Data(mkx(p,top,g)) {
     Set m=mkv(p,top,g),o={},o.t="null",o.position=p,o.field=top,o.count=mkc(p,top,g)
     Set o.empty=$Select(g=1:"null",1:"empty_string")
     If m=$Char(0) { Set kind="c0" } ElseIf m=($Char(32)_$Char(0)) { Set kind="space_c0" } ElseIf m=" " { Set kind="space" } ElseIf m=-100000000000000 { Set kind="minus1e14" } Else { Set kind="discovered" }
     Set o.kind=kind
     If kind="discovered" { Set o.length=$Length(m) Set:vals o.literal=m }
     Set ol=ol+1,ob(ol)=o } } } }
 If nu>0 { For i=1:1:tot { If $Get(ok(i)) { Set vv=$Get(@ln(i))
   If $ListValid(vv) { Set nl=$ListLength(vv) Set:nl>10 nl=10
     For e=1:1:nl { Set ev=$ListGet(vv,e) For j=1:1:nf { If ev=fv(i,j) { Set cv(e,j)=$Get(cv(e,j))+1 } } } } } } }
 Set e="" For { Set e=$Order(cv(e)) Quit:e=""
   Set bj=0,bn=0,j="" For { Set j=$Order(cv(e,j)) Quit:j=""  If cv(e,j)>bn { Set bn=cv(e,j),bj=j } }
   If bj>0 { Set o={},o.t="data",o.field=bj,o.element=e,o.matches=bn,o.total=nu,ol=ol+1,ob(ol)=o } }
 If ($ZH-t0)>secs { Set stopped="time" }
 Set o={},o.t="sample",o.nodes=nn,o."distinct_first"=distinct,o.usable=nu,o.stopped=stopped,o.depth=bestd,ob(0)=o
"#;

// ── sparse and conditional indexes (T051, T052) ──────────────────────────────────────────────
//
// An index that lacks master rows is sparse. After the link is scored, the candidate is walked for
// the IDs it holds (up to the count cap) and the first node-cap master rows are each marked present
// or missing; when the missing rows are over 5% of them, one rule is looked for that explains every
// missing row and no present row, over the fields the link explained first: a NULL field (`null`),
// an empty-string field (`empty_string`, stored as `$c(0)`), or a field every present row shares and
// no missing row has (`fixed`). The `missing` line has the share, the rule, the field and the
// counts; a fixed rule's literal is a sampled value and is written only under `vals`.

/// The `missing` line. Runs only for a subscript-form or value-form link of a single-part row ID that
/// found rows, never for a bitmap, and writes nothing when the candidate is too large to walk or the
/// time cap tripped, since the present set would not be exact.
const MISSING_BODY: &str = r#" Set mzon=0,mzm=0,mzx=0
 If (bw>=0)&&(nu>0)&&(kp=1)&&'isbm&&(($ZH-t0)<=secs) { Set mzon=1 }
 If mzon {
   Kill mzp
   Set mzab=0,mzc=0,mzn=cand
   For { Set mzn=$Query(@mzn) Quit:mzn=""
     If (base>0)&&($Name(@mzn,base)'=cand) { Quit }
     Set mzc=mzc+1
     If (mzc>ccap)||(($ZH-t0)>secs) { Set mzab=1 Quit }
     If ($QLength(mzn)-base)=bestd { Set mzk=$Select(bw=0:$Get(@mzn),1:$QSubscript(mzn,base+bw)) Set:mzk'="" mzp(mzk)=1 } }
   If mzab { Set mzon=0 } }
 If mzon {
   Set mzi=""
   For { Set mzi=$Order(@mst@(mzi)) Quit:mzi=""  Quit:mzm>=nodecap  Quit:($ZH-t0)>secs
     Set mzm=mzm+1,mzr=$Name(@mst@(mzi)),mzq(mzm)=$Select($Data(mzp(mzi))#2:1,1:0)
     For mzj=1:1:nf { Set mzfn=$List(fa(mzj),1),mzfd=$List(fa(mzj),2),mzfp=$List(fa(mzj),3),mzrr=mzr
       Set:mzfn'="" mzrr=$Name(@mzr@(mzfn))
       Set mzw=$Get(@mzrr)
       If mzfd=0 { Set mzv(mzm,mzj)=$Select($ListValid(mzw)&&($ListLength(mzw)>=mzfp):$ListGet(mzw,mzfp),1:"") } Else { Set mzv(mzm,mzj)=$Piece(mzw,$Char(mzfd),mzfp) } }
     If 'mzq(mzm) { Set mzx=mzx+1 } }
   If (mzm=0)||(mzx=0)||(mzx=mzm)||(($ZH-t0)>secs)||((mzx/mzm)'>.05) { Set mzon=0 } }
 If mzon {
   Kill mzex
   For mza=1:1:npos { Set mzpp=pp(mza),mzmk=$Order(rk(mzpp,""))
     If mzmk'="" { Set mzsq=$Order(rk(mzpp,mzmk,"")),mzex(+$List(rk(mzpp,mzmk,mzsq),1))=1 } }
   Set mzno=0
   For mzpass=1,2 { For mzj=1:1:nf { If ($Data(mzex(mzj))#2)=(mzpass=1) { Set mzno=mzno+1,mzord(mzno)=mzj } } }
   Set mzrule="none",mzfld="",mzlit=""
   For mzr1="null","empty_string","fixed" { Quit:mzrule'="none"
     Set mzmark=$Select(mzr1="null":"",1:$Char(0))
     For mzoi=1:1:nf { Quit:mzrule'="none"
       Set mzj=mzord(mzoi),mzok=1,mzfv="",mzfs=0
       For mzi=1:1:mzm { Quit:'mzok
         Set mzy=mzv(mzi,mzj)
         If mzr1'="fixed" { If mzq(mzi) { If mzy=mzmark { Set mzok=0 } } Else { If mzy'=mzmark { Set mzok=0 } } } Else { If mzq(mzi) { If (mzy="")||(mzy=$Char(0))||($Length(mzy)>100) { Set mzok=0 } ElseIf 'mzfs { Set mzfv=mzy,mzfs=1 } ElseIf mzy'=mzfv { Set mzok=0 } } } }
       If mzok&&(mzr1="fixed") {
         If 'mzfs { Set mzok=0 } Else { For mzi=1:1:mzm { If 'mzq(mzi)&&(mzv(mzi,mzj)=mzfv) { Set mzok=0 Quit } } } }
       If mzok { Set mzrule=mzr1,mzfld=mzj,mzlit=mzfv } } }
   Set o={},o.t="missing",o.share=$Number(mzx/mzm,4),o.rule=mzrule,o.rows=mzm,o.count=mzx
   If mzfld="" { Do o.%Set("field","","null") } Else { Set o.field=mzfld }
   Do o.%Set("literal","","null")
   If mzrule="fixed" { Set:vals o.literal=mzlit }
   Set ol=ol+1,ob(ol)=o }
"#;

// ── bitmaps (T049, T050) ─────────────────────────────────────────────────────────────────────
//
// A candidate whose nodes hold bit strings is a bitmap index (fact 7): the chunk is the last
// subscript and bit `p` of chunk `c` is row ID `(c - 1) * 64000 + p - 1`. The decode feeds the same
// scoring as any other index: each decoded ID is one "row" (the node's value subscripts, the master
// row the ID names), so a position is explained by the master field it reads under a transform, and
// the `bitmap` line says how many decoded IDs found a master row. Nothing but counts is written.

/// Whether the sampled nodes hold bit strings: `$BitCount` raises `<INVALID BIT STRING>` on anything
/// else (measured: a string, a number, a `$LIST` and `$c(0)` all do), so nine in ten valid is a
/// bitmap. Sets `isbm`.
const BITMAP_DETECT: &str = r#" Set nbit=0,isbm=0
 For i=1:1:nn { Set bv=$Get(@nd(i)) If bv'="" { Try { Set bc=$BitCount(bv),nbit=nbit+1 } Catch bx { } } }
 If (nn>0)&&((nbit*10)>=(nn*9)) { Set isbm=1 }
"#;

/// The rows of a bitmap candidate, in place of `LINK_BODY`'s search for the ID: the most common node
/// depth is the depth of the value nodes and its last subscript is the chunk; each node's bits are
/// decoded (at most 40 IDs a node, 1,000 in all) into rows `1..tot` with `sb(row, position)` the
/// node's subscripts before the chunk, `mrow(row)` the master row the ID names and `ok(row)` set when
/// that row exists. A composite row ID cannot be decoded (`kp>1`): no row, and the link is not linked.
const BITMAP_ROWS: &str = r#" Set bw=-1,tot=0,nu=0,npos=0,bestd=0,bestn=0,lc=0,lf=0
 For i=1:1:nn { Set d=$QLength(nd(i))-base,dc(d)=$Get(dc(d))+1 }
 Set d="" For { Set d=$Order(dc(d)) Quit:d=""  If (d>1)&&(dc(d)>bestn) { Set bestn=dc(d),bestd=d } }
 If (bestd>1)&&(kp=1) {
   For i=1:1:nn { Quit:tot>=1000
     Quit:($ZH-t0)>secs
     If ($QLength(nd(i))-base)=bestd {
       Set bv=$Get(@nd(i)),ch=$QSubscript(nd(i),base+bestd),okb=1
       If (ch'?1.N)||(ch<1) { Set okb=0 }
       If okb { Try { Set bc=$BitCount(bv) } Catch bx { Set okb=0 } }
       If okb { Set bper=0,bp=0
         For { Set bp=$BitFind(bv,1,bp+1) Quit:'bp
           Quit:bper>=40
           Quit:tot>=1000
           Set id=(ch-1)*64000+bp-1,tot=tot+1,bper=bper+1,ln(tot)=nd(i)
           For p=1:1:bestd-1 { Set sb(tot,p)=$QSubscript(nd(i),base+p) }
           Set mr=$Name(@mst@(id)),mrow(tot)=mr
           If ch>1 { Set lc=lc+1 }
           If $Data(@mr) { Set ok(tot)=1,nu=nu+1 Set:ch>1 lf=lf+1 } } } } } }
 If nu>0 { For p=1:1:bestd-1 { Set npos=npos+1,pp(npos)=p } }
 Set nn=tot
"#;

/// The `bitmap` line, after the scoring: where the chunk is, how many IDs were decoded and found a
/// master row (`later_*` for those from chunk 2 on, the only ones that confirm the chunk size), and
/// the index name a class-compiled bitmap holds first (a name, written only when it is one).
const BITMAP_LINE: &str = r#" If isbm {
   Set o={},o.t="bitmap",o."chunk_position"=$Select(bestd>1:bestd,1:1),o."ids_checked"=tot,o."ids_found"=nu,o."later_checked"=lc,o."later_found"=lf
   Set lead=""
   If (bestd>=3)&&(nu>0)&&($Get(dist(1))=1) { For i=1:1:tot { If $Get(ok(i)) { Set lead=sb(i,1) Quit } } }
   If (lead'="")&&($Length(lead)<=64)&&(lead?1A.AN) { Set o.lead=lead }
   Set ol=ol+1,ob(ol)=o }
"#;

/// The probe of a master for the shape `^G(const, chunk) = $BIT` (spec 173 FR-018), built from a
/// validated, quoted global name. It reads the first subscripts (there must be exactly one, a number
/// or a name), then each chunk node under it (`$BitCount` raises on anything but a bit string),
/// decoding up to 1,000 IDs with `$BitFind` in the order the scratch table will return them. It
/// writes one `masterbits` line (`found`, the constant, chunk and ID counts, the sum, first and last
/// ID) and `done`: counts and positions, and the constant (a number or a name).
pub fn master_bit_probe(global: &str, caps: Caps) -> Result<String, String> {
    let g = global_literal(global)?;
    let mut code = format!(
        "Set g={g},t0=$ZH\n Set nodecap={},secs={}\n",
        caps.node_cap, caps.time_cap_secs
    );
    code.push_str(TRY_OPEN);
    code.push_str(PROBE_BODY);
    code.push_str(TRY_CLOSE);
    code.push_str(EPILOGUE);
    Ok(code)
}

const PROBE_BODY: &str = r#" Set found=0,nodes=0,ids=0,idsum=0,first=0,last=0,k1="",na=0,bad=0
 Set a="" For { Set a=$Order(@g@(a)) Quit:a=""
   Set na=na+1,k1=a
   Quit:na>1 }
 If na'=1 { Set bad=1 }
 If 'bad {
   Set c="" For { Set c=$Order(@g@(k1,c)) Quit:c=""
     Quit:nodes>=nodecap
     Quit:($ZH-t0)>secs
     Set okb=1
     If (c'?1.N)||(c<1)||($Data(@g@(k1,c))'=1) { Set okb=0 }
     If okb { Set v=@g@(k1,c) Try { Set bc=$BitCount(v) } Catch bx { Set okb=0 } }
     If 'okb { Set bad=1 Quit }
     Set nodes=nodes+1
     Set bp=0 For { Set bp=$BitFind(v,1,bp+1) Quit:'bp
       Quit:ids>=1000
       Set id=(c-1)*64000+bp-1,ids=ids+1,idsum=idsum+id
       Set:ids=1 first=id
       Set last=id }
     Quit:ids>=1000 } }
 If (k1'?1.N)&&'((k1?1A.AN)&&($Length(k1)<=64)) { Set bad=1 }
 If 'bad&&(nodes>0)&&(ids>0) { Set found=1 }
 Set o={},o.t="masterbits"
 Do o.%Set("found",found,"boolean")
 Do o.%Set("constant",k1,"string")
 Set o.chunks=nodes,o.ids=ids,o.idsum=idsum,o.first=first,o.last=last,ob(0)=o
"#;
