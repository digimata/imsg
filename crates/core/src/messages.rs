use std::collections::HashMap;

use chrono::{DateTime, Local};
use imessage_database::{
    message_types::variants::{TapbackAction, Variant},
    tables::{
        messages::{Message, models::GroupAction},
        table::Table,
    },
};
use serde::{Serialize, Serializer};

use crate::attachments::{AttachmentInfo, attachments_for};
use crate::chats::apple_ns_to_local;
use crate::contacts::ContactBook;
use crate::db::Db;
use crate::error::Result;

/// Column list matching `imessage_database::tables::messages::Message::from_row`.
/// Mirrors the crate's private `COLS` constant plus the computed tail columns.
const SELECT_HEAD: &str = "SELECT
    m.rowid, m.guid, m.text, m.service, m.handle_id, m.destination_caller_id,
    m.subject, m.date, m.date_read, m.date_delivered, m.is_from_me, m.is_read,
    m.item_type, m.other_handle, m.share_status, m.share_direction, m.group_title,
    m.group_action_type, m.associated_message_guid, m.associated_message_type,
    m.balloon_bundle_id, m.expressive_send_style_id, m.thread_originator_guid,
    m.thread_originator_part, m.date_edited, m.associated_message_emoji,
    c.chat_id,
    (SELECT COUNT(*) FROM message_attachment_join a WHERE m.ROWID = a.message_id) as num_attachments,
    NULL as deleted_from,
    0 as num_replies
FROM message as m
LEFT JOIN chat_message_join as c ON m.ROWID = c.message_id";

/// SQL predicate that matches tapback rows (add and remove actions).
const TAPBACK_RANGE: &str = "m.associated_message_type BETWEEN 2000 AND 3999";

/// Message direction filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    FromMe,
    FromThem,
}

/// Filters for a message fetch. Empty `chat_ids` means all chats.
#[derive(Debug, Default)]
pub struct MessageQuery {
    pub chat_ids: Vec<i32>,
    pub since: Option<DateTime<Local>>,
    pub until: Option<DateTime<Local>>,
    pub limit: usize,
    pub direction: Option<Direction>,
    pub attachments_only: bool,
    /// Case-insensitive body filter, applied after typedstream decoding.
    pub text_contains: Option<String>,
}

/// Who sent a message.
#[derive(Debug, Clone)]
pub enum Sender {
    Me,
    Them {
        handle: String,
        name: Option<String>,
    },
}

impl Sender {
    /// Short display label: "me", the contact name, or the raw handle.
    pub fn label(&self) -> &str {
        match self {
            Sender::Me => "me",
            Sender::Them { handle, name } => name.as_deref().unwrap_or(handle),
        }
    }
}

impl Serialize for Sender {
    fn serialize<S: Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = ser.serialize_struct("Sender", 3)?;
        match self {
            Sender::Me => {
                s.serialize_field("handle", "me")?;
                s.serialize_field("name", &Option::<String>::None)?;
                s.serialize_field("is_me", &true)?;
            }
            Sender::Them { handle, name } => {
                s.serialize_field("handle", handle)?;
                s.serialize_field("name", name)?;
                s.serialize_field("is_me", &false)?;
            }
        }
        s.end()
    }
}

/// High-level classification of a message row after tapback folding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MsgKind {
    Text,
    Announcement,
}

/// A tapback reaction folded onto its target message.
#[derive(Debug, Serialize)]
pub struct Tapback {
    pub kind: String,
    pub by: String,
}

/// A fully hydrated message: decoded text, resolved sender, folded tapbacks.
#[derive(Debug, Serialize)]
pub struct Msg {
    pub id: i32,
    pub chat_id: Option<i32>,
    #[serde(serialize_with = "ser_date")]
    pub date: DateTime<Local>,
    pub sender: Sender,
    pub text: Option<String>,
    pub service: String,
    pub reply_to: Option<i32>,
    pub edited: bool,
    pub kind: MsgKind,
    pub tapbacks: Vec<Tapback>,
    pub attachments: Vec<AttachmentInfo>,
}

pub(crate) fn ser_date<S: Serializer>(
    date: &DateTime<Local>,
    ser: S,
) -> std::result::Result<S::Ok, S::Error> {
    ser.serialize_str(&date.to_rfc3339())
}

pub(crate) fn ser_opt_date<S: Serializer>(
    date: &Option<DateTime<Local>>,
    ser: S,
) -> std::result::Result<S::Ok, S::Error> {
    match date {
        Some(d) => ser.serialize_str(&d.to_rfc3339()),
        None => ser.serialize_none(),
    }
}

/// Fetch messages matching `q`, newest-first internally, returned in
/// chronological order with text decoded and tapbacks folded in.
pub fn fetch(db: &Db, book: &ContactBook, q: &MessageQuery) -> Result<Vec<Msg>> {
    let handles: HashMap<i32, String> = crate::chats::handle_rows(db)?.into_iter().collect();
    let mut clauses = vec![format!("NOT ({TAPBACK_RANGE})")];
    push_common_clauses(db, q, &mut clauses);
    if q.attachments_only {
        clauses.push(
            "(SELECT COUNT(*) FROM message_attachment_join a WHERE m.ROWID = a.message_id) > 0"
                .to_string(),
        );
    }
    let sql = format!(
        "{SELECT_HEAD}\nWHERE {}\nORDER BY m.date DESC",
        clauses.join(" AND ")
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = Message::rows(&mut stmt, [])?;

    // Stream newest-first, decode, filter, stop at limit.
    let pattern = q.text_contains.as_ref().map(|p| p.to_lowercase());
    let mut selected: Vec<Message> = Vec::new();
    for row in rows {
        let mut m = row?;
        hydrate_text(db, &mut m);
        if let Some(pat) = &pattern {
            let hit = m
                .text
                .as_ref()
                .is_some_and(|t| t.to_lowercase().contains(pat));
            if !hit {
                continue;
            }
        }
        selected.push(m);
        if q.limit > 0 && selected.len() >= q.limit {
            break;
        }
    }
    selected.reverse();

    let mut msgs: Vec<Msg> = selected
        .iter()
        .map(|m| to_msg(db, book, &handles, m))
        .collect::<Result<_>>()?;

    // Map thread originator GUIDs to rowids within the window.
    let guid_to_id: HashMap<&str, i32> = selected
        .iter()
        .map(|m| (m.guid.as_str(), m.rowid))
        .collect();
    for (msg, raw) in msgs.iter_mut().zip(&selected) {
        msg.reply_to = raw
            .thread_originator_guid
            .as_deref()
            .and_then(|g| guid_to_id.get(g).copied())
            .filter(|id| *id != msg.id);
    }

    if !q.chat_ids.is_empty() {
        fold_tapbacks(db, book, &handles, q, &selected, &mut msgs)?;
    }
    Ok(msgs)
}

fn push_common_clauses(db: &Db, q: &MessageQuery, clauses: &mut Vec<String>) {
    if !q.chat_ids.is_empty() {
        let ids = q
            .chat_ids
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        clauses.push(format!("c.chat_id IN ({ids})"));
    }
    if let Some(since) = &q.since {
        clauses.push(format!("m.date >= {}", db.to_apple_ns(since)));
    }
    if let Some(until) = &q.until {
        clauses.push(format!("m.date <= {}", db.to_apple_ns(until)));
    }
    match q.direction {
        Some(Direction::FromMe) => clauses.push("m.is_from_me = 1".to_string()),
        Some(Direction::FromThem) => clauses.push("m.is_from_me = 0".to_string()),
        None => {}
    }
}

/// Decode `attributedBody` into `text` when the plain column is empty.
/// Failures degrade to `None` — the renderer shows a placeholder.
fn hydrate_text(db: &Db, m: &mut Message) {
    if m.text.is_none()
        && let Ok(body) = m.parse_body(db.conn())
    {
        m.apply_body(body);
    }
}

fn to_msg(
    db: &Db,
    book: &ContactBook,
    handles: &HashMap<i32, String>,
    m: &Message,
) -> Result<Msg> {
    let sender = sender_of(book, handles, m);
    let kind = if m.is_announcement() {
        MsgKind::Announcement
    } else {
        MsgKind::Text
    };
    let text = if kind == MsgKind::Announcement {
        Some(announcement_text(m))
    } else {
        m.text.as_deref().map(clean_text).filter(|t| !t.is_empty())
    };
    let attachments = if m.num_attachments > 0 {
        attachments_for(db, m, book, handles)?
    } else {
        Vec::new()
    };
    Ok(Msg {
        id: m.rowid,
        chat_id: m.chat_id,
        date: message_date(db, m),
        sender,
        text,
        service: m.service.clone().unwrap_or_default(),
        reply_to: None,
        edited: m.is_edited(),
        kind,
        tapbacks: Vec::new(),
        attachments,
    })
}

pub(crate) fn sender_of(
    book: &ContactBook,
    handles: &HashMap<i32, String>,
    m: &Message,
) -> Sender {
    if m.is_from_me() {
        return Sender::Me;
    }
    let handle = m
        .handle_id
        .and_then(|id| handles.get(&id).cloned())
        .unwrap_or_else(|| "unknown".to_string());
    let name = book.name_for(&handle).map(String::from);
    Sender::Them { handle, name }
}

pub(crate) fn message_date(db: &Db, m: &Message) -> DateTime<Local> {
    apple_ns_to_local(db, m.date).unwrap_or_default()
}

/// Strip attachment-placeholder characters (U+FFFC) left by typedstream
/// decoding; attachments render separately.
fn clean_text(raw: &str) -> String {
    raw.replace('\u{FFFC}', "").trim().to_string()
}

fn announcement_text(m: &Message) -> String {
    match m.group_action() {
        Some(GroupAction::NameChange(name)) => format!("<renamed the chat to \"{name}\">"),
        Some(GroupAction::ParticipantAdded(_)) => "<added a participant>".to_string(),
        Some(GroupAction::ParticipantRemoved(_)) => "<removed a participant>".to_string(),
        Some(GroupAction::ParticipantLeft) => "<left the chat>".to_string(),
        Some(GroupAction::GroupIconChanged) => "<changed the group photo>".to_string(),
        Some(GroupAction::GroupIconRemoved) => "<removed the group photo>".to_string(),
        Some(GroupAction::ChatBackgroundChanged) => "<changed the chat background>".to_string(),
        Some(GroupAction::ChatBackgroundRemoved) => "<removed the chat background>".to_string(),
        Some(GroupAction::PhoneNumberChanged(_)) => "<changed their phone number>".to_string(),
        None => "<group event>".to_string(),
    }
}

/// Fetch tapback rows for the queried chats and fold them onto targets.
///
/// Tapbacks are matched by the target message GUID; a Removed action cancels
/// the most recent matching Added from the same sender.
fn fold_tapbacks(
    db: &Db,
    book: &ContactBook,
    handles: &HashMap<i32, String>,
    q: &MessageQuery,
    selected: &[Message],
    msgs: &mut [Msg],
) -> Result<()> {
    let index_by_guid: HashMap<&str, usize> = selected
        .iter()
        .enumerate()
        .map(|(i, m)| (m.guid.as_str(), i))
        .collect();
    let ids = q
        .chat_ids
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "{SELECT_HEAD}\nWHERE {TAPBACK_RANGE} AND c.chat_id IN ({ids})\nORDER BY m.date ASC"
    );
    let mut stmt = db.conn().prepare(&sql)?;
    for row in Message::rows(&mut stmt, [])? {
        let tb = row?;
        let Some((_, guid)) = tb.clean_associated_guid() else {
            continue;
        };
        let Some(&i) = index_by_guid.get(guid) else {
            continue;
        };
        let Variant::Tapback(_, action, kind) = tb.variant() else {
            continue;
        };
        let by = sender_of(book, handles, &tb).label().to_string();
        let kind = kind.to_string();
        let list = &mut msgs[i].tapbacks;
        match action {
            TapbackAction::Added => list.push(Tapback { kind, by }),
            TapbackAction::Removed => {
                if let Some(pos) = list.iter().rposition(|t| t.kind == kind && t.by == by) {
                    list.remove(pos);
                }
            }
        }
    }
    Ok(())
}
