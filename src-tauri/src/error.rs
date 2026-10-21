use serde::{Serialize, Serializer};
use ts_rs::TS;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A program ran and exited with a non-zero status.
    #[error("{command}: exit {code}: {stderr}")]
    Command { command: String, code: String, stderr: String },
    /// A program could not be started at all (not installed, not on PATH).
    #[error("{program}: {source}")]
    Spawn { program: String, source: std::io::Error },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Pty(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What the frontend receives when a command fails.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct IpcError {
    pub kind: ErrorKind,
    pub message: String,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ErrorKind {
    Command,
    Spawn,
    Io,
    Json,
    Invalid,
    Pty,
}

impl Error {
    fn kind(&self) -> ErrorKind {
        match self {
            Error::Command { .. } => ErrorKind::Command,
            Error::Spawn { .. } => ErrorKind::Spawn,
            Error::Io(_) => ErrorKind::Io,
            Error::Json(_) => ErrorKind::Json,
            Error::Invalid(_) => ErrorKind::Invalid,
            Error::Pty(_) => ErrorKind::Pty,
        }
    }

    pub fn pty(e: impl std::fmt::Display) -> Self {
        Error::Pty(e.to_string())
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        IpcError { kind: self.kind(), message: self.to_string() }.serialize(serializer)
    }
}
