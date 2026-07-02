use chrono::{DateTime, Local};
use imsg_core::attachments::AttachmentInfo;
use imsg_core::chats::ChatSummary;
use imsg_core::messages::Msg;
use serde::Serialize;

/// Print any serializable value as pretty JSON.
pub fn json<T: Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn fmt_date(date: &DateTime<Local>) -> String {
    date.format("%Y.%m.%d %H:%M").to_string()
}

/// Human-readable byte size (KB/MB/GB, base 1024).
pub fn fmt_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Render messages as a compact transcript, tapbacks indented under targets.
pub fn transcript(msgs: &[Msg]) {
    for m in msgs {
        let when = fmt_date(&m.date);
        let who = m.sender.label();
        // Attachment-only messages have no text; the attachment lines below
        // carry the content. Truly undecoded bodies get a placeholder.
        let placeholder = if m.attachments.is_empty() {
            "<undecodable>"
        } else {
            ""
        };
        let mut line = m
            .text
            .clone()
            .unwrap_or_else(|| placeholder.to_string())
            .replace('\n', "\n    ");
        if m.edited {
            line.push_str(" (edited)");
        }
        if let Some(reply) = m.reply_to {
            line = format!("(reply to #{reply}) {line}");
        }
        println!("[{when}] {who}: {line}");
        for a in &m.attachments {
            println!(
                "    <attachment: {} ({}, {})>",
                a.filename.as_deref().unwrap_or("unnamed"),
                a.mime.as_deref().unwrap_or("unknown"),
                fmt_size(a.size)
            );
        }
        for t in &m.tapbacks {
            println!("    {} {}", t.kind, t.by);
        }
    }
}

/// Render chat summaries as an aligned, grep-friendly table.
pub fn chats_table(chats: &[ChatSummary]) {
    println!(
        "{:>5}  {:<30} {:>6}  {:<16}  PARTICIPANTS",
        "ID", "NAME", "MSGS", "LAST"
    );
    for c in chats {
        let last = c
            .last_message_at
            .map_or_else(|| "-".to_string(), |d| fmt_date(&d));
        let participants = c
            .participants
            .iter()
            .map(|p| p.name.clone().unwrap_or_else(|| p.handle.clone()))
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{:>5}  {:<30} {:>6}  {:<16}  {}",
            c.id,
            truncate(&c.name, 30),
            c.message_count,
            last,
            participants
        );
    }
}

/// Render attachment metadata as an aligned table.
pub fn attachments_table(items: &[AttachmentInfo]) {
    println!(
        "{:<16}  {:<20} {:<28} {:>9}  PATH",
        "DATE", "FROM", "FILENAME", "SIZE"
    );
    for a in items {
        println!(
            "{:<16}  {:<20} {:<28} {:>9}  {}",
            fmt_date(&a.date),
            truncate(a.sender.label(), 20),
            truncate(a.filename.as_deref().unwrap_or("unnamed"), 28),
            fmt_size(a.size),
            a.path
                .as_ref()
                .map_or_else(|| "-".to_string(), |p| p.display().to_string())
        );
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}
