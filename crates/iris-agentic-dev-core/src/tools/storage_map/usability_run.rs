//! `check`'s usability lines on live IRIS (spec 173 US7, FR-023): the scratch copy is compiled once,
//! and while it exists one `EXPLAIN` per index map and field prefix is put to the planner. The
//! planner's words are read by [`super::usability`]; nothing here decides what a plan means.

use super::model::SqlMap;
use super::tree::Node;
use super::usability::{
    explain_prefix_query, line_for, questions, Line, MapSubs, Question, MAX_QUESTIONS,
};
use super::write::{scratch_compile_queries, scratch_probe};
use crate::iris::connection::IrisConnection;
use serde_json::Value;

/// The maps of a model as [`questions`] reads them.
pub fn map_subs(maps: &[SqlMap]) -> Vec<MapSubs> {
    maps.iter()
        .map(|m| MapSubs {
            name: m.name.clone(),
            kind: m.r#type.clone(),
            subscripts: m
                .subscripts
                .iter()
                .map(|s| (s.level, s.expression.clone()))
                .collect(),
        })
        .collect()
}

/// A question the planner is put, or the reason it is not.
pub fn askable(mut qs: Vec<Question>) -> Vec<Question> {
    let mut asked = 0;
    for q in qs.iter_mut().filter(|q| q.unanswerable.is_none()) {
        // A field that is no SQL name is found before the query is built.
        if let Err(why) = explain_prefix_query("A.B", &q.fields) {
            q.unanswerable = Some(why);
        } else if asked >= MAX_QUESTIONS {
            q.unanswerable = Some(format!(
                "not asked: `check` puts at most {MAX_QUESTIONS} questions to the planner"
            ));
        } else {
            asked += 1;
        }
    }
    qs
}

/// The plan an `EXPLAIN` answer holds: every text the rows carry, in order.
fn plan_text(answer: &Value) -> Result<String, String> {
    let rows = answer["result"]["content"]
        .as_array()
        .ok_or("IRIS answered the EXPLAIN with no rows")?;
    let text: Vec<&str> = rows
        .iter()
        .filter_map(Value::as_object)
        .flat_map(|o| o.values())
        .filter_map(Value::as_str)
        .collect();
    if text.is_empty() {
        return Err("IRIS answered the EXPLAIN with no plan text".into());
    }
    Ok(text.join("\n"))
}

/// Compile `nodes` in a scratch copy of `text` (as `write::scratch_check` does) and ask the planner,
/// in that copy, about every index map of `maps`. `Ok` is the scratch class's name and the lines;
/// `Err` is the answer a failed compile gives, unchanged. A question that cannot be answered is an
/// `unknown` line, never an error.
pub async fn scratch_check_with_usability(
    iris: &IrisConnection,
    client: &reqwest::Client,
    text: &str,
    nodes: &[Node],
    ns: &str,
    class: &str,
    maps: &[SqlMap],
) -> Result<(String, Vec<Line>), Value> {
    scratch_probe(text, nodes)?;
    let qs = askable(questions(&map_subs(maps)));
    let asked: Vec<&Question> = qs.iter().filter(|q| q.unanswerable.is_none()).collect();
    let build = |scratch: &str| -> Vec<String> {
        asked
            .iter()
            // Checked before (`askable`), so the empty text is never sent; it keeps the answers
            // in step with the questions if that ever fails.
            .map(|q| explain_prefix_query(scratch, &q.fields).unwrap_or_default())
            .collect()
    };
    let untouched = format!("the real class {class} was not touched");
    let (scratch, answers) = scratch_compile_queries(
        iris,
        client,
        text,
        nodes,
        ns,
        (!asked.is_empty()).then_some(&build as &(dyn Fn(&str) -> Vec<String> + Sync)),
        &untouched,
    )
    .await?;
    let mut answers = answers.unwrap_or_default().into_iter();
    let lines = qs
        .iter()
        .map(|q| {
            let plan = if q.unanswerable.is_some() {
                Err("not asked".to_string())
            } else {
                match answers.next() {
                    Some(Ok(answer)) => plan_text(&answer),
                    Some(Err(e)) => Err(e.to_string()),
                    None => Err("the planner was not asked".to_string()),
                }
            };
            line_for(q, plan.as_deref().map_err(String::as_str), &scratch, class)
        })
        .collect();
    Ok((scratch, lines))
}
