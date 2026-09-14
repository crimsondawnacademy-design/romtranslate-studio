use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("erro de IO em {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("arquivo muito grande: {size} bytes (limite atual: {limit} bytes)")]
    FileTooLarge { size: u64, limit: u64 },

    #[error("projeto: {0}")]
    Project(String),

    #[error("serializacao: {0}")]
    Serde(#[from] serde_json::Error),
}

impl CoreError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        CoreError::Io {
            path: path.into(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;
