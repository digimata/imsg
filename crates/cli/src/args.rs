use imsg_core::messages::{Direction, MessageQuery};
use imsg_core::{ContactBook, Db, Error};

use crate::dates::parse_date_arg;

/// Target selection shared by message/attachment commands.
#[derive(clap::Args, Debug)]
pub struct Selector {
    /// Contact name fragment, phone number, or email
    #[arg(long, conflicts_with = "chat")]
    pub contact: Option<String>,
    /// Chat rowid (from `imsg chats list`)
    #[arg(long)]
    pub chat: Option<i32>,
}

impl Selector {
    /// Resolve to `(label, chat_ids)`. Empty selector → all chats.
    pub fn resolve(&self, db: &Db, book: &ContactBook) -> anyhow::Result<(String, Vec<i32>)> {
        if let Some(id) = self.chat {
            let chat = imsg_core::chats::show(db, book, id)?;
            return Ok((chat.name, vec![id]));
        }
        if let Some(contact) = &self.contact {
            let resolved = imsg_core::chats::resolve_selector(db, book, contact)?;
            return Ok(resolved);
        }
        Ok((String::from("all chats"), Vec::new()))
    }

    /// Like [`resolve`](Self::resolve) but requires a target.
    pub fn resolve_required(
        &self,
        db: &Db,
        book: &ContactBook,
    ) -> anyhow::Result<(String, Vec<i32>)> {
        if self.chat.is_none() && self.contact.is_none() {
            anyhow::bail!("pass --contact <name|phone|email> or --chat <id>");
        }
        self.resolve(db, book)
    }
}

/// Time-window and limit flags shared by message/attachment commands.
#[derive(clap::Args, Debug)]
pub struct Window {
    /// Start of window: YYYY-MM-DD, YYYY-MM-DDTHH:MM, or relative (7d/24h/2w)
    #[arg(long)]
    pub since: Option<String>,
    /// End of window: same formats as --since
    #[arg(long)]
    pub until: Option<String>,
    /// Maximum number of messages returned
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
}

impl Window {
    /// Build a [`MessageQuery`] for the given chats and direction flags.
    pub fn to_query(
        &self,
        chat_ids: Vec<i32>,
        from_me: bool,
        from_them: bool,
    ) -> anyhow::Result<MessageQuery> {
        let direction = match (from_me, from_them) {
            (true, false) => Some(Direction::FromMe),
            (false, true) => Some(Direction::FromThem),
            _ => None,
        };
        Ok(MessageQuery {
            chat_ids,
            since: self.since.as_deref().map(|s| parse_date_arg(s, false)).transpose()?,
            until: self.until.as_deref().map(|s| parse_date_arg(s, true)).transpose()?,
            limit: self.limit,
            direction,
            attachments_only: false,
            text_contains: None,
        })
    }
}

/// Exit code for a failed run: 2 for ambiguous/no-match selectors, 1 otherwise.
pub fn exit_code_for(err: &anyhow::Error) -> i32 {
    match err.downcast_ref::<Error>() {
        Some(Error::AmbiguousContact { .. } | Error::NoMatch(_) | Error::NoChat(_)) => 2,
        _ => 1,
    }
}
