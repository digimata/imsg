use std::collections::HashMap;
use std::path::PathBuf;

use imessage_database::util::dirs::home;
use rusqlite::{Connection, OpenFlags};

/// A resolved contact: display name plus the normalized handle keys it owns.
#[derive(Debug, Clone)]
pub struct ContactMatch {
    pub name: String,
    /// Normalized keys (see [`normalize_handle`]) for the contact's phones/emails.
    pub keys: Vec<String>,
}

/// In-memory index of the local AddressBook.
///
/// Maps normalized handles (phone/email) to display names and back. Loading
/// degrades to an empty book when the AddressBook is unreadable — callers
/// then see raw handles instead of names.
pub struct ContactBook {
    by_key: HashMap<String, String>,
    by_name: Vec<ContactMatch>,
}

/// Normalize a phone number or email into a comparison key.
///
/// Phones reduce to their last 10 digits (tolerant of +1/formatting);
/// emails lowercase. Returns `None` for strings with no usable content.
pub fn normalize_handle(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains('@') {
        return Some(trimmed.to_lowercase());
    }
    let digits: String = trimmed.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return Some(trimmed.to_lowercase());
    }
    let key = if digits.len() > 10 {
        digits[digits.len() - 10..].to_string()
    } else {
        digits
    };
    Some(key)
}

impl ContactBook {
    /// Scan every AddressBook source database and build the index.
    ///
    /// Failures (missing dir, unreadable db) are swallowed by design: an
    /// empty book means handles render raw but everything still works.
    pub fn load() -> ContactBook {
        let mut book = ContactBook {
            by_key: HashMap::new(),
            by_name: Vec::new(),
        };
        for db_path in addressbook_sources() {
            let Ok(conn) = Connection::open_with_flags(
                &db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ) else {
                continue;
            };
            let _ = book.ingest_source(&conn);
        }
        book
    }

    /// Look up the display name for a raw handle (phone or email).
    pub fn name_for(&self, handle: &str) -> Option<&str> {
        let key = normalize_handle(handle)?;
        self.by_key.get(&key).map(String::as_str)
    }

    /// Fuzzy-resolve a query (name fragment, phone, or email) to contacts.
    ///
    /// The caller decides the ambiguity policy; this returns every match.
    pub fn resolve(&self, query: &str) -> Vec<ContactMatch> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        // Phone/email queries match on normalized key.
        let looks_like_handle =
            q.contains('@') || q.chars().filter(char::is_ascii_digit).count() >= 7;
        if looks_like_handle
            && let Some(key) = normalize_handle(&q)
        {
            return self
                .by_name
                .iter()
                .filter(|c| c.keys.iter().any(|k| k == &key))
                .cloned()
                .collect();
        }
        // Name queries: exact match wins, then word-prefix, then substring.
        let exact: Vec<ContactMatch> = self
            .by_name
            .iter()
            .filter(|c| c.name.to_lowercase() == q)
            .cloned()
            .collect();
        if !exact.is_empty() {
            return exact;
        }
        let prefix: Vec<ContactMatch> = self
            .by_name
            .iter()
            .filter(|c| {
                c.name
                    .to_lowercase()
                    .split_whitespace()
                    .any(|word| word.starts_with(&q))
            })
            .cloned()
            .collect();
        if !prefix.is_empty() {
            return prefix;
        }
        self.by_name
            .iter()
            .filter(|c| c.name.to_lowercase().contains(&q))
            .cloned()
            .collect()
    }

    /// All known contacts, sorted by name.
    pub fn all(&self) -> &[ContactMatch] {
        &self.by_name
    }

    fn ingest_source(&mut self, conn: &Connection) -> rusqlite::Result<()> {
        let mut collected: HashMap<String, Vec<String>> = HashMap::new();
        let mut ingest = |sql: &str| -> rusqlite::Result<()> {
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map([], |row| {
                let first: Option<String> = row.get(0)?;
                let last: Option<String> = row.get(1)?;
                let org: Option<String> = row.get(2)?;
                let value: Option<String> = row.get(3)?;
                Ok((display_name(first, last, org), value))
            })?;
            for row in rows.flatten() {
                let (Some(name), Some(value)) = row else {
                    continue;
                };
                if let Some(key) = normalize_handle(&value) {
                    collected.entry(name).or_default().push(key);
                }
            }
            Ok(())
        };
        ingest(
            "SELECT r.ZFIRSTNAME, r.ZLASTNAME, r.ZORGANIZATION, p.ZFULLNUMBER
             FROM ZABCDRECORD r JOIN ZABCDPHONENUMBER p ON p.ZOWNER = r.Z_PK",
        )?;
        ingest(
            "SELECT r.ZFIRSTNAME, r.ZLASTNAME, r.ZORGANIZATION, e.ZADDRESS
             FROM ZABCDRECORD r JOIN ZABCDEMAILADDRESS e ON e.ZOWNER = r.Z_PK",
        )?;
        for (name, keys) in collected {
            for key in &keys {
                self.by_key.entry(key.clone()).or_insert_with(|| name.clone());
            }
            match self.by_name.iter_mut().find(|c| c.name == name) {
                Some(existing) => existing.keys.extend(keys),
                None => self.by_name.push(ContactMatch { name, keys }),
            }
        }
        self.by_name.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(())
    }
}

fn display_name(
    first: Option<String>,
    last: Option<String>,
    org: Option<String>,
) -> Option<String> {
    let full = [first, last]
        .into_iter()
        .flatten()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if !full.is_empty() {
        return Some(full);
    }
    org.map(|o| o.trim().to_string()).filter(|o| !o.is_empty())
}

/// Paths of every AddressBook source database on this machine.
pub fn addressbook_sources() -> Vec<PathBuf> {
    let root = PathBuf::from(home()).join("Library/Application Support/AddressBook");
    let mut paths = Vec::new();
    let direct = root.join("AddressBook-v22.abcddb");
    if direct.exists() {
        paths.push(direct);
    }
    if let Ok(entries) = std::fs::read_dir(root.join("Sources")) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("AddressBook-v22.abcddb");
            if candidate.exists() {
                paths.push(candidate);
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::normalize_handle;

    #[test]
    fn phones_normalize_to_last_ten_digits() {
        assert_eq!(
            normalize_handle("+1 (415) 555-1234"),
            Some("4155551234".into())
        );
        assert_eq!(normalize_handle("4155551234"), Some("4155551234".into()));
        assert_eq!(normalize_handle("555-1234"), Some("5551234".into()));
    }

    #[test]
    fn emails_normalize_to_lowercase() {
        assert_eq!(
            normalize_handle(" Foo@Bar.COM "),
            Some("foo@bar.com".into())
        );
    }

    #[test]
    fn empty_input_yields_none() {
        assert_eq!(normalize_handle("  "), None);
    }
}
