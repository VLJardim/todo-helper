//! Reads new messages from the chosen Discord channels through the REST API, with a bot token.
//! No live connection: each check asks for messages newer than the last one seen per channel.
use crate::db::{add_inbox_item, get_setting, set_setting};
use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use serde_json::Value;

const API: &str = "https://discord.com/api/v10";

pub struct Message {
    pub id: u64,
    pub author: String,
    pub content: String,
    pub sent_at: String,
    /// Discord's message type: 0 is a normal message, 19 a reply. Others are system messages.
    pub kind: i64,
    pub has_media: bool,
}

pub struct Fetched {
    pub stored: usize,
    /// Normal messages that came back with no text and nothing attached: the Message Content intent is off.
    pub blank: usize,
    /// One message per channel that failed. The other channels are still read.
    pub errors: Vec<String>,
}

pub fn fetch_new(conn: &Connection, token: &str, channel_ids: &[&str]) -> Fetched {
    let mut out = Fetched { stored: 0, blank: 0, errors: Vec::new() };
    for &channel in channel_ids {
        if let Err(e) = fetch_channel(conn, token, channel, &mut out) {
            out.errors.push(e);
        }
    }
    out
}

fn fetch_channel(conn: &Connection, token: &str, channel: &str, out: &mut Fetched) -> Result<(), String> {
    let db_err = |e: rusqlite::Error| format!("Couldn't save Discord messages: {e}");
    let info = get(token, &format!("{API}/channels/{channel}"), channel)?;
    let name = info["name"].as_str().map(|n| format!("#{n}")).unwrap_or_else(|| format!("channel {channel}"));
    let key = format!("discord_last.{channel}");
    let last: Option<u64> = get_setting(conn, &key).map_err(db_err)?.and_then(|v| v.parse().ok());

    // ponytail: 100 messages per channel per check; a busier channel catches up over the next checks.
    let url = match last {
        Some(id) => format!("{API}/channels/{channel}/messages?limit=100&after={id}"),
        None => format!("{API}/channels/{channel}/messages?limit=50"),
    };
    let messages = parse_messages(&get(token, &url, channel)?);
    let since = (Utc::now() - Duration::days(2)).to_rfc3339();
    let (keep, blank, newest) = select(&messages, last, &since);
    out.blank += blank;
    for m in keep {
        out.stored += add_inbox_item(conn, "discord", &m.id.to_string(), &m.author, &name, &m.sent_at, &m.content.chars().take(4000).collect::<String>()).map_err(db_err)?;
    }
    if let Some(id) = newest {
        set_setting(conn, &key, &id.to_string()).map_err(db_err)?;
    }
    Ok(())
}

/// Which messages to store, how many look blank, and the id to remember. The first check of a
/// channel (`last` is None) only takes messages since `since`, so old chat doesn't flood the board.
/// When every new message came back blank, the Message Content intent is probably off, so the id
/// stays put and turning the intent on brings those messages back. The id never moves backwards.
pub fn select<'a>(messages: &'a [Message], last: Option<u64>, since: &str) -> (Vec<&'a Message>, usize, Option<u64>) {
    let fresh: Vec<&Message> = messages.iter().filter(|m| last.is_some() || m.sent_at.as_str() >= since).collect();
    let keep = fresh.iter().copied().filter(|m| !m.content.trim().is_empty()).collect();
    let blank = fresh.iter().filter(|m| m.content.trim().is_empty() && !m.has_media && (m.kind == 0 || m.kind == 19)).count();
    let all_blank = blank > 0 && blank == fresh.len();
    (keep, blank, if all_blank { last } else { messages.iter().map(|m| m.id).chain(last).max() })
}

/// Messages from a Discord API list, oldest first. Snowflake ids are compared as numbers.
pub fn parse_messages(list: &Value) -> Vec<Message> {
    let mut messages: Vec<Message> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let author = &m["author"];
            Some(Message {
                id: m["id"].as_str()?.parse().ok()?,
                author: author["global_name"].as_str().or(author["username"].as_str()).unwrap_or("someone").to_string(),
                // A forward keeps its text in a snapshot, and a poll in its question.
                content: [&m["content"], &m["message_snapshots"][0]["message"]["content"]]
                    .iter()
                    .filter_map(|v| v.as_str())
                    .find(|s| !s.trim().is_empty())
                    .map(str::to_string)
                    .or_else(|| m["poll"]["question"]["text"].as_str().map(|q| format!("Poll: {q}")))
                    .unwrap_or_default(),
                sent_at: m["timestamp"].as_str().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.to_rfc3339()).unwrap_or_default(),
                kind: m["type"].as_i64().unwrap_or(0),
                has_media: [&m["attachments"], &m["embeds"], &m["sticker_items"]].iter().any(|v| v.as_array().is_some_and(|a| !a.is_empty())),
            })
        })
        .collect();
    messages.sort_by_key(|m| m.id);
    messages
}

fn get(token: &str, url: &str, channel: &str) -> Result<Value, String> {
    let response = ureq::get(url)
        .timeout(std::time::Duration::from_secs(30))
        .set("Authorization", &format!("Bot {token}"))
        .set("User-Agent", "DiscordBot (https://github.com/todo-helper, 0.1)")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => status_error(code, channel),
            ureq::Error::Transport(t) => format!(
                "Couldn't reach Discord ({}). Check the internet connection; the app tries again at the next check.",
                t.kind()
            ),
        })?;
    response.into_json().map_err(|e| format!("Discord sent something unexpected: {e}"))
}

/// What a failed request means, and how to fix it.
pub fn status_error(code: u16, channel: &str) -> String {
    match code {
        401 => "Discord refused the bot token. Copy a new token from the Bot page of your app at discord.com/developers and save it here.".into(),
        403 => format!(
            "The bot can't read channel {channel}. In Discord, give the bot's role View Channel and Read Message History in that channel."
        ),
        404 => format!("Channel {channel} doesn't exist, or the bot isn't in its server. Check the channel ID, and invite the bot to the server."),
        429 => "Discord asked the app to slow down. It tries again at the next check.".into(),
        _ => format!("Discord answered with HTTP {code}. The app tries again at the next check."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn messages_come_oldest_first_with_numeric_ids() {
        // Discord lists newest first. A 19-digit id sorts after an 18-digit one as a number, not as text.
        let list = json!([
            { "id": "1290000000000000001", "content": "everyone bring your laptop Friday", "timestamp": "2026-10-19T08:00:00.000000+00:00",
              "author": { "username": "mads", "global_name": "Mads" } },
            { "id": "999999999999999999", "content": "", "timestamp": "2026-10-19T07:00:00+00:00", "author": { "username": "bot" } },
            { "id": "not a snowflake", "content": "skipped" }
        ]);
        let messages = parse_messages(&list);
        assert_eq!(messages.iter().map(|m| m.id).collect::<Vec<_>>(), vec![999999999999999999, 1290000000000000001]);
        assert_eq!(messages[1].author, "Mads");
        assert_eq!(messages[0].author, "bot");
        assert_eq!(select(&messages, Some(5), "").2, Some(1290000000000000001));
        assert_eq!(select(&messages, Some(u64::MAX), "").2, Some(u64::MAX), "never moves backwards");
        assert_eq!(select(&[], None, "").2, None);
    }

    #[test]
    fn forwards_and_polls_keep_their_text() {
        let list = json!([
            { "id": "1", "content": "", "message_snapshots": [{ "message": { "content": "hand in the report Friday" } }], "author": {} },
            { "id": "2", "content": "", "poll": { "question": { "text": "Meeting Tuesday or Wednesday?" } }, "author": {} }
        ]);
        let messages = parse_messages(&list);
        assert_eq!(messages[0].content, "hand in the report Friday");
        assert_eq!(messages[1].content, "Poll: Meeting Tuesday or Wednesday?");
    }

    fn msg(id: u64, content: &str, sent_at: &str, kind: i64, has_media: bool) -> Message {
        Message { id, author: "a".into(), content: content.into(), sent_at: sent_at.into(), kind, has_media }
    }

    #[test]
    fn what_each_check_keeps() {
        let since = "2026-10-17T08:00:00+00:00";
        let old = msg(1, "old news", "2026-10-01T08:00:00+00:00", 0, false);
        let new = msg(2, "bring laptop", "2026-10-19T08:00:00+00:00", 0, false);
        let image = msg(3, "", "2026-10-19T09:00:00+00:00", 0, true);
        let joined = msg(4, "", "2026-10-19T09:30:00+00:00", 7, false);
        let blank = msg(5, "", "2026-10-19T10:00:00+00:00", 0, false);

        // First check: only the last 2 days; images and join notices aren't "blank".
        let all = [old, new, image, joined, blank];
        let (keep, blank_count, newest) = select(&all, None, since);
        assert_eq!(keep.iter().map(|m| m.id).collect::<Vec<_>>(), vec![2]);
        assert_eq!((blank_count, newest), (1, Some(5)));

        // Only old messages on the first check: none kept, but the id moves past them.
        let (keep, _, newest) = select(&all[..1], None, since);
        assert!(keep.is_empty());
        assert_eq!(newest, Some(1));

        // Every new message blank: the id stays, so they come back once the intent is on.
        let blanks = [msg(6, "", "2026-10-19T10:00:00+00:00", 0, false), msg(7, " ", "2026-10-19T10:01:00+00:00", 19, false)];
        let (keep, blank_count, newest) = select(&blanks, Some(5), since);
        assert_eq!((keep.len(), blank_count, newest), (0, 2, Some(5)));
    }

    #[test]
    fn errors_say_how_to_fix_them() {
        assert!(status_error(401, "1").contains("bot token"));
        assert!(status_error(403, "123").contains("Read Message History in that channel"));
        assert!(status_error(404, "123").contains("invite the bot"));
        assert!(status_error(429, "1").contains("slow down"));
        assert!(status_error(500, "1").contains("HTTP 500"));
    }
}
