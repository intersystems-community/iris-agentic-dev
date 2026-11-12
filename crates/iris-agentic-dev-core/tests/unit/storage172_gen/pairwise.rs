//! A covering array (T103): rows of value indices such that every allowed pair of values from two
//! dimensions appears in some row. Greedy, one row at a time, each row started from an uncovered
//! pair; deterministic for a seed; never emits a forbidden pair.

use super::{Dim, Pair};
use std::collections::BTreeSet;

type Cell = (usize, usize); // (dimension, value)

fn forbidden_cells(dims: &[Dim], forbidden: &[Pair]) -> BTreeSet<(Cell, Cell)> {
    let find = |(d, v): &(String, String)| -> Option<Cell> {
        let di = dims.iter().position(|x| &x.name == d)?;
        let vi = dims[di].values.iter().position(|x| x == v)?;
        Some((di, vi))
    };
    forbidden
        .iter()
        .filter_map(|p| {
            let (a, b) = (find(&p.a)?, find(&p.b)?);
            Some(if a < b { (a, b) } else { (b, a) })
        })
        .collect()
}

fn pairs_of(row: &[usize]) -> Vec<(Cell, Cell)> {
    let mut out = Vec::new();
    for i in 0..row.len() {
        for j in i + 1..row.len() {
            out.push(((i, row[i]), (j, row[j])));
        }
    }
    out
}

pub fn pairwise(dims: &[Dim], forbidden: &[Pair], seed: u64) -> Vec<Vec<usize>> {
    let bad = forbidden_cells(dims, forbidden);
    let mut uncovered: BTreeSet<(Cell, Cell)> = BTreeSet::new();
    for i in 0..dims.len() {
        for j in i + 1..dims.len() {
            for a in 0..dims[i].values.len() {
                for b in 0..dims[j].values.len() {
                    let p = ((i, a), (j, b));
                    if !bad.contains(&p) {
                        uncovered.insert(p);
                    }
                }
            }
        }
    }
    let mut state = seed | 1;
    let mut rand = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    let mut rows = Vec::new();
    while let Some(&((i, a), (j, b))) = uncovered.iter().next() {
        let mut row: Vec<Option<usize>> = vec![None; dims.len()];
        row[i] = Some(a);
        row[j] = Some(b);
        // Fill the other dimensions in a seeded order, each with the value that covers the most
        // uncovered pairs with what is already chosen and makes no forbidden pair.
        let mut order: Vec<usize> = (0..dims.len()).filter(|d| *d != i && *d != j).collect();
        for k in (1..order.len()).rev() {
            order.swap(k, (rand() as usize) % (k + 1));
        }
        for d in order {
            let mut best: Option<(usize, usize)> = None;
            for v in 0..dims[d].values.len() {
                let conflicts = row.iter().enumerate().any(|(e, c)| {
                    c.is_some_and(|cv| {
                        let p = if e < d {
                            ((e, cv), (d, v))
                        } else {
                            ((d, v), (e, cv))
                        };
                        bad.contains(&p)
                    })
                });
                if conflicts {
                    continue;
                }
                let gain = row
                    .iter()
                    .enumerate()
                    .filter_map(|(e, c)| c.map(|cv| (e, cv)))
                    .filter(|&(e, cv)| {
                        let p = if e < d {
                            ((e, cv), (d, v))
                        } else {
                            ((d, v), (e, cv))
                        };
                        uncovered.contains(&p)
                    })
                    .count();
                if best.is_none_or(|(_, g)| gain > g) {
                    best = Some((v, gain));
                }
            }
            row[d] = Some(best.map_or(0, |(v, _)| v));
        }
        let row: Vec<usize> = row.into_iter().map(|c| c.unwrap_or(0)).collect();
        for p in pairs_of(&row) {
            uncovered.remove(&p);
        }
        rows.push(row);
    }
    rows
}
