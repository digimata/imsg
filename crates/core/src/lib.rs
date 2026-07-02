//! Read-only access layer over the local iMessage database.
//!
//! Wraps `imessage-database` (typedstream decoding, schema handling) with
//! contact resolution, tapback folding, and query-shaped domain types.

pub mod attachments;
pub mod chats;
pub mod contacts;
pub mod db;
pub mod error;
pub mod messages;

pub use contacts::ContactBook;
pub use db::Db;
pub use error::{Error, Result};
