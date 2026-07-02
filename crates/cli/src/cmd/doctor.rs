use std::path::Path;

use imsg_core::contacts::addressbook_sources;
use imsg_core::{ContactBook, Db};

/// Run environment checks: DB access, table counts, decode rate, AddressBook.
pub fn run(db_path: Option<&Path>) -> anyhow::Result<()> {
    let db = match Db::open(db_path) {
        Ok(db) => {
            println!("ok    chat.db readable at {}", db.path().display());
            db
        }
        Err(e) => {
            println!("FAIL  {e}");
            anyhow::bail!("database not accessible");
        }
    };

    let count = |sql: &str| -> anyhow::Result<i64> {
        Ok(db.conn().query_row(sql, [], |r| r.get(0))?)
    };
    let messages = count("SELECT COUNT(*) FROM message")?;
    let chats = count("SELECT COUNT(*) FROM chat")?;
    let handles = count("SELECT COUNT(*) FROM handle")?;
    println!("ok    {messages} messages, {chats} chats, {handles} handles");

    let encoded = count("SELECT COUNT(*) FROM message WHERE text IS NULL AND attributedBody IS NOT NULL")?;
    println!("ok    {encoded} messages need typedstream decoding");
    report_decode_rate(&db)?;

    let sources = addressbook_sources();
    if sources.is_empty() {
        println!("warn  no AddressBook sources found — handles will render raw");
    } else {
        let book = ContactBook::load();
        println!(
            "ok    {} AddressBook source(s), {} named contacts",
            sources.len(),
            book.all().len()
        );
    }
    Ok(())
}

/// Decode the most recent body-bearing messages and report the success rate.
fn report_decode_rate(db: &Db) -> anyhow::Result<()> {
    use imsg_core::messages::MessageQuery;
    let book = ContactBook::load();
    let q = MessageQuery {
        limit: 200,
        ..MessageQuery::default()
    };
    let msgs = imsg_core::messages::fetch(db, &book, &q)?;
    if msgs.is_empty() {
        println!("warn  no messages to sample for decode rate");
        return Ok(());
    }
    let decoded = msgs.iter().filter(|m| m.text.is_some()).count();
    let pct = 100.0 * decoded as f64 / msgs.len() as f64;
    let status = if pct >= 95.0 { "ok  " } else { "warn" };
    println!(
        "{status}  decode rate {pct:.1}% over the {} most recent messages",
        msgs.len()
    );
    Ok(())
}
