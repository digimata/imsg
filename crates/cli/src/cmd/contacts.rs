use clap::Subcommand;
use imsg_core::{ContactBook, Db};
use serde::Serialize;

use crate::render;

#[derive(Subcommand, Debug)]
pub enum ContactsCmd {
    /// List message-bearing handles with resolved names and counts
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show which handles and chats a query maps to
    Resolve {
        query: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Serialize)]
struct Resolution {
    label: String,
    chat_ids: Vec<i32>,
}

/// Dispatch `imsg contacts ...`.
pub fn run(cmd: &ContactsCmd, db: &Db, book: &ContactBook) -> anyhow::Result<()> {
    match cmd {
        ContactsCmd::List { json } => {
            let rows = imsg_core::chats::handle_message_counts(db, book)?;
            if *json {
                render::json(&rows)?;
            } else {
                println!("{:>7}  {:<24} HANDLE", "MSGS", "NAME");
                for r in &rows {
                    println!(
                        "{:>7}  {:<24} {}",
                        r.messages,
                        r.name.as_deref().unwrap_or("-"),
                        r.handle
                    );
                }
            }
        }
        ContactsCmd::Resolve { query, json } => {
            let (label, chat_ids) = imsg_core::chats::resolve_selector(db, book, query)?;
            if *json {
                render::json(&Resolution { label, chat_ids })?;
            } else {
                println!("{label}");
                println!("chats: {chat_ids:?}");
            }
        }
    }
    Ok(())
}
