//! Core do RomTranslate Studio.
//!
//! Nao depende de Tauri. Tudo aqui e testavel via `cargo test` e reutilizavel
//! por CLI/GUI. Sprint 0-1: deteccao de plataforma, hashing e projeto local.

pub mod adapter;
pub mod adapters;
pub mod cue;
pub mod db;
pub mod detect;
pub mod error;
pub mod export;
pub mod hash;
pub mod patch;
pub mod pipeline;
pub mod project;
pub mod provider;
pub mod providers;
pub mod reinsert;
pub mod scan;
pub mod synth;
pub mod tbl;
pub mod types;
pub mod validate;

pub use error::{CoreError, Result};
