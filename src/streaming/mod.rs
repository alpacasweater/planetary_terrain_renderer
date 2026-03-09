//! Shared contracts for streamed terrain cache metadata and remote tile providers.
//!
//! This module is intentionally provider-agnostic. It exists so local cache resolution,
//! procedural tile validity, and future remote backends can agree on the same tile and
//! metadata contract before any HTTP-specific logic is introduced.

pub mod cache_manifest;
pub mod cache_paths;
pub mod source_contract;
pub mod tile_source;

pub use self::{cache_manifest::*, source_contract::*, tile_source::*};
