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
//! A field is its 1-based number in the master draft (`Field1` is 1). A position is 1-based among
//! the candidate's subscripts. A `null` below is JSON null. `transform` is one of `exact`,
//! `sqlupper`, `sqlstring`, `string`, `alphaup`, `upper`, `mvr`, `truncate`; `length` is set for
//! `truncate` and for the length variants of `sqlupper`, `sqlstring` and `string`; `piece` is 1 or 2
//! for the name-piece forms and null otherwise.
//!
//! ```text
//! {"t":"sample","nodes":40,"distinct_first":38,"usable":40,"stopped":"none"}
//! {"t":"id","form":"subscript","positions":[2],"resolved":40,"total":40}
//! {"t":"id","form":"value","positions":[],"resolved":0,"total":40}
//! {"t":"pos","position":1,"total":40,"distinct":38,"scores":[
//!    {"field":1,"transform":"sqlupper","length":null,"piece":null,"matches":40}]}
//! {"t":"null","position":1,"field":2,"kind":"c0","count":3}
//! {"t":"data","field":2,"element":1,"matches":40,"total":40}
//! {"t":"missing","share":0.14,"rule":"null","field":2,"literal":null}
//! {"t":"bitmap","chunk_position":2,"ids_checked":1000,"ids_found":1000}
//! {"t":"error","text":"<IRIS error text>"}
//! {"t":"done"}
//! ```
//!
//! - `sample`: nodes read, distinct first-subscript values seen, nodes whose ID resolved to a
//!   master row (`usable`), and why the walk stopped (`none`, `nodes` for the node cap, `time` for
//!   the time cap, `count_cap` when the counting pass reached its cap).
//! - `id`: one line per way the row ID was tried, best first. `resolved` counts nodes whose ID
//!   found a master row. Form `subscript` names the run of `k` positions holding the key (k = 1
//!   for a single key part); form `value` reads the ID from the node value.
//! - `pos`: for every non-ID position, up to five best `(field, transform)` scores by `matches`
//!   out of `total` usable nodes, and how many distinct values the position held (`distinct` 1
//!   means a constant). A position no pair explains still gets its best scores, so `link.rs` can
//!   report `not_explained` with the best agreement.
//! - `null`: a position whose empty-field nodes share one marker, with `kind` one of `c0`,
//!   `minus1e14`, `space`, `space_c0`, `discovered`, the empty kind (`empty_string` or `null`) in
//!   `empty`, and `count` of nodes it covers. A discovered marker adds `length`.
//! - `data`: the node value is a `$LIST`; element `element` (1-based) equals master field `field`
//!   on `matches` of `total` nodes.
//! - `missing`: the share of sampled master rows the index lacks, and the rule that explains every
//!   missing row and no present row: `null`, `empty_string`, `fixed` (with `field`, and `literal`
//!   only under `with_values`) or `none`.
//! - `bitmap`: a node value that is a bit string; `chunk_position` is the last subscript; the IDs
//!   decoded from up to 1,000 set bits, and how many of them found a master row.
//! - `error`: any IRIS error. A run that ends without a `done` line is a failed run.
