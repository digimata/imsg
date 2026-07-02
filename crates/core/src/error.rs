use std::path::PathBuf;

use imessage_database::error::table::TableError;

/// Failure modes for imsg-core operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "cannot open messages db at {path}: {source}\n\
         hint: grant Full Disk Access to your terminal (System Settings → Privacy & Security)"
    )]
    DbOpen { path: PathBuf, source: TableError },
    #[error("query failed: {0}")]
    Query(#[from] TableError),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("ambiguous contact '{query}': matches {}", candidates.join(", "))]
    AmbiguousContact {
        query: String,
        candidates: Vec<String>,
    },
    #[error("no contact or chat matches '{0}'")]
    NoMatch(String),
    #[error("no chat with id {0}")]
    NoChat(i32),
}

pub type Result<T> = std::result::Result<T, Error>;
