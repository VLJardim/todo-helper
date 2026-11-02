//! Runs each source's check on its own schedule, and on demand from Settings.
use crate::{calendar, db, discord, extract, gcal, itslearning, outlook, settings};
use rusqlite::Connection;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

fn interval_key(source: &str) -> &'static str {
    match source {
        "outlook" => "email_interval_min",
        "discord" => "discord_interval_min",
        "calendar" => "calendar_interval_min",
        "gcal" => "gcal_interval_min",
        _ => "itslearning_interval_min",
    }
}

/// Starts one thread per source: check now, then again every interval. The interval is read
/// again every minute, so a shorter one from Settings takes effect within a minute.
pub fn start(app: AppHandle, db_path: PathBuf) {
    for source in settings::SOURCES {
        if source == "outlook" && !settings::OUTLOOK_WORKS {
            continue;
        }
        let (app, db_path) = (app.clone(), db_path.clone());
        std::thread::spawn(move || loop {
            check(&app, &db_path, source, false);
            let mut waited = 0;
            loop {
                std::thread::sleep(Duration::from_secs(60));
                waited += 1;
                let minutes = db::open(&db_path)
                    .ok()
                    .and_then(|c| settings::plain(&c, interval_key(source)).parse::<u64>().ok())
                    .unwrap_or(5);
                if waited >= minutes {
                    break;
                }
            }
        });
    }
}

// Sources being checked right now, so a "Check now" and a scheduled check never overlap.
static RUNNING: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

fn not_set_up(source: &str) -> &'static str {
    match source {
        "discord" => "Not set up: add the bot token and at least one channel ID, then press Check now.",
        "calendar" => "Not set up: add the Project URL and the publishable key, then sign in.",
        "gcal" => "Not set up: add your Google calendar's secret address in iCal format, then press Check now.",
        _ => "Not set up: add the calendar feed link, then press Check now.",
    }
}

/// Checks one source, records the outcome as its status line, and tells the window.
/// `manual` is true for Check now, which always gets an answer, even "not set up".
pub fn check(app: &AppHandle, db_path: &Path, source: &'static str, manual: bool) {
    {
        let mut running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
        if running.contains(&source) {
            return;
        }
        running.push(source);
    }
    // A bug in one check must not take the app or the source's thread down with it, or leave the source marked as running.
    let work = catch_unwind(AssertUnwindSafe(|| run_check(app, db_path, source, manual)));
    if work.is_err() {
        if let Ok(conn) = db::open(db_path) {
            settings::set_status(&conn, source, false, "The check stopped on something unexpected. The app tries again at the next check.");
        }
    }
    RUNNING.lock().unwrap_or_else(|e| e.into_inner()).retain(|s| *s != source);
    // The payload names the source, so Settings knows which Check now finished.
    let _ = app.emit("settings-changed", source);
}

fn run_check(app: &AppHandle, db_path: &Path, source: &'static str, manual: bool) {
    let conn = match db::open(db_path) {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("{source}: can't open the database: {e}");
            return;
        }
    };
    let connected = |m: Option<String>| m.map(|m| (true, m));
    let mut result = match source {
        "outlook" if !settings::OUTLOOK_WORKS => Err("Outlook only works on Windows, with classic Outlook.".to_string()),
        "outlook" => outlook::fetch_new(&conn, &db_path.parent().unwrap_or(db_path).join("outlook"))
            .map(|(n, accounts)| Some((true, format!("Connected. {} from {}.", plural(n, "new message"), plural(accounts, "account"))))),
        "itslearning" => check_itslearning(app, &conn).map(connected),
        "discord" => check_discord(&conn),
        "calendar" => check_calendar(app, &conn).map(connected),
        "gcal" => check_gcal(app, &conn).map(connected),
        _ => Ok(None),
    };
    // New Outlook mail and Discord items become cards (items left over from a failed run go too),
    // even when one Discord channel failed, so the others' messages still arrive.
    if source == "outlook" || source == "discord" {
        if let Ok(Some((ok, message))) = &mut result {
            match extract::run(&conn, &db_path.parent().unwrap_or(db_path).join("extract")) {
                Ok(0) => {}
                Ok(n) => {
                    let _ = app.emit("board-changed", ());
                    *message = format!("{message} {} added to the board.", plural(n, "card"));
                }
                Err(e) => {
                    *ok = false;
                    *message = format!("{message} Turning them into cards failed: {e}");
                }
            }
        }
    }
    match result {
        Ok(Some((ok, message))) => settings::set_status(&conn, source, ok, &message),
        // Not set up. A scheduled check clears an old "Connected" line, for example after Remove.
        Ok(None) if manual => settings::set_status(&conn, source, false, not_set_up(source)),
        Ok(None) => settings::clear_status(&conn, source),
        Err(message) => settings::set_status(&conn, source, false, &message),
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn check_calendar(app: &AppHandle, conn: &Connection) -> Result<Option<String>, String> {
    let Some(s) = calendar::sync(conn)? else { return Ok(None) };
    if s.changed {
        let _ = app.emit("calendar-changed", ());
    }
    Ok(Some(format!("Connected. {} from {}.", plural(s.events, "event"), plural(s.members, "member"))))
}

fn check_gcal(app: &AppHandle, conn: &Connection) -> Result<Option<String>, String> {
    let Some(feeds) = settings::secret("gcal_feed_urls") else { return Ok(None) };
    let s = gcal::sync(conn, &feeds)?;
    if s.added + s.updated + s.removed > 0 {
        // Bring the copy up to date so the Calendar view and the board show the change now.
        let _ = calendar::sync(conn);
        let _ = app.emit("calendar-changed", ());
    }
    Ok(Some(format!(
        "Connected. {} in the next 26 weeks: {} added, {} updated, {} removed.",
        plural(s.found, "event"),
        s.added,
        s.updated,
        s.removed
    )))
}

fn check_itslearning(app: &AppHandle, conn: &Connection) -> Result<Option<String>, String> {
    let Some(link) = settings::secret("itslearning_feed_url") else { return Ok(None) };
    let s = itslearning::sync(conn, &link)?;
    if s.added + s.updated > 0 {
        let _ = app.emit("board-changed", ());
    }
    Ok(Some(format!(
        "Connected. {} in the next 26 weeks: {} added, {} with a new date.",
        plural(s.upcoming, "event"),
        s.added,
        s.updated
    )))
}

/// Ok(Some((false, ..))) means some channels failed; the others were still read.
fn check_discord(conn: &Connection) -> Result<Option<(bool, String)>, String> {
    let Some(token) = settings::secret("discord_bot_token") else { return Ok(None) };
    let ids = settings::plain(conn, "discord_channel_ids");
    let ids: Vec<&str> = ids.split(", ").filter(|s| !s.is_empty()).collect();
    if ids.is_empty() {
        return Ok(None);
    }
    let f = discord::fetch_new(conn, &token, &ids);
    let read = ids.len() - f.errors.len();
    let mut message = if f.errors.is_empty() {
        format!("Connected. {} in {}.", plural(f.stored, "new message"), plural(ids.len(), "channel"))
    } else {
        format!("{} {} from the other {}.", f.errors.join(" "), plural(f.stored, "new message"), plural(read, "channel"))
    };
    if f.blank > 0 && f.stored == 0 {
        message.push_str(" Messages came back without text: turn on Message Content Intent on the Bot page at discord.com/developers.");
    }
    Ok(Some((f.errors.is_empty(), message)))
}
