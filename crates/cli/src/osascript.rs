//! Thin wrapper over `osascript` → Messages.app for sending.
//!
//! Scripts take their inputs as `argv` — never interpolated into the script
//! source — so message text can't break out of the AppleScript string.
//! An osascript exit code of 0 does NOT mean the message was sent (bare
//! `participant` sends no-op silently); callers must verify against chat.db.

use std::process::Command;

use anyhow::{Context, bail};

const FIND_IMESSAGE_ACCOUNT: &str = r#"on run argv
tell application "Messages"
    repeat with a in accounts
        try
            if enabled of a and (service type of a as text) is "iMessage" then return id of a
        end try
    end repeat
end tell
error "no enabled iMessage account is signed in" number 9001
end run"#;

const SEND_TO_CHAT: &str = r#"on run argv
tell application "Messages" to send (item 2 of argv) to chat id (item 1 of argv)
end run"#;

const SEND_TO_PARTICIPANT: &str = r#"on run argv
tell application "Messages" to send (item 3 of argv) to participant (item 2 of argv) of account id (item 1 of argv)
end run"#;

/// Id of the enabled iMessage account, discovered via AppleScript.
///
/// Property reads throw on some Messages accounts, so the script probes each
/// account inside a `try` block.
pub fn imessage_account_id() -> anyhow::Result<String> {
    let out = run(FIND_IMESSAGE_ACCOUNT, &[])?;
    Ok(out.trim().to_string())
}

/// Send `text` to an existing chat by its chat.db guid.
pub fn send_to_chat(guid: &str, text: &str) -> anyhow::Result<()> {
    run(SEND_TO_CHAT, &[guid, text]).map(drop)
}

/// Send `text` to a raw handle, qualified with the iMessage account id.
/// Creates the 1:1 chat when none exists.
pub fn send_to_participant(account_id: &str, handle: &str, text: &str) -> anyhow::Result<()> {
    run(SEND_TO_PARTICIPANT, &[account_id, handle, text]).map(drop)
}

fn run(script: &str, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .args(args)
        .output()
        .context("failed to run osascript")?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("-1743") || stderr.contains("Not authorized") {
        bail!(
            "Messages automation not authorized\n\
             hint: System Settings → Privacy & Security → Automation → allow \
             your terminal to control Messages"
        );
    }
    bail!("osascript failed: {}", stderr.trim());
}
