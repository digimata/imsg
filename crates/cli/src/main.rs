//! imsg — CLI over the local iMessage database.
//!
//! Reading is read-only against chat.db by construction; sending goes
//! through Messages.app via AppleScript and never writes the database.

mod args;
mod cmd;
mod dates;
mod osascript;
mod render;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use imsg_core::{BlockSet, ContactBook, Db};

#[derive(Parser, Debug)]
#[command(name = "imsg", version, about = "CLI over the local iMessage database (reads chat.db; sends via Messages.app)")]
struct Cli {
    /// Path to chat.db (default: ~/Library/Messages/chat.db)
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// List and inspect chats
    #[command(subcommand)]
    Chats(cmd::chats::ChatsCmd),
    /// Read and search messages
    #[command(subcommand)]
    Messages(cmd::messages::MessagesCmd),
    /// List and resolve contacts
    #[command(subcommand)]
    Contacts(cmd::contacts::ContactsCmd),
    /// List attachments
    #[command(subcommand)]
    Attachments(cmd::attachments::AttachmentsCmd),
    /// Send a message via Messages.app (confirms before sending)
    Send(cmd::send::SendCmd),
    /// Check database access, schema, and decode health
    Doctor,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("imsg: {err:#}");
            ExitCode::from(u8::try_from(args::exit_code_for(&err)).unwrap_or(1))
        }
    }
}

fn run(cli: &Cli) -> anyhow::Result<()> {
    if let Cmd::Doctor = &cli.cmd {
        return cmd::doctor::run(cli.db.as_deref());
    }
    let db = Db::open(cli.db.as_deref())?;
    let book = ContactBook::load();
    let blocks: BlockSet = imsg_core::blocklist::load_and_build(&db, &book)?;
    match &cli.cmd {
        Cmd::Chats(c) => cmd::chats::run(c, &db, &book, &blocks),
        Cmd::Messages(c) => cmd::messages::run(c, &db, &book, &blocks),
        Cmd::Contacts(c) => cmd::contacts::run(c, &db, &book, &blocks),
        Cmd::Attachments(c) => cmd::attachments::run(c, &db, &book, &blocks),
        Cmd::Send(c) => cmd::send::run(c, &db, &book, &blocks),
        Cmd::Doctor => unreachable!("handled above"),
    }
}
