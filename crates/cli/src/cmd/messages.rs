use clap::Subcommand;
use imsg_core::{ContactBook, Db};

use crate::args::{Selector, Window};
use crate::render;

#[derive(Subcommand, Debug)]
pub enum MessagesCmd {
    /// Read a conversation window as a transcript
    List {
        #[command(flatten)]
        selector: Selector,
        #[command(flatten)]
        window: Window,
        /// Only messages I sent
        #[arg(long, conflicts_with = "from_them")]
        from_me: bool,
        /// Only messages they sent
        #[arg(long)]
        from_them: bool,
        /// Only messages carrying attachments
        #[arg(long)]
        attachments_only: bool,
        /// Only inbound messages not yet read
        #[arg(long, conflicts_with = "from_me")]
        unread: bool,
        #[arg(long)]
        json: bool,
    },
    /// Search message text (case-insensitive, includes decoded bodies)
    Search {
        query: String,
        #[command(flatten)]
        selector: Selector,
        #[command(flatten)]
        window: Window,
        #[arg(long)]
        json: bool,
    },
}

/// Dispatch `imsg messages ...`.
pub fn run(cmd: &MessagesCmd, db: &Db, book: &ContactBook) -> anyhow::Result<()> {
    match cmd {
        MessagesCmd::List {
            selector,
            window,
            from_me,
            from_them,
            attachments_only,
            unread,
            json,
        } => {
            let (label, chat_ids) = if *unread {
                // Unread triage sweeps all chats by default.
                selector.resolve(db, book)?
            } else {
                selector.resolve_required(db, book)?
            };
            let mut q = window.to_query(chat_ids, *from_me, *from_them)?;
            q.attachments_only = *attachments_only;
            q.unread_only = *unread;
            let msgs = imsg_core::messages::fetch(db, book, &q)?;
            if *json {
                render::json(&msgs)?;
            } else {
                eprintln!("# {label} — {} messages", msgs.len());
                render::transcript(&msgs);
            }
        }
        MessagesCmd::Search {
            query,
            selector,
            window,
            json,
        } => {
            let (label, chat_ids) = selector.resolve(db, book)?;
            let mut q = window.to_query(chat_ids, false, false)?;
            q.text_contains = Some(query.clone());
            let msgs = imsg_core::messages::fetch(db, book, &q)?;
            if *json {
                render::json(&msgs)?;
            } else {
                eprintln!("# \"{query}\" in {label} — {} matches", msgs.len());
                render::transcript(&msgs);
            }
        }
    }
    Ok(())
}
