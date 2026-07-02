use std::collections::HashMap;

use chrono::{DateTime, Local, TimeZone};
use serde::Serialize;

use crate::contacts::{ContactBook, normalize_handle};
use crate::db::Db;
use crate::error::{Error, Result};

/// One member of a chat: raw handle plus resolved contact name, if known.
#[derive(Debug, Clone, Serialize)]
pub struct Participant {
    pub handle: String,
    pub name: Option<String>,
}

/// Summary row for a chat, as shown by `imsg chats list`.
#[derive(Debug, Serialize)]
pub struct ChatSummary {
    pub id: i32,
    pub name: String,
    pub identifier: String,
    pub participants: Vec<Participant>,
    pub message_count: i64,
    /// Inbound messages not yet read (tapbacks excluded). Messages predating
    /// read receipts can sit unread forever, so treat as an upper bound.
    pub unread_count: i64,
    #[serde(serialize_with = "crate::messages::ser_opt_date")]
    pub last_message_at: Option<DateTime<Local>>,
    pub service: String,
    pub is_group: bool,
}

/// List chats ordered by most recent activity.
pub fn list(db: &Db, book: &ContactBook, limit: usize) -> Result<Vec<ChatSummary>> {
    let participants = participants_by_chat(db, book)?;
    let mut stmt = db.conn().prepare(
        "SELECT c.ROWID, c.chat_identifier, c.service_name, c.display_name,
                COUNT(j.message_id), MAX(m.date),
                COUNT(CASE WHEN m.is_read = 0 AND m.is_from_me = 0
                           AND COALESCE(m.associated_message_type, 0)
                               NOT BETWEEN 2000 AND 3999
                      THEN 1 END)
         FROM chat c
         LEFT JOIN chat_message_join j ON j.chat_id = c.ROWID
         LEFT JOIN message m ON m.ROWID = j.message_id
         GROUP BY c.ROWID
         ORDER BY MAX(m.date) DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit as i64], |row| {
        Ok((
            row.get::<_, i32>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, i64>(6)?,
        ))
    })?;
    let mut chats = Vec::new();
    for row in rows {
        let (id, identifier, service, display_name, count, last_ns, unread) = row?;
        chats.push(summarize(
            db,
            book,
            &participants,
            id,
            identifier,
            service,
            display_name,
            count,
            unread,
            last_ns,
        ));
    }
    Ok(chats)
}

/// Show a single chat by rowid.
pub fn show(db: &Db, book: &ContactBook, id: i32) -> Result<ChatSummary> {
    let participants = participants_by_chat(db, book)?;
    let mut stmt = db.conn().prepare(
        "SELECT c.chat_identifier, c.service_name, c.display_name,
                COUNT(j.message_id), MAX(m.date),
                COUNT(CASE WHEN m.is_read = 0 AND m.is_from_me = 0
                           AND COALESCE(m.associated_message_type, 0)
                               NOT BETWEEN 2000 AND 3999
                      THEN 1 END)
         FROM chat c
         LEFT JOIN chat_message_join j ON j.chat_id = c.ROWID
         LEFT JOIN message m ON m.ROWID = j.message_id
         WHERE c.ROWID = ?1
         GROUP BY c.ROWID",
    )?;
    let row = stmt
        .query_row([id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|_| Error::NoChat(id))?;
    let (identifier, service, display_name, count, last_ns, unread) = row;
    Ok(summarize(
        db,
        book,
        &participants,
        id,
        identifier,
        service,
        display_name,
        count,
        unread,
        last_ns,
    ))
}

/// Resolve a `--contact` query to a display label and the chat ids to read.
///
/// Resolution order: AddressBook contacts (fuzzy), then raw chat.db handles.
/// More than one distinct match is an error carrying the candidates so the
/// caller can retry precisely.
pub fn resolve_selector(db: &Db, book: &ContactBook, query: &str) -> Result<(String, Vec<i32>)> {
    let matches = book.resolve(query);
    if matches.len() > 1 {
        return Err(Error::AmbiguousContact {
            query: query.to_string(),
            candidates: matches.into_iter().map(|m| m.name).collect(),
        });
    }
    let handles = handle_rows(db)?;
    if let Some(found) = matches.into_iter().next() {
        let rowids: Vec<i32> = handles
            .iter()
            .filter(|(_, id)| {
                normalize_handle(id).is_some_and(|key| found.keys.contains(&key))
            })
            .map(|(rowid, _)| *rowid)
            .collect();
        if rowids.is_empty() {
            return Err(Error::NoMatch(query.to_string()));
        }
        let chats = chats_for_handle_rowids(db, &rowids)?;
        return Ok((found.name, chats));
    }
    // Fall back to matching raw handles in chat.db (contact-less numbers).
    let Some(key) = normalize_handle(query) else {
        return Err(Error::NoMatch(query.to_string()));
    };
    let matched: Vec<&(i32, String)> = handles
        .iter()
        .filter(|(_, id)| normalize_handle(id).is_some_and(|k| k.contains(&key)))
        .collect();
    let mut distinct: Vec<String> = matched.iter().map(|(_, id)| id.clone()).collect();
    distinct.sort();
    distinct.dedup();
    match distinct.len() {
        0 => Err(Error::NoMatch(query.to_string())),
        1 => {
            let rowids: Vec<i32> = matched.iter().map(|(rowid, _)| *rowid).collect();
            let chats = chats_for_handle_rowids(db, &rowids)?;
            Ok((distinct.remove(0), chats))
        }
        _ => Err(Error::AmbiguousContact {
            query: query.to_string(),
            candidates: distinct,
        }),
    }
}

/// How a send should address its recipient.
#[derive(Debug, Serialize)]
pub struct SendTarget {
    /// Display label (contact name, chat name, or raw handle).
    pub label: String,
    pub service: String,
    #[serde(flatten)]
    pub kind: SendTargetKind,
}

/// Addressing mode for a send.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SendTargetKind {
    /// An existing chat, targeted by AppleScript `chat id` (works for groups).
    Chat { rowid: i32, guid: String },
    /// A raw handle with no existing 1:1 chat; targeted by AppleScript
    /// `participant` qualified with the iMessage account (creates the chat).
    Participant { handle: String },
}

/// Build a send target for an explicit chat rowid.
pub fn send_target_for_chat(db: &Db, book: &ContactBook, id: i32) -> Result<SendTarget> {
    let summary = show(db, book, id)?;
    let guid: String = db
        .conn()
        .prepare("SELECT guid FROM chat WHERE ROWID = ?1")?
        .query_row([id], |row| row.get(0))
        .map_err(|_| Error::NoChat(id))?;
    Ok(SendTarget {
        label: summary.name,
        service: summary.service,
        kind: SendTargetKind::Chat { rowid: id, guid },
    })
}

/// Resolve a `--to` query (contact name, phone, or email) to a send target.
///
/// Resolution order mirrors [`resolve_selector`]: AddressBook contact first,
/// then raw handles in chat.db. Prefers the recipient's most recently active
/// 1:1 chat (guid targeting); falls back to a `participant` send with their
/// busiest handle, or the query verbatim when it looks like a phone/email
/// never messaged before. Group chats are never selected implicitly — use an
/// explicit `--chat` id for those.
pub fn send_target_for_contact(db: &Db, book: &ContactBook, query: &str) -> Result<SendTarget> {
    let matches = book.resolve(query);
    if matches.len() > 1 {
        return Err(Error::AmbiguousContact {
            query: query.to_string(),
            candidates: matches.into_iter().map(|m| m.name).collect(),
        });
    }
    let handles = handle_rows(db)?;
    if let Some(found) = matches.into_iter().next() {
        let rowids: Vec<i32> = handles
            .iter()
            .filter(|(_, id)| normalize_handle(id).is_some_and(|key| found.keys.contains(&key)))
            .map(|(rowid, _)| *rowid)
            .collect();
        if let Some(target) = target_for_handle_rowids(db, &rowids, &found.name)? {
            return Ok(target);
        }
        // Known contact but no message history: only a raw handle is sendable.
        if looks_like_handle(query) {
            return Ok(participant_target(found.name, query));
        }
        return Err(Error::NoMatch(query.to_string()));
    }
    // Not in the AddressBook: match raw handles by exact normalized key.
    if !looks_like_handle(query) {
        return Err(Error::NoMatch(query.to_string()));
    }
    let Some(key) = normalize_handle(query) else {
        return Err(Error::NoMatch(query.to_string()));
    };
    let rowids: Vec<i32> = handles
        .iter()
        .filter(|(_, id)| normalize_handle(id).is_some_and(|k| k == key))
        .map(|(rowid, _)| *rowid)
        .collect();
    if let Some(target) = target_for_handle_rowids(db, &rowids, query)? {
        return Ok(target);
    }
    Ok(participant_target(query.to_string(), query))
}

/// Best target for a set of handle rowids: their most recently active 1:1
/// chat, else a participant send with the busiest handle. `None` when the
/// rowid set is empty.
fn target_for_handle_rowids(
    db: &Db,
    rowids: &[i32],
    label: &str,
) -> Result<Option<SendTarget>> {
    if rowids.is_empty() {
        return Ok(None);
    }
    let placeholders = vec!["?"; rowids.len()].join(",");
    // style = 45 is a 1:1 chat (43 = group).
    let sql = format!(
        "SELECT c.ROWID, c.guid, c.service_name FROM chat c
         JOIN chat_handle_join j ON j.chat_id = c.ROWID
         WHERE c.style = 45 AND j.handle_id IN ({placeholders})
         ORDER BY (SELECT MAX(m.date) FROM chat_message_join cj
                   JOIN message m ON m.ROWID = cj.message_id
                   WHERE cj.chat_id = c.ROWID) DESC
         LIMIT 1"
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let params = rusqlite::params_from_iter(rowids.iter());
    let direct = stmt
        .query_row(params, |row| {
            Ok((
                row.get::<_, i32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    if let Some((rowid, guid, service)) = direct {
        return Ok(Some(SendTarget {
            label: label.to_string(),
            service: service.unwrap_or_default(),
            kind: SendTargetKind::Chat { rowid, guid },
        }));
    }
    // No 1:1 chat (group-only history): send to their busiest handle.
    let sql = format!(
        "SELECT h.id FROM handle h
         LEFT JOIN message m ON m.handle_id = h.ROWID
         WHERE h.ROWID IN ({placeholders})
         GROUP BY h.ROWID ORDER BY COUNT(m.ROWID) DESC LIMIT 1"
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let params = rusqlite::params_from_iter(rowids.iter());
    let handle: String = stmt.query_row(params, |row| row.get(0))?;
    Ok(Some(participant_target(label.to_string(), &handle)))
}

fn participant_target(label: String, handle: &str) -> SendTarget {
    SendTarget {
        label,
        service: String::from("iMessage"),
        kind: SendTargetKind::Participant {
            handle: handle.trim().to_string(),
        },
    }
}

/// Heuristic used by send resolution: does the query look like a phone/email
/// rather than a name fragment?
fn looks_like_handle(query: &str) -> bool {
    query.contains('@') || query.chars().filter(char::is_ascii_digit).count() >= 7
}

/// A message-bearing handle with its resolved name and message volume.
#[derive(Debug, Serialize)]
pub struct HandleCount {
    pub handle: String,
    pub name: Option<String>,
    pub messages: i64,
}

/// Handles that carry messages, merged by normalized key (e.g. `+1415...`
/// vs `415...`), ordered by message volume descending.
pub fn handle_message_counts(db: &Db, book: &ContactBook) -> Result<Vec<HandleCount>> {
    let mut stmt = db.conn().prepare(
        "SELECT h.id, COUNT(m.ROWID) FROM handle h
         LEFT JOIN message m ON m.handle_id = h.ROWID
         GROUP BY h.id",
    )?;
    let raw = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut merged: HashMap<String, HandleCount> = HashMap::new();
    for (handle, count) in raw {
        let key = normalize_handle(&handle).unwrap_or_else(|| handle.clone());
        let entry = merged.entry(key).or_insert_with(|| HandleCount {
            name: book.name_for(&handle).map(String::from),
            handle,
            messages: 0,
        });
        entry.messages += count;
    }
    let mut rows: Vec<HandleCount> = merged.into_values().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.messages));
    Ok(rows)
}

/// All `(rowid, id)` pairs from the handle table.
pub fn handle_rows(db: &Db) -> Result<Vec<(i32, String)>> {
    let mut stmt = db.conn().prepare("SELECT ROWID, id FROM handle")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn chats_for_handle_rowids(db: &Db, rowids: &[i32]) -> Result<Vec<i32>> {
    assert!(!rowids.is_empty());
    let placeholders = vec!["?"; rowids.len()].join(",");
    let sql = format!(
        "SELECT DISTINCT chat_id FROM chat_handle_join WHERE handle_id IN ({placeholders})"
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let params = rusqlite::params_from_iter(rowids.iter());
    let rows = stmt.query_map(params, |row| row.get::<_, i32>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn participants_by_chat(
    db: &Db,
    book: &ContactBook,
) -> Result<HashMap<i32, Vec<Participant>>> {
    let mut stmt = db.conn().prepare(
        "SELECT j.chat_id, h.id FROM chat_handle_join j
         JOIN handle h ON h.ROWID = j.handle_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i32>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map: HashMap<i32, Vec<Participant>> = HashMap::new();
    for row in rows {
        let (chat_id, handle) = row?;
        let name = book.name_for(&handle).map(String::from);
        map.entry(chat_id).or_default().push(Participant { handle, name });
    }
    Ok(map)
}

#[expect(clippy::too_many_arguments, reason = "internal row-assembly helper")]
fn summarize(
    db: &Db,
    book: &ContactBook,
    participants: &HashMap<i32, Vec<Participant>>,
    id: i32,
    identifier: String,
    service: Option<String>,
    display_name: Option<String>,
    message_count: i64,
    unread_count: i64,
    last_ns: Option<i64>,
) -> ChatSummary {
    let members = participants.get(&id).cloned().unwrap_or_default();
    let is_group = members.len() > 1;
    let name = display_name
        .filter(|n| !n.is_empty())
        .or_else(|| book.name_for(&identifier).map(String::from))
        .or_else(|| members.first().and_then(|p| p.name.clone()).filter(|_| !is_group))
        .unwrap_or_else(|| identifier.clone());
    let last_message_at = last_ns.and_then(|ns| apple_ns_to_local(db, ns));
    ChatSummary {
        id,
        name,
        identifier,
        participants: members,
        message_count,
        unread_count,
        last_message_at,
        service: service.unwrap_or_default(),
        is_group,
    }
}

/// Convert a raw Apple-epoch nanosecond timestamp to local time.
pub fn apple_ns_to_local(db: &Db, ns: i64) -> Option<DateTime<Local>> {
    let secs = ns / 1_000_000_000 + db.offset();
    Local.timestamp_opt(secs, 0).single()
}
