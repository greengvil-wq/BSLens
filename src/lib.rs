//! R01 scaffold. Production opening is intentionally unavailable until full validation.
use std::{fmt, path::Path};

#[derive(Debug)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self(format!("SQLite: {e}"))
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self(format!("JSON: {e}"))
    }
}
pub type Result<T> = std::result::Result<T, Error>;

/// Cannot be constructed in R01. No incomplete validator can yield this capability.
pub struct ValidatedSnapshot {
    _private: (),
}
impl ValidatedSnapshot {
    pub fn open(_path: &Path, _snapshot_id: &str) -> Result<Self> {
        Err(Error(
            "production validation not implemented; R01 is synthetic-only".into(),
        ))
    }
}

#[cfg(feature = "synthetic-dev")]
pub mod synthetic;

#[cfg(feature = "experimental-xml")]
pub mod xml221;

#[cfg(feature = "experimental-git")]
pub mod git_manifest;
#[cfg(feature = "experimental-tree")]
pub mod structure;

#[cfg(feature = "experimental-pipeline")]
pub mod pipeline;

#[cfg(feature = "experimental-lexical")]
pub mod lexical;

#[cfg(feature = "experimental-stdio")]
pub mod mcp_transport;

#[cfg(feature = "experimental-router")]
pub mod mcp_router;

#[cfg(feature = "experimental-cache")]
pub mod cache;
