---
title: "imsg v1 — read-only iMessage CLI"
date: 2026-07-01
status: done
affects: "new project — imsg workspace"
---

## Context

We want a `kdb`-style noun-verb CLI over `~/Library/Messages/chat.db` that Claude can drive: list chats, read conversations by contact, search, list attachments. Verified against the live DB (2026.07.01): 29,806 messages, 28,131 of which have `text = NULL` with the body in `attributedBody` (typedstream) — so decoding is on the critical path for ~94% of messages.

Key facts established during scoping:

- `chat.db` is readable from the terminal (Full Disk Access already granted).
- AddressBook SQLite sources exist at `~/Library/Application Support/AddressBook/Sources/*/AddressBook-v22.abcddb` for name resolution.
- Dependency: [`imessage-database` 4.2.0](https://docs.rs/imessage-database/4.2.0) (GPL-3.0, from `imessage-exporter`) solves typedstream decoding, Apple-epoch dates, tapback/edit variants. It force-pins `rusqlite = "=0.40.0"` and `chrono = "=0.4.44"`; our crates match those pins. Everything else floats at latest. Requires rustc ≥ 1.94 (toolchain updated to 1.96.1).

### `imessage-database` API surface we build on

- `tables::table::{Table, get_connection}` — `get_connection(&path)` opens read-only; `Table::rows(stmt, params)` deserializes any custom `SELECT message.*/chat.*/handle.* ...` statement into typed rows.
- `tables::messages::Message` — fields `rowid, guid, text, handle_id, date, is_from_me, associated_message_guid, ...`; methods `parse_body(&db) -> ParsedBody` (decodes `attributedBody`), `apply_body`, `date(offset)`, `is_tapback()`, `is_reply()`, `is_edited()`, `variant()`.
- `tables::chat::Chat` — `rowid, chat_identifier, service_name, display_name`, `name()`.
- `tables::handle::Handle` — `rowid, id (phone/email), person_centric_id`.
- `tables::attachment::Attachment` — filename, mime, size, on-disk path resolution.
- `util::dates::get_offset()` — Apple-epoch offset for `Message::date()`.
- `util::dirs::default_db_path()` — `~/Library/Messages/chat.db`.

## Changes

### 1. Workspace scaffold (done)

`Cargo.toml` workspace, members `crates/core` (`imsg-core`, library) and `crates/cli` (`imsg`, binary). Edition 2024, license GPL-3.0-or-later (forced by the dependency). Deps installed via `cargo add` at latest: clap 4.6 (derive), serde 1.0.228, serde_json 1.0.150, anyhow, thiserror 2; pinned: rusqlite =0.40.0, chrono =0.4.44.

### 2. `imsg-core` — library crate

```
crates/core/src/
├── lib.rs           # index: module decls + re-exports only (CC-R1)
├── error.rs         # Error enum
├── db.rs            # Db handle
├── contacts.rs      # AddressBook resolution
├── chats.rs         # chat listing + participant mapping
├── messages.rs      # message fetch, decode, tapback folding
└── attachments.rs   # attachment metadata queries
```

**`error.rs`** — one variant per failure mode, context attached (CC-R11):

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot open messages db at {path}: {source}")]
    DbOpen { path: PathBuf, source: TableError },
    #[error("query failed: {0}")]
    Query(#[from] TableError),
    #[error("ambiguous contact '{query}': matches {candidates:?}")]
    AmbiguousContact { query: String, candidates: Vec<String> },
    #[error("no contact or chat matches '{0}'")]
    NoMatch(String),
    #[error("invalid date '{0}' (expected YYYY-MM-DD, YYYY-MM-DDTHH:MM, or 7d/24h/2w)")]
    BadDate(String),
}
pub type Result<T> = std::result::Result<T, Error>;
```

**`db.rs`** — handle owning the connection and epoch offset:

```rust
pub struct Db {
    conn: Connection,     // via imessage_database get_connection (read-only)
    offset: i64,          // util::dates::get_offset(), computed once
}
impl Db {
    /// Open chat.db read-only. `path` overrides the default location.
    pub fn open(path: Option<&Path>) -> Result<Db>;
    pub fn conn(&self) -> &Connection;
    pub fn offset(&self) -> i64;
}
```

**`contacts.rs`** — loads all AddressBook sources once per invocation into an in-memory map:

```rust
pub struct ContactBook {
    by_key: HashMap<String, String>,   // normalized handle key -> display name
    names: Vec<(String, Vec<String>)>, // display name -> handles (for reverse lookup)
}
impl ContactBook {
    /// Scan AddressBook sources; degrade to an empty book if unreadable.
    pub fn load() -> ContactBook;
    /// Handle ("+14155551234" / email) -> contact name, if known.
    pub fn name_for(&self, handle: &str) -> Option<&str>;
    /// Fuzzy query (name fragment, phone, email) -> matches. Caller decides ambiguity policy.
    pub fn resolve(&self, query: &str) -> Vec<ContactMatch>;
}
pub struct ContactMatch { pub name: String, pub handles: Vec<String> }
```

Normalization: phones → last-10-digits key (E.164-tolerant); emails → lowercase. SQL against `ZABCDRECORD` joined to `ZABCDPHONENUMBER` / `ZABCDEMAILADDRESS` per source DB, read-only. AddressBook failure is non-fatal by design — raw handles still render.

**`chats.rs`**:

```rust
pub struct ChatSummary {
    pub id: i32,
    pub name: String,                  // display_name | resolved 1:1 contact | identifier
    pub identifier: String,
    pub participants: Vec<Participant>,
    pub message_count: i64,
    pub last_message_at: Option<DateTime<Local>>,
    pub service: String,
    pub is_group: bool,
}
pub struct Participant { pub handle: String, pub name: Option<String> }

pub fn list(db: &Db, book: &ContactBook, limit: usize) -> Result<Vec<ChatSummary>>;
pub fn show(db: &Db, book: &ContactBook, id: i32) -> Result<ChatSummary>;
/// All chat rowids in which any of `handles` participates (drives --contact).
pub fn ids_for_handles(db: &Db, handles: &[String]) -> Result<Vec<i32>>;
```

**`messages.rs`** — the core. Query struct in, domain messages out:

```rust
pub struct MessageQuery {
    pub chat_ids: Vec<i32>,            // empty = all chats
    pub since: Option<DateTime<Local>>,
    pub until: Option<DateTime<Local>>,
    pub limit: usize,                  // applied to non-tapback messages, newest-first then reversed
    pub direction: Option<Direction>,  // FromMe | FromThem
    pub attachments_only: bool,
    pub text_contains: Option<String>, // case-insensitive, post-decode (search)
}

pub enum Sender { Me, Them { handle: String, name: Option<String> } }

pub struct Msg {
    pub id: i32,
    pub chat_id: i32,
    pub date: DateTime<Local>,
    pub sender: Sender,
    pub text: Option<String>,          // decoded via parse_body when text is NULL
    pub service: String,
    pub reply_to: Option<i32>,
    pub edited: bool,
    pub tapbacks: Vec<Tapback>,        // folded from associated rows
    pub attachments: Vec<AttachmentInfo>,
    pub kind: MsgKind,                 // Text | Announcement | App | Unknown
}
pub struct Tapback { pub kind: String, pub by: Sender }   // "love", "like", ...

pub fn fetch(db: &Db, book: &ContactBook, q: &MessageQuery) -> Result<Vec<Msg>>;
```

Implementation notes:

- One SQL statement with `JOIN chat_message_join` + optional date/chat/direction predicates, `ORDER BY message.date DESC LIMIT ?`, deserialized via `Message::rows`; result reversed to chronological.
- Tapbacks: rows where `is_tapback()`; folded onto targets by `associated_message_guid` → GUID map. A second targeted query fetches tapbacks pointing at the selected window (so a tapback outside the LIMIT window still shows).
- Decode: `msg.parse_body(db)` + `apply_body` when `text` is `None`; decode failure degrades to `<undecodable>` rather than erroring the batch.
- Search (`text_contains`): SQL prefilter `WHERE message.text LIKE ?` **OR** `attributedBody IS NOT NULL`, then decode + case-insensitive filter in Rust. 30k rows decodes in well under a second; correctness over cleverness for v1.

**`attachments.rs`**:

```rust
pub struct AttachmentInfo {
    pub filename: Option<String>,
    pub mime: Option<String>,
    pub size: Option<u64>,
    pub path: Option<PathBuf>,         // resolved to absolute under ~/Library/Messages
    pub message_id: i32,
    pub date: DateTime<Local>,
    pub sender: Sender,
}
pub fn list(db: &Db, book: &ContactBook, q: &MessageQuery) -> Result<Vec<AttachmentInfo>>;
```

### 3. `imsg` — CLI crate

```
crates/cli/src/
├── main.rs          # clap derive tree, dispatch, exit codes
├── args.rs          # shared flag structs (Selector, Window, OutputMode)
├── resolve.rs       # --contact/--chat -> chat_ids (ambiguity → exit 2 with candidates)
├── dates.rs         # date-arg parsing (absolute + relative 7d/24h/2w)
├── render.rs        # transcript + table + JSON renderers
└── cmd/
    ├── mod.rs
    ├── chats.rs     # list, show
    ├── messages.rs  # list, search
    ├── contacts.rs  # list, resolve
    ├── attachments.rs
    └── doctor.rs
```

Shared clap structs (flattened into subcommands, single source of truth for flags):

```rust
#[derive(clap::Args)]
struct Selector {
    #[arg(long)] contact: Option<String>,
    #[arg(long)] chat: Option<i32>,
}
#[derive(clap::Args)]
struct Window {
    #[arg(long)] since: Option<String>,
    #[arg(long)] until: Option<String>,
    #[arg(long, default_value_t = 50)] limit: usize,
}
```

Rendering:

- Transcript (default for `messages`): `[YYYY.MM.DD HH:MM] sender: text`, tapbacks indented beneath their target, attachments as `<attachment: name (mime, size)>`, group chats always show sender names.
- Tables (for `chats list`, `contacts list`, `attachments list`): plain aligned columns, no box-drawing (grep-friendly).
- `--json`: `serde_json` straight off the core types (`Serialize` derives in core).

Exit codes: `0` ok, `1` error, `2` ambiguous/no-match contact (with candidate list on stderr so Claude can retry precisely).

`doctor`: DB path + readability, message/chat/handle counts, decode success rate over the 200 most recent body-bearing messages, AddressBook source count, actionable Full-Disk-Access message on permission failure.

### 4. Out of scope for v1

Sending, watch/tail, HTML/PDF export, attachment content extraction, FTS index. `imsg-core` stays cleanly separable if we ever want an MCP server on top.

## Files touched

```
┌──────────────────────────────────┬─────────────────────────────┐
│ File                             │ Action                      │
├──────────────────────────────────┼─────────────────────────────┤
│ Cargo.toml                       │ Done (workspace)            │
│ README.md                        │ Done                        │
│ docs/impl-plan.md                │ Done (this file)            │
│ crates/core/Cargo.toml           │ Done                        │
│ crates/core/src/lib.rs           │ Create (index/re-exports)   │
│ crates/core/src/error.rs         │ Create                      │
│ crates/core/src/db.rs            │ Create                      │
│ crates/core/src/contacts.rs      │ Create                      │
│ crates/core/src/chats.rs         │ Create                      │
│ crates/core/src/messages.rs      │ Create                      │
│ crates/core/src/attachments.rs   │ Create                      │
│ crates/cli/Cargo.toml            │ Done                        │
│ crates/cli/src/main.rs           │ Create (clap tree)          │
│ crates/cli/src/args.rs           │ Create                      │
│ crates/cli/src/resolve.rs        │ Create                      │
│ crates/cli/src/dates.rs          │ Create                      │
│ crates/cli/src/render.rs         │ Create                      │
│ crates/cli/src/cmd/*.rs          │ Create (5 command modules)  │
│ .gitignore                       │ Create (target/)            │
└──────────────────────────────────┴─────────────────────────────┘
```

## Verification

1. `cargo clippy --all-targets -- -D warnings` clean (CC-2.3).
2. Unit tests: date parsing (absolute/relative/invalid), phone normalization, contact fuzzy matching, tapback folding (fixture rows).
3. Live smoke against the real DB:
   - `imsg doctor` → all checks green, decode rate reported (expect >95%).
   - `imsg chats list --limit 10` → names resolved, sane last-activity ordering.
   - `imsg messages list --contact <known>` → transcript matches Messages.app for the same window (spot-check content, order, tapbacks).
   - `imsg messages search <known-word>` → finds messages whose body lives only in `attributedBody`.
   - `imsg messages list --contact <ambiguous fragment>` → exit 2 + candidates.
   - `--json` outputs parse with `jq` and round-trip cleanly.
4. Read-only assurance: run the suite while Messages.app is open; confirm `chat.db-wal` untouched by us (open with `SQLITE_OPEN_READ_ONLY`).
