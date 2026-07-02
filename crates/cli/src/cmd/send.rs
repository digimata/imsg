use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

use clap::Args;
use imsg_core::chats::{SendTarget, SendTargetKind};
use imsg_core::messages::{self, SentReceipt};
use imsg_core::{ContactBook, Db};
use serde::Serialize;

use crate::{osascript, render};

/// How long to poll chat.db for the sent row before declaring the send
/// unverified. Sends land within a second or two; the slack covers a busy
/// Messages.app.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(10);
const VERIFY_INTERVAL: Duration = Duration::from_millis(400);

#[derive(Args, Debug)]
pub struct SendCmd {
    /// Recipient: contact name fragment, phone number, or email
    #[arg(long, conflicts_with = "chat", required_unless_present = "chat")]
    to: Option<String>,
    /// Chat rowid (from `imsg chats list`; required for group chats)
    #[arg(long)]
    chat: Option<i32>,
    /// Message text
    text: String,
    /// Skip the confirmation prompt
    #[arg(long)]
    yes: bool,
    /// Resolve the recipient and show what would be sent, without sending
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Serialize)]
struct SendReport<'a> {
    target: &'a SendTarget,
    text: &'a str,
    sent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt: Option<&'a SentReceipt>,
}

/// Dispatch `imsg send`.
pub fn run(cmd: &SendCmd, db: &Db, book: &ContactBook) -> anyhow::Result<()> {
    let target = match (cmd.chat, &cmd.to) {
        (Some(id), _) => imsg_core::chats::send_target_for_chat(db, book, id)?,
        (None, Some(query)) => imsg_core::chats::send_target_for_contact(db, book, query)?,
        (None, None) => unreachable!("clap enforces --to or --chat"),
    };
    let dest = describe(&target);

    if cmd.dry_run {
        if cmd.json {
            return render::json(&SendReport {
                target: &target,
                text: &cmd.text,
                sent: false,
                receipt: None,
            });
        }
        println!("would send to {dest}: {}", cmd.text);
        return Ok(());
    }
    if !cmd.yes {
        confirm(&dest, &cmd.text)?;
    }

    let before = messages::max_rowid(db)?;
    let (chat_rowid, ident) = match &target.kind {
        SendTargetKind::Chat { rowid, guid } => {
            osascript::send_to_chat(guid, &cmd.text)?;
            (Some(*rowid), None)
        }
        SendTargetKind::Participant { handle } => {
            let account = osascript::imessage_account_id()?;
            osascript::send_to_participant(&account, handle, &cmd.text)?;
            (None, Some(handle.as_str()))
        }
    };

    let receipt = verify(db, before, chat_rowid, ident)?;
    if receipt.error != 0 {
        anyhow::bail!(
            "Messages reported send error {} (message rowid {}) — check Messages.app",
            receipt.error,
            receipt.rowid
        );
    }
    if cmd.json {
        return render::json(&SendReport {
            target: &target,
            text: &cmd.text,
            sent: true,
            receipt: Some(&receipt),
        });
    }
    let status = if receipt.is_sent {
        "sent"
    } else {
        "accepted (delivery pending)"
    };
    println!(
        "{status} to {dest} [{}] (message {})",
        receipt.date.format("%Y.%m.%d %H:%M:%S"),
        receipt.rowid
    );
    Ok(())
}

fn describe(target: &SendTarget) -> String {
    match &target.kind {
        SendTargetKind::Chat { rowid, .. } => {
            format!("{} (chat {rowid}, {})", target.label, target.service)
        }
        SendTargetKind::Participant { handle } if handle == &target.label => {
            format!("{handle} (new chat)")
        }
        SendTargetKind::Participant { handle } => {
            format!("{} ({handle}, new chat)", target.label)
        }
    }
}

/// Interactive y/N gate. Refuses non-interactive use without `--yes` so a
/// script can never send by accident.
fn confirm(dest: &str, text: &str) -> anyhow::Result<()> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("refusing to send without confirmation; pass --yes");
    }
    print!("send to {dest}: \"{text}\" — proceed? [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
        anyhow::bail!("aborted");
    }
    Ok(())
}

/// Poll chat.db until the outgoing row appears (the only real confirmation —
/// osascript exit codes are meaningless), then keep polling briefly for
/// `is_sent` to flip.
fn verify(
    db: &Db,
    before: i64,
    chat_rowid: Option<i32>,
    ident: Option<&str>,
) -> anyhow::Result<SentReceipt> {
    let deadline = Instant::now() + VERIFY_TIMEOUT;
    let mut receipt: Option<SentReceipt> = None;
    loop {
        if let Some(found) = messages::outgoing_after(db, before, chat_rowid, ident)? {
            let settled = found.is_sent || found.error != 0;
            receipt = Some(found);
            if settled {
                break;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(VERIFY_INTERVAL);
    }
    receipt.ok_or_else(|| {
        anyhow::anyhow!(
            "send not confirmed: no outgoing message appeared in chat.db within {}s — \
             the send may have silently failed; check Messages.app",
            VERIFY_TIMEOUT.as_secs()
        )
    })
}
