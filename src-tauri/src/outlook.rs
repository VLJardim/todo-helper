//! Reads new Inbox mail from every account in classic Outlook for Windows, through the running
//! Outlook app (outlook.ps1, over COM). Outlook is already signed in, so no password is stored.
use crate::db::{add_inbox_item, get_setting, set_setting};
use crate::extract::no_window;
use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use serde::Deserialize;
use std::path::Path;
use std::process::Command;
use std::time::{Duration as Wait, Instant};

const SCRIPT: &str = include_str!("outlook.ps1");
const TIME: &str = "%Y-%m-%dT%H:%M:%SZ";
/// The script exits with this when no classic Outlook is running.
const NOT_RUNNING: i32 = 3;

#[derive(Deserialize, Debug, PartialEq)]
pub struct Mail {
    /// The Message-ID, so mail read twice, or sitting in two accounts, is stored once.
    pub id: String,
    pub sender: String,
    pub subject: String,
    pub received: String,
    pub body: String,
}

#[derive(Deserialize, Debug)]
pub struct Reply {
    pub accounts: usize,
    pub mail: Vec<Mail>,
}

/// Reads mail newer than the last check, stores it, and returns how many new messages were
/// stored and from how many accounts. The first run takes the last 3 days, so old mail never
/// floods the board.
pub fn fetch_new(conn: &Connection, work_dir: &Path) -> Result<(usize, usize), String> {
    let db_err = |e: rusqlite::Error| format!("Couldn't save mail to the database: {e}");
    let saved = get_setting(conn, "outlook_since").map_err(db_err)?;
    let since = saved.unwrap_or_else(|| (Utc::now() - Duration::days(3)).format(TIME).to_string());
    let reply = parse(&run_script(work_dir, &read_from(&since))?)?;
    let mut stored = 0;
    for m in &reply.mail {
        let body: String = m.body.chars().take(8000).collect();
        stored += add_inbox_item(conn, "email", &m.id, &m.sender, &m.subject, &m.received, &body).map_err(db_err)?;
    }
    set_setting(conn, "outlook_since", &newest(&since, &reply.mail)).map_err(db_err)?;
    Ok((stored, reply.accounts))
}

/// Where the script starts reading: 2 hours before the newest mail seen, because Outlook can sync
/// a message after a newer one. Mail read twice is skipped by its Message-ID.
// ponytail: a fixed 2-hour overlap; widen it if a slow mailbox ever syncs later than that.
pub fn read_from(since: &str) -> String {
    DateTime::parse_from_rfc3339(since).map(|t| (t.to_utc() - Duration::hours(2)).format(TIME).to_string()).unwrap_or(since.to_string())
}

/// The newest received time seen so far. It never moves backwards.
pub fn newest(since: &str, mail: &[Mail]) -> String {
    mail.iter().map(|m| m.received.as_str()).chain([since]).max().unwrap_or(since).to_string()
}

pub fn parse(json: &str) -> Result<Reply, String> {
    serde_json::from_str(json.trim_start_matches('\u{feff}')).map_err(|e| format!("Outlook's reply didn't read as mail: {e}"))
}

fn run_script(work_dir: &Path, since: &str) -> Result<String, String> {
    std::fs::create_dir_all(work_dir).map_err(|e| format!("Couldn't create {}: {e}", work_dir.display()))?;
    let (script, out) = (work_dir.join("outlook.ps1"), work_dir.join("outlook-mail.json"));
    std::fs::write(&script, SCRIPT).map_err(|e| format!("Couldn't write {}: {e}", script.display()))?;
    let _ = std::fs::remove_file(&out);
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]).arg(&script).args(["-Since", since, "-Out"]).arg(&out);
    no_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start PowerShell: {e}"))?;
    // Outlook can block on a dialog; give up after a minute so the next check can try again.
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("PowerShell stopped unexpectedly: {e}"))? {
            break status;
        }
        if start.elapsed() > Wait::from_secs(60) {
            let _ = child.kill();
            return Err("Outlook didn't answer within a minute. Close any open Outlook dialog; the app tries again at the next check.".into());
        }
        std::thread::sleep(Wait::from_millis(250));
    };
    match status.code() {
        Some(0) => std::fs::read_to_string(&out).map_err(|e| format!("Couldn't read Outlook's reply: {e}")),
        Some(NOT_RUNNING) => Err("Outlook isn't running. Open classic Outlook for Windows (the one with a File menu), then press Check now.".into()),
        _ => Err(format!("Reading Outlook failed ({status}). The app tries again at the next check.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(received: &str) -> Mail {
        Mail { id: format!("<{received}@x>"), sender: "a".into(), subject: "b".into(), received: received.into(), body: "c".into() }
    }

    #[test]
    fn parses_the_scripts_reply() {
        let empty = parse("{\"accounts\":3,\"mail\":[]}").unwrap();
        assert_eq!((empty.accounts, empty.mail.len()), (3, 0));
        let one = parse("\u{feff}{\"accounts\":1,\"mail\":[{\"id\":\"<a@b>\",\"sender\":\"Lærer <l@skole.dk>\",\"subject\":\"Aflevering\",\"received\":\"2026-10-24T08:15:00Z\",\"body\":\"Husk essayet\"}]}").unwrap();
        assert_eq!(one.mail[0].sender, "Lærer <l@skole.dk>");
        assert!(parse("not json").is_err());
    }

    #[test]
    fn the_newest_received_time_only_moves_forward() {
        let since = "2026-10-24T08:00:00Z";
        assert_eq!(newest(since, &[]), since);
        assert_eq!(newest(since, &[mail("2026-10-24T07:00:00Z")]), since);
        assert_eq!(newest(since, &[mail("2026-10-24T09:30:00Z"), mail("2026-10-24T09:00:00Z")]), "2026-10-24T09:30:00Z");
        assert_eq!(read_from(since), "2026-10-24T06:00:00Z");
    }

    #[test]
    fn stored_mail_is_skipped_the_second_time() {
        let conn = crate::db::open(&crate::db::temp_path("outlook")).unwrap();
        let m = mail("2026-10-24T09:00:00Z");
        assert_eq!(add_inbox_item(&conn, "email", &m.id, &m.sender, &m.subject, &m.received, &m.body).unwrap(), 1);
        assert_eq!(add_inbox_item(&conn, "email", &m.id, &m.sender, &m.subject, &m.received, &m.body).unwrap(), 0);
    }

    /// Runs the real script against the running classic Outlook. `cargo test -- --ignored outlook_live`
    #[test]
    #[ignore]
    fn outlook_live() {
        let dir = std::env::temp_dir().join("todo-helper-outlook-live");
        let since = (Utc::now() - Duration::days(3)).format(TIME).to_string();
        let reply = parse(&run_script(&dir, &since).unwrap()).unwrap();
        println!("{} accounts, {} messages", reply.accounts, reply.mail.len());
        assert!(reply.accounts > 0);
    }
}
