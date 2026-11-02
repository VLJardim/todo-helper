//! Settings: secrets in the system's password store, everything else in the SQLite settings table.
use crate::db::{get_setting, set_setting};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeMap;

/// The system's password store, by the name people know it under.
#[cfg(windows)]
pub const STORE: &str = "Windows Credential Manager";
#[cfg(target_os = "macos")]
pub const STORE: &str = "Keychain";
#[cfg(not(any(windows, target_os = "macos")))]
pub const STORE: &str = "the system's password store";

/// Secrets you can set in Settings. On Windows each is a generic credential named "todo-helper:<name>",
/// on macOS a Keychain item for the service "todo-helper".
pub const SECRETS: [&str; 3] = ["discord_bot_token", "itslearning_feed_url", "gcal_feed_urls"];

/// Plain settings and their defaults. The calendar's Project URL and publishable key are meant to
/// be public; Supabase's rules protect the data.
pub const PLAIN: [(&str, &str); 9] = [
    ("gcal_interval_min", "720"),
    // The board's open days: "all", or the open dates (YYYY-MM-DD) separated by commas.
    ("board_open_days", "all"),
    ("discord_channel_ids", ""),
    ("email_interval_min", "5"),
    ("discord_interval_min", "5"),
    ("itslearning_interval_min", "30"),
    ("calendar_url", ""),
    ("calendar_key", ""),
    ("calendar_interval_min", "1"),
];

/// Sources that report a status line in Settings.
pub const SOURCES: [&str; 5] = ["outlook", "discord", "itslearning", "calendar", "gcal"];

#[cfg(windows)]
fn entry(name: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new_with_target(&format!("todo-helper:{name}"), "todo-helper", name)
}

// On macOS the target names a keychain, not the item, so the item is named by service and account only.
#[cfg(not(windows))]
fn entry(name: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new("todo-helper", name)
}

fn check_secret_name(name: &str) -> Result<(), String> {
    SECRETS.contains(&name).then_some(()).ok_or_else(|| format!("Unknown secret \"{name}\"."))
}

pub fn set_secret(name: &str, value: &str) -> Result<(), String> {
    check_secret_name(name)?;
    let value = value.trim();
    if value.is_empty() {
        return Err("The value is empty. To remove a secret, use Remove.".into());
    }
    if value.chars().count() > 1000 {
        return Err(format!("That's longer than {STORE} allows. Check that you copied only the password, token, or link."));
    }
    keep(name, value)
}

pub fn clear_secret(name: &str) -> Result<(), String> {
    check_secret_name(name)?;
    forget(name)
}

/// Stores a secret the app makes itself, such as the calendar's sign-in, without the checks for typed values.
pub(crate) fn keep(name: &str, value: &str) -> Result<(), String> {
    entry(name).and_then(|e| e.set_password(value)).map_err(|e| format!("{STORE} refused the secret: {e}"))
}

pub(crate) fn forget(name: &str) -> Result<(), String> {
    match entry(name).and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("{STORE} could not remove the secret: {e}")),
    }
}

/// The secret's value, for the background jobs only. Never send it to the window.
pub fn secret(name: &str) -> Option<String> {
    entry(name).and_then(|e| e.get_password()).ok()
}

pub fn plain(conn: &Connection, key: &str) -> String {
    let default = PLAIN.iter().find(|(k, _)| *k == key).map(|(_, d)| *d).unwrap_or_default();
    get_setting(conn, key).ok().flatten().unwrap_or_else(|| default.to_string())
}

/// Checks and normalizes a plain setting before it is saved.
pub fn clean_plain(key: &str, value: &str) -> Result<String, String> {
    let value = value.trim();
    match key {
        "discord_channel_ids" => {
            let ids: Vec<&str> = value.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()).collect();
            match ids.iter().find(|id| !((15..=20).contains(&id.len()) && id.chars().all(|c| c.is_ascii_digit()))) {
                Some(_) => Err(
                    "Channel IDs are 15 to 20 digits, separated by commas. In Discord, turn on Developer Mode, right-click the channel, and choose Copy Channel ID."
                        .to_string(),
                ),
                None => Ok(ids.join(", ")),
            }
        }
        // http only for a Supabase running on this computer, for testing.
        "calendar_url" => {
            let url = value.trim_end_matches('/');
            let local = url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost");
            if url.is_empty() || (url.starts_with("https://") && !url.contains(char::is_whitespace)) || local {
                Ok(url.to_string())
            } else {
                Err("Use the Project URL from Supabase, such as https://abcdefgh.supabase.co.".into())
            }
        }
        "calendar_key" => {
            if value.starts_with("sb_secret_") {
                Err("That's the secret key, which skips every rule. Use the publishable key, which starts with sb_publishable_.".into())
            } else if value.is_empty() || value.starts_with("sb_publishable_") || value.starts_with("eyJ") {
                Ok(value.to_string())
            } else {
                Err("The publishable key starts with sb_publishable_. Copy it from Project Settings, API Keys.".into())
            }
        }
        "email_interval_min" | "discord_interval_min" | "itslearning_interval_min" | "calendar_interval_min" | "gcal_interval_min" => match value.parse::<u32>() {
            Ok(n) if (1..=1440).contains(&n) => Ok(n.to_string()),
            _ => Err("Use a whole number of minutes from 1 to 1440.".into()),
        },
        "board_open_days" => {
            let date = |d: &str| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok();
            if value == "all" || value.split(',').filter(|d| !d.is_empty()).all(date) {
                Ok(value.to_string())
            } else {
                Err("Use \"all\" or dates like 2026-10-27, separated by commas.".into())
            }
        }
        _ => Err(format!("Unknown setting \"{key}\".")),
    }
}

#[derive(Serialize, serde::Deserialize)]
pub struct Status {
    pub ok: bool,
    pub message: String,
    pub at: String,
}

/// Records how the last check of a source went, for the Settings view.
pub fn set_status(conn: &Connection, source: &str, ok: bool, message: &str) {
    let status = Status { ok, message: message.to_string(), at: chrono::Utc::now().to_rfc3339() };
    let _ = set_setting(conn, &format!("status.{source}"), &serde_json::to_string(&status).unwrap_or_default());
}

pub fn clear_status(conn: &Connection, source: &str) {
    let _ = conn.execute("DELETE FROM settings WHERE key = ?1", [format!("status.{source}")]);
}

/// Outlook is read through classic Outlook's COM interface, which only exists on Windows.
pub const OUTLOOK_WORKS: bool = cfg!(windows);

#[derive(Serialize)]
pub struct View {
    pub plain: BTreeMap<String, String>,
    /// Whether each secret is set. Never the value.
    pub secrets: BTreeMap<String, bool>,
    pub status: BTreeMap<String, Option<Status>>,
    pub store: &'static str,
    pub outlook_works: bool,
}

pub fn view(conn: &Connection) -> View {
    View {
        store: STORE,
        outlook_works: OUTLOOK_WORKS,
        plain: PLAIN.iter().map(|(k, _)| (k.to_string(), plain(conn, k))).collect(),
        secrets: SECRETS.iter().map(|n| (n.to_string(), secret(n).is_some())).collect(),
        status: SOURCES
            .iter()
            .map(|s| {
                let st = get_setting(conn, &format!("status.{s}")).ok().flatten().and_then(|v| serde_json::from_str(&v).ok());
                (s.to_string(), st)
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_settings_are_checked() {
        assert!(clean_plain("gmail_address", "me@gmail.com").is_err(), "Gmail was removed");
        assert!(clean_plain("discord_channel_ids", &"1".repeat(40)).is_err());
        assert!(clean_plain("foo_interval_min", "5").is_err());
        assert!(clean_plain("status.gmail_interval_min", "5").is_err());
        assert_eq!(
            clean_plain("discord_channel_ids", "123456789012345678,  234567890123456789\n").unwrap(),
            "123456789012345678, 234567890123456789"
        );
        assert!(clean_plain("discord_channel_ids", "#general").is_err());
        assert_eq!(clean_plain("email_interval_min", "10").unwrap(), "10");
        assert!(clean_plain("email_interval_min", "0").is_err());
        assert!(clean_plain("email_interval_min", "five").is_err());
        assert!(clean_plain("password", "x").is_err());
        assert_eq!(clean_plain("board_open_days", "all").unwrap(), "all");
        assert_eq!(clean_plain("board_open_days", "").unwrap(), "");
        assert_eq!(clean_plain("board_open_days", "2026-10-27,2026-10-29").unwrap(), "2026-10-27,2026-10-29");
        assert!(clean_plain("board_open_days", "tuesday").is_err());
        assert_eq!(clean_plain("calendar_url", " https://abc.supabase.co/ ").unwrap(), "https://abc.supabase.co");
        assert!(clean_plain("calendar_url", "abc.supabase.co").is_err());
        assert!(clean_plain("calendar_url", "http://abc.supabase.co").is_err(), "only https leaves the computer");
        assert_eq!(clean_plain("calendar_url", "http://127.0.0.1:54321").unwrap(), "http://127.0.0.1:54321");
        assert!(clean_plain("calendar_key", "sb_publishable_abc").is_ok());
        assert!(clean_plain("calendar_key", "sb_secret_abc").unwrap_err().contains("secret key"));
        assert!(clean_plain("calendar_key", "hello").is_err());
        assert_eq!(clean_plain("calendar_key", "").unwrap(), "");
    }

    #[test]
    fn unknown_secret_names_are_refused() {
        assert!(set_secret("anything_else", "x").is_err());
        assert!(clear_secret("anything_else").is_err());
    }

    /// Writes to the real password store: `cargo test -- --ignored password_store_live`.
    #[test]
    #[ignore]
    fn password_store_live() {
        keep("live_check", "s3cret").unwrap();
        assert_eq!(secret("live_check").as_deref(), Some("s3cret"));
        forget("live_check").unwrap();
        assert_eq!(secret("live_check"), None);
        forget("live_check").unwrap(); // removing twice is fine
    }
}
