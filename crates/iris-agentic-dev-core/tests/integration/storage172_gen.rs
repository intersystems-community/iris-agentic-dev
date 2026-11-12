//! The spec 172 storage-map generator, from `tests/unit/storage172_gen/`, for the live compile
//! oracle (`test_storage_map_generated_172_live`).

#![allow(dead_code)]

#[path = "../unit/storage172_gen/common.rs"]
mod common;
pub use common::*;

#[path = "../unit/storage172_gen/class.rs"]
pub mod class;
#[path = "../unit/storage172_gen/pairwise.rs"]
pub mod pairwise;
