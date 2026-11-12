//! The spec 172 storage-map generator (T102–T104), shared by the unit tests and the live compile
//! oracle (`tests/integration/storage172_gen.rs` includes the same files). Test support only.

#![allow(dead_code)]

mod common;
pub use common::*;

pub mod class;
pub mod pairwise;
