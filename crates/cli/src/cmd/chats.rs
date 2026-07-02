use clap::Subcommand;
use imsg_core::{ContactBook, Db};

use crate::render;

#[derive(Subcommand, Debug)]
pub enum ChatsCmd {
    /// List chats by most recent activity
    List {
        #[arg(long, default_value_t = 30)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Show participants and metadata for one chat
    Show {
        id: i32,
        #[arg(long)]
        json: bool,
    },
}

/// Dispatch `imsg chats ...`.
pub fn run(cmd: &ChatsCmd, db: &Db, book: &ContactBook) -> anyhow::Result<()> {
    match cmd {
        ChatsCmd::List { limit, json } => {
            let chats = imsg_core::chats::list(db, book, *limit)?;
            if *json {
                render::json(&chats)?;
            } else {
                render::chats_table(&chats);
            }
        }
        ChatsCmd::Show { id, json } => {
            let chat = imsg_core::chats::show(db, book, *id)?;
            if *json {
                render::json(&chat)?;
            } else {
                println!("chat {} — {}", chat.id, chat.name);
                println!("identifier:  {}", chat.identifier);
                println!("service:     {}", chat.service);
                println!("messages:    {}", chat.message_count);
                println!("group:       {}", chat.is_group);
                println!("participants:");
                for p in &chat.participants {
                    match &p.name {
                        Some(name) => println!("  {name} <{}>", p.handle),
                        None => println!("  {}", p.handle),
                    }
                }
            }
        }
    }
    Ok(())
}
