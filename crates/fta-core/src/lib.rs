//! Shared engine behavior stays independent of Tauri so a headless server runs the same protocol.
pub mod config;
pub mod engine;
pub mod identity;
pub mod ipc;
pub mod model;
pub mod network;
pub mod source;
pub mod storage;

use serde::{Deserialize, Serialize};

/// Stable error codes let CLI and desktop render failures without parsing diagnostic text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    pub code: String,
    pub message: String,
}
impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        let code = match error.kind() {
            std::io::ErrorKind::PermissionDenied => "PERMISSION_DENIED",
            std::io::ErrorKind::NotFound => "NOT_FOUND",
            _ => "IO_ERROR",
        };
        Self::new(code, error.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::new("INVALID_REQUEST", error.to_string())
    }
}
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::new("DATABASE_ERROR", error.to_string())
    }
}
pub type Result<T> = std::result::Result<T, Error>;
