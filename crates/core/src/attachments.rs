use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Local};
use imessage_database::{
    tables::{attachment::Attachment, messages::Message},
    util::platform::Platform,
};
use serde::Serialize;

use crate::contacts::ContactBook;
use crate::db::Db;
use crate::error::Result;
use crate::messages::{MessageQuery, Sender, message_date, sender_of};

/// Metadata for one attachment, with its on-disk path resolved.
#[derive(Debug, Serialize)]
pub struct AttachmentInfo {
    pub filename: Option<String>,
    pub mime: Option<String>,
    pub size: u64,
    pub path: Option<PathBuf>,
    pub message_id: i32,
    #[serde(serialize_with = "crate::messages::ser_date")]
    pub date: DateTime<Local>,
    pub sender: Sender,
}

/// Attachments belonging to one message row.
pub(crate) fn attachments_for(
    db: &Db,
    m: &Message,
    book: &ContactBook,
    handles: &HashMap<i32, String>,
) -> Result<Vec<AttachmentInfo>> {
    let rows = Attachment::from_message(db.conn(), m)?;
    let sender = sender_of(book, handles, m);
    let date = message_date(db, m);
    Ok(rows
        .into_iter()
        .map(|a| {
            let path = a
                .resolved_attachment_path(&Platform::macOS, db.path(), None)
                .map(PathBuf::from);
            AttachmentInfo {
                filename: a.filename().map(String::from),
                mime: a.mime_type.clone(),
                size: u64::try_from(a.total_bytes).unwrap_or(0),
                path,
                message_id: m.rowid,
                date,
                sender: sender.clone(),
            }
        })
        .collect())
}

/// List attachments across every message matching `q`, chronological order.
pub fn list(db: &Db, book: &ContactBook, q: &MessageQuery) -> Result<Vec<AttachmentInfo>> {
    let query = MessageQuery {
        chat_ids: q.chat_ids.clone(),
        since: q.since,
        until: q.until,
        limit: q.limit,
        direction: q.direction,
        attachments_only: true,
        unread_only: q.unread_only,
        text_contains: None,
    };
    let msgs = crate::messages::fetch(db, book, &query)?;
    Ok(msgs.into_iter().flat_map(|m| m.attachments).collect())
}
