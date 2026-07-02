use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use imessage_database::{
    tables::table::get_connection,
    util::{dates::get_offset, dirs::default_db_path},
};
use rusqlite::Connection;

use crate::error::{Error, Result};

/// Read-only handle to the iMessage database.
///
/// Wraps the SQLite connection (opened read-only by `imessage-database`)
/// together with the Apple-epoch offset needed to interpret raw timestamps.
pub struct Db {
    conn: Connection,
    offset: i64,
    path: PathBuf,
}

impl Db {
    /// Open the Messages database read-only.
    ///
    /// `path` overrides the default `~/Library/Messages/chat.db`.
    pub fn open(path: Option<&Path>) -> Result<Db> {
        let path = path.map_or_else(default_db_path, Path::to_path_buf);
        let conn = get_connection(&path).map_err(|source| Error::DbOpen {
            path: path.clone(),
            source,
        })?;
        Ok(Db {
            conn,
            offset: get_offset(),
            path,
        })
    }

    /// The underlying SQLite connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Seconds between the Unix epoch and the Apple epoch (2001-01-01).
    pub fn offset(&self) -> i64 {
        self.offset
    }

    /// Path the database was opened from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Convert a local datetime to a raw Apple-epoch nanosecond timestamp.
    pub fn to_apple_ns(&self, dt: &DateTime<Local>) -> i64 {
        (dt.timestamp() - self.offset).saturating_mul(1_000_000_000)
    }
}
