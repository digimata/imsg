use clap::Subcommand;
use imsg_core::{ContactBook, Db};

use crate::args::{Selector, Window};
use crate::render;

#[derive(Subcommand, Debug)]
pub enum AttachmentsCmd {
    /// List attachments with on-disk paths
    List {
        #[command(flatten)]
        selector: Selector,
        #[command(flatten)]
        window: Window,
        #[arg(long)]
        json: bool,
    },
}

/// Dispatch `imsg attachments ...`.
pub fn run(cmd: &AttachmentsCmd, db: &Db, book: &ContactBook) -> anyhow::Result<()> {
    match cmd {
        AttachmentsCmd::List {
            selector,
            window,
            json,
        } => {
            let (label, chat_ids) = selector.resolve_required(db, book)?;
            let q = window.to_query(chat_ids, false, false)?;
            let items = imsg_core::attachments::list(db, book, &q)?;
            if *json {
                render::json(&items)?;
            } else {
                eprintln!("# {label} — {} attachments", items.len());
                render::attachments_table(&items);
            }
        }
    }
    Ok(())
}
