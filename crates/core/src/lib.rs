//! Core do RomTranslate Studio.
//!
//! Nao depende de Tauri. Tudo aqui e testavel via `cargo test` e reutilizavel
//! por CLI/GUI. Sprint 0-1: deteccao de plataforma, hashing e projeto local.

pub mod adapter;
pub mod adapters;
pub mod detect;
pub mod error;
pub mod hash;
pub mod project;
pub mod synth;
pub mod types;

pub use error::{CoreError, Result};
