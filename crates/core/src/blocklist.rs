//! Blocklist: chats and contacts imsg refuses to read or message.
//!
//! Entries live in `~/.config/imsg/blocklist`, one per line — a contact
//! name, phone number, email, or `chat:<rowid>`; `#` starts a comment.
//! Enforcement is deny-by-default in core: the resolved [`BlockSet`] is
//! consulted by every read and send path, so no CLI surface (transcripts,
//! JSON, search, unread sweeps, attachments, sends) can leak a blocked
//! conversation. Any group chat a blocked contact participates in is
//! hidden entirely.
//!
//! This is a guardrail for the sanctioned tool, not a security boundary:
//! chat.db itself remains readable by anything with Full Disk Access.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use imessage_database::util::dirs::home;

use crate::contacts::{ContactBook, normalize_handle};
use crate::db::Db;
use crate::error::Result;

/// Raw entries parsed from the blocklist file.
#[derive(Debug, Default)]
pub struct Blocklist {
    pub names: Vec<String>,
    pub handles: Vec<String>,
    pub chat_ids: Vec<i32>,
}

/// Resolved deny-set enforced by read and send paths.
#[derive(Debug, Default)]
pub struct BlockSet {
    /// Chats hidden entirely: explicitly blocked ids plus every chat a
    /// blocked handle participates in (whole-group semantics).
    pub chat_ids: HashSet<i32>,
    /// Normalized handle keys (see [`normalize_handle`]) of blocked contacts.
    pub handle_keys: HashSet<String>,
    /// `handle` table rowids for blocked contacts.
    pub handle_rowids: HashSet<i32>,
}

/// Default blocklist path: `~/.config/imsg/blocklist`.
pub fn default_path() -> PathBuf {
    PathBuf::from(home()).join(".config/imsg/blocklist")
}

impl Blocklist {
    /// Parse the blocklist file; a missing file is an empty blocklist.
    pub fn load(path: &Path) -> Blocklist {
        match std::fs::read_to_string(path) {
            Ok(text) => Blocklist::parse(&text),
            Err(_) => Blocklist::default(),
        }
    }

    /// Parse blocklist entries from text (one entry per line).
    pub fn parse(text: &str) -> Blocklist {
        let mut list = Blocklist::default();
        for line in text.lines() {
            let entry = line.split('#').next().unwrap_or("").trim();
            if entry.is_empty() {
                continue;
            }
            if let Some(id) = entry.strip_prefix("chat:") {
                if let Ok(id) = id.trim().parse::<i32>() {
                    list.chat_ids.push(id);
                }
                continue;
            }
            let looks_like_handle = entry.contains('@')
                || entry.chars().filter(char::is_ascii_digit).count() >= 7;
            if looks_like_handle {
                list.handles.push(entry.to_string());
            } else {
                list.names.push(entry.to_string());
            }
        }
        list
    }

    /// Resolve entries against the AddressBook and chat.db into the
    /// enforceable [`BlockSet`]. Ambiguous name entries block every match
    /// (privacy-first).
    pub fn build(&self, db: &Db, book: &ContactBook) -> Result<BlockSet> {
        let mut set = BlockSet {
            chat_ids: self.chat_ids.iter().copied().collect(),
            ..BlockSet::default()
        };
        for handle in &self.handles {
            if let Some(key) = normalize_handle(handle) {
                set.handle_keys.insert(key);
            }
        }
        for name in &self.names {
            for found in book.resolve(name) {
                set.handle_keys.extend(found.keys);
            }
        }
        if !set.handle_keys.is_empty() {
            for (rowid, id) in crate::chats::handle_rows(db)? {
                if normalize_handle(&id).is_some_and(|k| set.handle_keys.contains(&k)) {
                    set.handle_rowids.insert(rowid);
                }
            }
        }
        if !set.handle_rowids.is_empty() {
            let ids = set
                .handle_rowids
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT DISTINCT chat_id FROM chat_handle_join WHERE handle_id IN ({ids})"
            );
            let mut stmt = db.conn().prepare(&sql)?;
            let rows = stmt.query_map([], |row| row.get::<_, i32>(0))?;
            for row in rows {
                set.chat_ids.insert(row?);
            }
        }
        Ok(set)
    }
}

/// Load the default blocklist file and resolve it against this database.
pub fn load_and_build(db: &Db, book: &ContactBook) -> Result<BlockSet> {
    Blocklist::load(&default_path()).build(db, book)
}

impl BlockSet {
    /// True when nothing is blocked (lets query builders skip clauses).
    pub fn is_empty(&self) -> bool {
        self.chat_ids.is_empty() && self.handle_rowids.is_empty()
    }

    /// Is this chat hidden?
    pub fn blocks_chat(&self, id: i32) -> bool {
        self.chat_ids.contains(&id)
    }

    /// Does this normalized key set intersect the blocked contacts?
    pub fn blocks_any_key(&self, keys: &[String]) -> bool {
        keys.iter().any(|k| self.handle_keys.contains(k))
    }

    /// Is this raw handle blocked?
    pub fn blocks_handle(&self, handle: &str) -> bool {
        normalize_handle(handle).is_some_and(|k| self.handle_keys.contains(&k))
    }

    /// SQL predicates excluding blocked chats and handles; empty when
    /// nothing is blocked.
    pub fn sql_clauses(&self) -> Vec<String> {
        let mut clauses = Vec::new();
        if !self.chat_ids.is_empty() {
            let ids = self
                .chat_ids
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            clauses.push(format!("(c.chat_id IS NULL OR c.chat_id NOT IN ({ids}))"));
        }
        if !self.handle_rowids.is_empty() {
            let ids = self
                .handle_rowids
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            clauses.push(format!("m.handle_id NOT IN ({ids})"));
        }
        clauses
    }
}

#[cfg(test)]
mod tests {
    use super::Blocklist;

    #[test]
    fn parses_entry_kinds() {
        let list = Blocklist::parse(
            "# comment\n\nJane Doe\n+1 (415) 555-1234\nfoo@bar.com # trailing\nchat:42\n",
        );
        assert_eq!(list.names, vec!["Jane Doe"]);
        assert_eq!(list.handles, vec!["+1 (415) 555-1234", "foo@bar.com"]);
        assert_eq!(list.chat_ids, vec![42]);
    }

    #[test]
    fn missing_file_is_empty() {
        let list = Blocklist::load(std::path::Path::new("/nonexistent/blocklist"));
        assert!(list.names.is_empty() && list.handles.is_empty() && list.chat_ids.is_empty());
    }
}
