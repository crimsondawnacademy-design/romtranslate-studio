//! Core do RomTranslate Studio.
//!
//! Nao depende de Tauri. Tudo aqui e testavel via `cargo test` e reutilizavel
//! por CLI/GUI. Sprint 0-1: deteccao de plataforma, hashing e projeto local.

pub mod adapter;
pub mod adapters;
pub mod audit;
pub mod cue;
pub mod db;
pub mod detect;
pub mod error;
pub mod export;
pub mod fileio;
pub mod hash;
pub mod images;
pub mod kuten;
pub mod lz77;
pub mod pairs;
pub mod patch;
pub mod pipeline;
pub mod profile;
pub mod profile_draft;
pub mod project;
pub mod provider;
pub mod providers;
pub mod reinsert;
pub mod relative;
pub mod scan;
pub mod settings;
pub mod sjis;
pub mod synth;
pub mod tbl;
pub mod types;
pub mod validate;
pub mod xray;

pub use error::{CoreError, Result};
