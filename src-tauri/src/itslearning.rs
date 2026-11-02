//! Reads the itslearning calendar feed (iCal) and keeps one card per event in the next 26 weeks.
use crate::db;
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::time::Duration;

/// New cards are made for events in the next 26 weeks, as far as the Calendar view pages. Lessons
/// stay off the board (board.ts), so a long window only fills the Calendar view.
const DAYS_AHEAD: i64 = 182;
const MAX_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub struct Event {
    /// The UID, plus "#" and the RECURRENCE-ID for a changed occurrence of a repeating event.
    pub key: String,
    pub summary: String,
    pub date: NaiveDate,
    /// Start and end in UTC (RFC 3339), for an event with a time of day and an end.
    pub times: Option<(String, String)>,
    pub cancelled: bool,
}

pub struct Synced {
    pub upcoming: usize,
    pub added: usize,
    pub updated: usize,
}

/// Downloads the feed and syncs its events to the board.
pub fn sync(conn: &Connection, link: &str) -> Result<Synced, String> {
    let ics = download(link)?;
    apply(conn, &parse(&ics), Local::now().date_naive()).map_err(|e| format!("Couldn't save calendar events: {e}"))
}

fn download(link: &str) -> Result<Vec<u8>, String> {
    let link = link.trim();
    let lower = link.to_ascii_lowercase();
    let url = if lower.starts_with("webcal://") { format!("https://{}", &link[9..]) } else { link.to_string() };
    if !(lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("webcal://")) {
        return Err("The calendar feed link should start with https:// or webcal://. Copy it again from Calendar, then Subscribe, in itslearning.".into());
    }
    // Error texts below never include the URL: it holds a private token.
    let response = ureq::get(&url).timeout(Duration::from_secs(30)).call().map_err(|e| match e {
        ureq::Error::Status(code @ (401 | 403 | 404 | 410), _) => format!(
            "itslearning refused the feed link (HTTP {code}). Copy a new link from Calendar, then Subscribe, in itslearning, and save it here."
        ),
        ureq::Error::Status(code, _) => format!("itslearning answered with HTTP {code}. The app tries again at the next check."),
        ureq::Error::Transport(t) => match t.kind() {
            ureq::ErrorKind::TooManyRedirects | ureq::ErrorKind::BadHeader | ureq::ErrorKind::InvalidUrl => format!(
                "The feed link led somewhere unexpected ({}). Copy the link again from Calendar, then Subscribe, in itslearning.",
                t.kind()
            ),
            kind => format!("Couldn't reach itslearning ({kind}). Check the internet connection; the app tries again at the next check."),
        },
    })?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Couldn't read the calendar: {e}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The calendar is larger than 10 MB, which is more than the app reads. Check that the link is your own calendar feed.".into());
    }
    if !bytes.to_ascii_uppercase().windows(15).any(|w| w == b"BEGIN:VCALENDAR") {
        return Err("The feed link didn't return a calendar. Check that you copied the Subscribe link, not the page address.".into());
    }
    Ok(bytes)
}

/// Joins folded lines (a line break followed by a space or tab) before decoding, so a UTF-8
/// letter split across a fold comes back whole.
fn unfold(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let fold = match (bytes[i], bytes.get(i + 1), bytes.get(i + 2)) {
            (b'\r', Some(b'\n'), Some(b' ' | b'\t')) => 3,
            (b'\n', Some(b' ' | b'\t'), _) => 2,
            _ => 0,
        };
        if fold > 0 {
            i += fold;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// Splits "NAME;PARAMS:value" at the first colon outside double quotes, since a quoted
/// TZID such as "(UTC+01:00) Amsterdam" can hold one.
fn split_property(line: &str) -> Option<(&str, &str)> {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => return Some((&line[..i], &line[i + 1..])),
            _ => {}
        }
    }
    None
}

/// Parses VEVENTs from iCal data. Properties of nested blocks such as VALARM are ignored.
// ponytail: RRULE is not expanded, so only an event's first date counts; expand it if itslearning sends repeating assignments.
pub fn parse(ics: &[u8]) -> Vec<Event> {
    let text = String::from_utf8_lossy(&unfold(ics)).into_owned();
    let mut events = Vec::new();
    let mut seen = HashSet::new();
    let mut current: Option<HashMap<String, String>> = None;
    let mut nested = 0;
    for line in text.lines() {
        let upper = line.trim_end().to_ascii_uppercase();
        if upper == "BEGIN:VEVENT" {
            current = Some(HashMap::new());
            nested = 0;
        } else if upper == "END:VEVENT" {
            if let Some(e) = current.take().and_then(to_event) {
                if seen.insert(e.key.clone()) {
                    events.push(e);
                }
            }
        } else if let Some(props) = current.as_mut() {
            if upper.starts_with("BEGIN:") {
                nested += 1;
            } else if upper.starts_with("END:") {
                nested -= 1;
            } else if nested == 0 {
                let Some((head, value)) = split_property(line) else { continue };
                let name = head.split(';').next().unwrap_or_default().to_ascii_uppercase();
                props.entry(name).or_insert(value.to_string());
            }
        }
    }
    events
}

fn to_event(p: HashMap<String, String>) -> Option<Event> {
    let uid = p.get("UID")?.trim().to_string();
    if uid.is_empty() {
        return None;
    }
    let key = match p.get("RECURRENCE-ID") {
        Some(v) => format!("{uid}#{}", v.trim()),
        None => uid,
    };
    let date = start_date(p.get("DTSTART")?)?;
    let summary = p
        .get("SUMMARY")
        .map(|v| unescape(v).chars().take(200).collect::<String>())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "itslearning event".into());
    let cancelled = p.get("STATUS").is_some_and(|v| v.trim().eq_ignore_ascii_case("CANCELLED"));
    let times = match (p.get("DTSTART").and_then(|v| date_time(v)), p.get("DTEND").and_then(|v| date_time(v))) {
        (Some(s), Some(e)) if e >= s => Some((stamp(s), stamp(e))),
        _ => None,
    };
    Some(Event { key, summary, date, times, cancelled })
}

/// "20261022T090000" (local or with TZID) or "20261022T080000Z" (UTC) as a UTC time; a plain
/// date has no time, so it gives None.
fn date_time(v: &str) -> Option<DateTime<Utc>> {
    let v = v.trim();
    if let Some(utc) = v.strip_suffix('Z').or_else(|| v.strip_suffix('z')) {
        let t = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(t, Utc));
    }
    let t = NaiveDateTime::parse_from_str(v.get(..15)?, "%Y%m%dT%H%M%S").ok()?;
    Local.from_local_datetime(&t).earliest().map(|t| t.with_timezone(&Utc))
}

fn stamp(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// "20261022", "20261022T090000" (local or with TZID), or "20261022T080000Z" (UTC, shown as the local date).
// ponytail: a TZID time is read as local time, which is right while the school and the owner share a time zone.
fn start_date(v: &str) -> Option<NaiveDate> {
    let v = v.trim();
    if let Some(utc) = v.strip_suffix('Z').or_else(|| v.strip_suffix('z')) {
        let t = chrono::NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(t, Utc).with_timezone(&Local).date_naive());
    }
    NaiveDate::parse_from_str(v.get(..8)?, "%Y%m%d").ok()
}

fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push(' '),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out.trim().to_string()
}

/// Adds a card for each new event in the next 26 weeks. When itslearning changes an event's date,
/// or cancels it, the card follows, wherever the new date is. A due date you set yourself stays
/// until itslearning changes the event again, and a card you deleted is not added again.
pub fn apply(conn: &Connection, events: &[Event], today: NaiveDate) -> rusqlite::Result<Synced> {
    let end = today + chrono::Duration::days(DAYS_AHEAD);
    let in_window = |e: &Event| !e.cancelled && e.date >= today && e.date < end;
    let tx = conn.unchecked_transaction()?;
    let (mut added, mut updated) = (0, 0);
    for e in events {
        let external_id = format!("itslearning:{}", e.key);
        let due = e.date.format("%Y-%m-%d").to_string();
        // The date, plus the times for a timed event. Rows saved before times were kept hold the
        // date alone, so the next sync adds the times to those cards.
        let state = match (&e.times, e.cancelled) {
            (_, true) => "cancelled".to_string(),
            (Some((s, end)), _) => format!("{due} {s}/{end}"),
            (None, _) => due.clone(),
        };
        let (starts_at, ends_at) = e.times.clone().unzip();
        let synced: Option<Option<String>> =
            tx.query_row("SELECT due FROM synced_events WHERE uid = ?1", [&e.key], |r| r.get(0)).optional()?;
        match synced {
            None if in_window(e) => {
                let col = if e.date == today { "today" } else { "inbox" };
                let id = db::add_source_card(&tx, &e.summary, col, Some(&due), "itslearning", None, Some(&external_id))?;
                tx.execute("UPDATE cards SET starts_at = ?2, ends_at = ?3 WHERE id = ?1", params![id, starts_at, ends_at])?;
                tx.execute("INSERT INTO synced_events (uid, due) VALUES (?1, ?2)", params![e.key, state])?;
                added += 1;
            }
            None => {}
            Some(last) if last.as_deref() == Some(state.as_str()) => {}
            Some(last) => {
                // itslearning changed the event since the last sync.
                tx.execute("UPDATE synced_events SET due = ?2 WHERE uid = ?1", params![e.key, state])?;
                let card: Option<(i64, String)> = tx
                    .query_row("SELECT id, col FROM cards WHERE external_id = ?1", [&external_id], |r| Ok((r.get(0)?, r.get(1)?)))
                    .optional()?;
                let Some((id, col)) = card else { continue };
                if e.cancelled {
                    tx.execute(
                        "UPDATE cards SET due = NULL, title = 'Cancelled: ' || title WHERE id = ?1 AND title NOT LIKE 'Cancelled: %'",
                        [id],
                    )?;
                } else {
                    // A cancelled event that itslearning restored loses the "Cancelled: " it got.
                    if last.as_deref() == Some("cancelled") {
                        tx.execute("UPDATE cards SET title = substr(title, 12) WHERE id = ?1 AND title LIKE 'Cancelled: %'", [id])?;
                    }
                    tx.execute("UPDATE cards SET starts_at = ?2, ends_at = ?3 WHERE id = ?1", params![id, starts_at, ends_at])?;
                    // Same day, new times (or times seen for the first time): only the times
                    // change, so a date you set yourself stays.
                    if last.as_deref().and_then(|l| l.get(..10)) == Some(due.as_str()) {
                        updated += 1;
                        continue;
                    }
                    tx.execute("UPDATE cards SET due = ?2 WHERE id = ?1", params![id, due])?;
                    if e.date == today && col == "inbox" {
                        tx.execute(
                            "UPDATE cards SET col = 'today', position = (SELECT COALESCE(MIN(position), 0) - 1 FROM cards WHERE col = 'today') WHERE id = ?1",
                            [id],
                        )?;
                    }
                }
                updated += 1;
            }
        }
    }
    tx.commit()?;
    Ok(Synced { upcoming: events.iter().filter(|e| in_window(e)).count(), added, updated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::day;

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//itslearning//EN\r\n\
BEGIN:VEVENT\r\nUID:event-1@itslearning.com\r\nDTSTART;VALUE=DATE:20261022\r\nSUMMARY:Danish: hand in essay\\, part 2\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:event-2@itslearning.com\r\nDTSTART;TZID=Europe/Copenhagen:20261020T081500\r\nDTEND;TZID=Europe/Copenhagen:20261020T094500\r\n\
SUMMARY:Physics lab: bring safety gogg\r\n les\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:event-3@itslearning.com\r\nDTSTART:20261021T080000Z\r\nSUMMARY:Cancelled trip\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:event-old@itslearning.com\r\nDTSTART;VALUE=DATE:20260101\r\nSUMMARY:Old\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn parses_events_with_folding_escapes_and_time_zones() {
        let events = parse(SAMPLE.as_bytes());
        assert_eq!(events.len(), 4);
        assert_eq!(
            events[0],
            Event {
                key: "event-1@itslearning.com".into(),
                summary: "Danish: hand in essay, part 2".into(),
                date: day("2026-10-22"),
                times: None,
                cancelled: false
            }
        );
        assert_eq!(events[1].summary, "Physics lab: bring safety goggles");
        assert_eq!(events[1].date, day("2026-10-20"));
        assert!(events[2].cancelled);
    }

    #[test]
    fn real_world_feed_quirks() {
        // "ø" is two bytes; the fold falls between them. Lowercase names, a quoted TZID with a
        // colon, an alarm with its own SUMMARY, and a changed occurrence of a repeating event.
        let mut ics = b"begin:vcalendar\nbegin:vevent\nuid:a\ndtstart;value=date:20261021\nsummary:Sm\xc3".to_vec();
        ics.extend_from_slice(b"\n \xb8rrebr\xc3\xb8d\nend:vevent\n");
        ics.extend_from_slice(b"BEGIN:VEVENT\r\nUID:b\r\nDTSTART;TZID=\"(UTC+01:00) Amsterdam, Berlin\":20261023T090000\r\n");
        ics.extend_from_slice(b"BEGIN:VALARM\r\nSUMMARY:Alarm text\r\nEND:VALARM\r\nEND:VEVENT\r\n");
        ics.extend_from_slice(b"BEGIN:VEVENT\r\nUID:c\r\nDTSTART:20261026T080000Z\r\nRRULE:FREQ=WEEKLY\r\nSUMMARY:Choir\r\nEND:VEVENT\r\n");
        ics.extend_from_slice(b"BEGIN:VEVENT\r\nUID:c\r\nRECURRENCE-ID:20261102T080000Z\r\nDTSTART:20261103T080000Z\r\nSUMMARY:Choir (moved)\r\nEND:VEVENT\r\n");
        let events = parse(&ics);
        assert_eq!(events[0].summary, "Smørrebrød");
        assert_eq!((events[1].key.as_str(), events[1].date, events[1].summary.as_str()), ("b", day("2026-10-23"), "itslearning event"));
        assert_eq!(events.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c", "c#20261102T080000Z"]);
    }

    #[test]
    fn syncing_adds_once_and_respects_deletions() {
        let conn = db::open(&db::temp_path("ical")).unwrap();
        let events = parse(SAMPLE.as_bytes());
        let first = apply(&conn, &events, day("2026-10-20")).unwrap();
        assert_eq!((first.upcoming, first.added, first.updated), (2, 2, 0), "the old and the cancelled events get no card");
        let lab = db::list_cards(&conn).unwrap().into_iter().find(|c| c.title.starts_with("Physics")).unwrap();
        assert_eq!((lab.col.as_str(), lab.source.as_str()), ("today", "itslearning"));

        // Nothing changed in the feed: nothing changes on the board, again and again.
        let again = apply(&conn, &events, day("2026-10-20")).unwrap();
        assert_eq!((again.added, again.updated), (0, 0));

        // A card the owner deleted stays deleted.
        let essay = db::list_cards(&conn).unwrap().into_iter().find(|c| c.title.starts_with("Danish")).unwrap().id;
        db::delete_card(&conn, essay).unwrap();
        assert_eq!(apply(&conn, &events, day("2026-10-20")).unwrap().added, 0);
        assert_eq!(db::list_cards(&conn).unwrap().len(), 1);
    }

    #[test]
    fn cards_follow_moved_and_cancelled_events_but_keep_your_own_dates() {
        let conn = db::open(&db::temp_path("ical-moves")).unwrap();
        let mut events = parse(SAMPLE.as_bytes());
        apply(&conn, &events, day("2026-10-20")).unwrap();
        let essay = || db::list_cards(&conn).unwrap().into_iter().find(|c| c.title.contains("essay")).unwrap();

        // You set your own date: the next sync leaves it alone.
        db::set_due(&conn, essay().id, Some("2026-10-21")).unwrap();
        apply(&conn, &events, day("2026-10-20")).unwrap();
        assert_eq!(essay().due.as_deref(), Some("2026-10-21"));

        // itslearning moves the essay far ahead, out of the window: the card follows.
        events[0].date = day("2026-12-19");
        assert_eq!(apply(&conn, &events, day("2026-10-20")).unwrap().updated, 1);
        assert_eq!(essay().due.as_deref(), Some("2026-12-19"));

        // Moved to today: the card goes to Today.
        events[0].date = day("2026-10-20");
        apply(&conn, &events, day("2026-10-20")).unwrap();
        assert_eq!((essay().due.as_deref(), essay().col.as_str()), (Some("2026-10-20"), "today"));

        // Cancelled: the card says so and loses its date, once.
        events[0].cancelled = true;
        apply(&conn, &events, day("2026-10-20")).unwrap();
        apply(&conn, &events, day("2026-10-20")).unwrap();
        assert_eq!((essay().title.as_str(), essay().due.as_deref()), ("Cancelled: Danish: hand in essay, part 2", None));

        // Restored: the title loses "Cancelled: " and the date comes back.
        events[0].cancelled = false;
        apply(&conn, &events, day("2026-10-20")).unwrap();
        assert_eq!((essay().title.as_str(), essay().due.as_deref()), ("Danish: hand in essay, part 2", Some("2026-10-20")));
    }

    #[test]
    fn lessons_keep_their_times_and_older_cards_get_them() {
        let conn = db::open(&db::temp_path("ical-times")).unwrap();
        let mut events = parse(SAMPLE.as_bytes());
        // The sample's TZID times are read as this computer's local time, like the rest of the feed.
        let local = |s: &str| stamp(Local.from_local_datetime(&NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S").unwrap()).earliest().unwrap().with_timezone(&Utc));
        let lab_times = Some((local("20261020T081500"), local("20261020T094500")));
        assert_eq!(events[1].times, lab_times);
        assert_eq!(events[0].times, None, "a date alone has no time");

        apply(&conn, &events, day("2026-10-20")).unwrap();
        let lab = || db::list_cards(&conn).unwrap().into_iter().find(|c| c.title.starts_with("Physics")).unwrap();
        assert_eq!((lab().starts_at, lab().ends_at), lab_times.clone().unzip());

        // A card from before times were kept, which you moved to another day: the next sync adds
        // the times and leaves your date alone.
        conn.execute("UPDATE synced_events SET due = '2026-10-20' WHERE uid = 'event-2@itslearning.com'", []).unwrap();
        conn.execute("UPDATE cards SET starts_at = NULL, ends_at = NULL", []).unwrap();
        db::set_due(&conn, lab().id, Some("2026-10-23")).unwrap();
        assert_eq!(apply(&conn, &events, day("2026-10-20")).unwrap().updated, 1);
        assert_eq!((lab().due.as_deref(), lab().starts_at.is_some()), (Some("2026-10-23"), true));

        // itslearning moves the lesson an hour later on the same day: the times follow, once.
        events[1].times = Some((local("20261020T091500"), local("20261020T104500")));
        assert_eq!(apply(&conn, &events, day("2026-10-20")).unwrap().updated, 1);
        assert_eq!(lab().starts_at, Some(local("20261020T091500")));
        assert_eq!(apply(&conn, &events, day("2026-10-20")).unwrap().updated, 0);
    }

    #[test]
    fn feed_links_are_checked_before_any_request() {
        assert!(download("ftp://example.com/cal.ics").unwrap_err().contains("https://"));
        assert!(download("calendar").is_err());
    }
}
