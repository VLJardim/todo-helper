//! Copies Google calendars into the shared calendar (it replaced scripts/sync_gcal.py, removed 2026-10-27). Reads
//! each calendar's secret iCal address and makes your copies match it for the next 26 weeks:
//! adds new events, updates moved or renamed ones, and removes cancelled ones.
//!
//! Scoping: every copy carries "gcal:<key>" in its note, and only those, added under your own
//! sign-in, are ever changed or removed. Events added in the app are never touched. Nothing that
//! already started is removed. Private events aren't copied, and an empty feed changes nothing.
use crate::calendar::{from_json, me, ready, rest};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, Utc};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

const WEEKS: i64 = 26;
const MARK: &str = "gcal:";
/// Zones on Central European time; their offset comes from the EU rule in cet_offset().
const CET_ZONES: [&str; 14] = [
    "Europe/Copenhagen", "Europe/Berlin", "Europe/Oslo", "Europe/Stockholm", "Europe/Paris", "Europe/Amsterdam", "Europe/Brussels",
    "Europe/Vienna", "Europe/Zurich", "Europe/Rome", "Europe/Madrid", "Europe/Prague", "Europe/Warsaw", "Europe/Budapest",
];
const DAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
const STAMP: &str = "%Y%m%dT%H%M%S";

type Params = HashMap<String, String>;
/// One VEVENT: property name -> every (params, value) it had.
type Props = HashMap<String, Vec<(Params, String)>>;
/// What the shared calendar should hold: title, start, end, all day.
type Want = (String, DateTime<Utc>, DateTime<Utc>, bool);

// ---- Reading the iCal feed ----

pub fn parse_ics(text: &str) -> Vec<Props> {
    let unfolded = text.replace("\r\n ", "").replace("\r\n\t", "").replace("\n ", "").replace("\n\t", "");
    let (mut events, mut cur): (Vec<Props>, Option<Props>) = (vec![], None);
    for line in unfolded.lines() {
        let line = line.trim_end_matches('\r');
        if line == "BEGIN:VEVENT" {
            cur = Some(HashMap::new());
        } else if line == "END:VEVENT" {
            events.extend(cur.take());
        } else if let (Some(ev), Some((head, value))) = (cur.as_mut(), line.split_once(':')) {
            let mut parts = head.split(';');
            let name = parts.next().unwrap_or_default().to_uppercase();
            let params = parts.filter_map(|p| p.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
            ev.entry(name).or_default().push((params, value.to_string()));
        }
    }
    events
}

fn prop<'a>(ev: &'a Props, name: &str) -> Option<&'a (Params, String)> {
    ev.get(name).and_then(|v| v.first())
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (c, chars.clone().next()) {
            ('\\', Some(n @ ('\\' | ';' | ','))) => {
                out.push(n);
                chars.next();
            }
            ('\\', Some('n' | 'N')) => {
                out.push('\n');
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Date,
    Utc,
    /// Central European wall-clock time.
    Local,
}

fn when(params: &Params, value: &str) -> Result<(NaiveDateTime, Kind), String> {
    let value = value.trim();
    if params.get("VALUE").map(String::as_str) == Some("DATE") || value.len() == 8 {
        let d = NaiveDate::parse_from_str(value.get(..8).unwrap_or(value), "%Y%m%d").map_err(|_| format!("date {value} isn't valid"))?;
        return Ok((d.and_hms_opt(0, 0, 0).expect("midnight"), Kind::Date));
    }
    let t = NaiveDateTime::parse_from_str(value.get(..15).unwrap_or(value), STAMP).map_err(|_| format!("time {value} isn't valid"))?;
    if value.ends_with('Z') {
        return Ok((t, Kind::Utc));
    }
    let tz = params.get("TZID").map(|z| z.trim_matches('"')).unwrap_or_default();
    if !tz.is_empty() && !CET_ZONES.contains(&tz) {
        return Err(format!("time zone {tz} isn't supported"));
    }
    Ok((t, Kind::Local))
}

/// An iCal DURATION; without one, an all-day event lasts a day and a timed one no time.
fn duration(value: Option<&str>, kind: Kind) -> Result<Duration, String> {
    let Some(v) = value.map(str::trim) else { return Ok(Duration::days(if kind == Kind::Date { 1 } else { 0 })) };
    let bad = || format!("duration {v} isn't supported");
    let rest = v.strip_prefix('P').ok_or_else(bad)?;
    let (mut total, mut num, mut in_time) = (Duration::zero(), String::new(), false);
    for c in rest.chars() {
        match c {
            '0'..='9' => num.push(c),
            'T' => in_time = true,
            _ => {
                let n: i64 = num.parse().map_err(|_| bad())?;
                num.clear();
                total += match (c, in_time) {
                    ('W', false) => Duration::weeks(n),
                    ('D', false) => Duration::days(n),
                    ('H', true) => Duration::hours(n),
                    ('M', true) => Duration::minutes(n),
                    ('S', true) => Duration::seconds(n),
                    _ => return Err(bad()),
                };
            }
        }
    }
    if num.is_empty() { Ok(total) } else { Err(bad()) }
}

/// Hours ahead of UTC for a Central European wall-clock time: CEST from the last Sunday of March
/// 02:00 to the last Sunday of October 03:00, CET otherwise.
fn cet_offset(local: NaiveDateTime) -> i64 {
    let last_sunday = |month: u32, hour: u32| {
        let d = NaiveDate::from_ymd_opt(local.year(), month, 31).expect("March and October have 31 days");
        (d - Duration::days(d.weekday().num_days_from_sunday() as i64)).and_hms_opt(hour, 0, 0).expect("valid hour")
    };
    if last_sunday(3, 2) <= local && local < last_sunday(10, 3) { 2 } else { 1 }
}

fn to_utc(t: NaiveDateTime, kind: Kind) -> DateTime<Utc> {
    match kind {
        Kind::Utc => t.and_utc(),
        _ => (t - Duration::hours(cet_offset(t))).and_utc(),
    }
}

/// The same day and time in another month; None when that month has no such day.
fn same_day(t: NaiveDateTime, year: i32, month: u32) -> Option<NaiveDateTime> {
    NaiveDate::from_ymd_opt(year, month, t.day()).map(|d| d.and_time(t.time()))
}

/// The start times of a repeating event, in its own wall-clock time, up to `hi`. Handles DAILY,
/// WEEKLY (with BYDAY), MONTHLY and YEARLY on the start's day, with INTERVAL, COUNT and UNTIL.
/// Any other rule gives the first start only, rather than a wrong guess.
fn series(start: NaiveDateTime, rule: &str, hi: NaiveDateTime) -> Vec<NaiveDateTime> {
    let r: HashMap<&str, &str> = rule.split(';').filter_map(|p| p.split_once('=')).collect();
    let (freq, byday) = (r.get("FREQ").copied().unwrap_or_default(), r.get("BYDAY").copied());
    let step: i64 = r.get("INTERVAL").and_then(|s| s.parse().ok()).unwrap_or(1);
    let count: Option<usize> = r.get("COUNT").and_then(|s| s.parse().ok());
    let until = r.get("UNTIL").and_then(|u| when(&Params::new(), u).ok()).map(|(t, _)| t);
    let known = r.keys().all(|k| ["FREQ", "INTERVAL", "COUNT", "UNTIL", "WKST", "BYDAY"].contains(k));
    let byday_ok = byday.is_none_or(|b| freq == "WEEKLY" && b.split(',').all(|d| DAYS.contains(&d)));
    if !known || !["DAILY", "WEEKLY", "MONTHLY", "YEARLY"].contains(&freq) || !byday_ok {
        return vec![start];
    }
    let mut days: Vec<i64> = match byday {
        Some(b) => b.split(',').filter_map(|d| DAYS.iter().position(|x| *x == d)).map(|i| i as i64).collect(),
        None => vec![start.weekday().num_days_from_monday() as i64],
    };
    days.sort();
    let mut out = vec![];
    for i in 0..5000i64 {
        let batch: Vec<Option<NaiveDateTime>> = match freq {
            "DAILY" => vec![Some(start + Duration::days(i * step))],
            "WEEKLY" => {
                let monday = start - Duration::days(start.weekday().num_days_from_monday() as i64) + Duration::weeks(i * step);
                days.iter().map(|d| monday + Duration::days(*d)).filter(|t| *t >= start).map(Some).collect()
            }
            "MONTHLY" => {
                let m = start.month0() as i64 + i * step;
                vec![same_day(start, start.year() + (m / 12) as i32, (m % 12 + 1) as u32)]
            }
            _ => vec![same_day(start, start.year() + (i * step) as i32, start.month())],
        };
        for t in batch.into_iter().flatten() {
            if until.is_some_and(|u| t > u) || count.is_some_and(|c| out.len() >= c) || t > hi {
                return out;
            }
            out.push(t);
        }
    }
    out
}

/// (starts_at, ends_at) in UTC, as the app stores them: an all-day event ends at the start of its
/// last day, and iCal's all-day end is the day after.
fn span(start: NaiveDateTime, end: NaiveDateTime, kind: Kind) -> (DateTime<Utc>, DateTime<Utc>) {
    if kind == Kind::Date {
        let last = if end > start { end - Duration::days(1) } else { start };
        return (to_utc(start, kind), to_utc(last, kind));
    }
    (to_utc(start, kind), to_utc(end, kind))
}

/// What the shared calendar should hold from lo to hi, by key. The key is the UID, plus the
/// original start for one time of a repeating event.
fn wanted(events: &[Props], lo: DateTime<Utc>, hi: DateTime<Utc>) -> BTreeMap<String, Want> {
    let mut out = BTreeMap::new();
    let hi_naive = hi.naive_utc() + Duration::hours(2);
    // Whole series first, then the single times that were moved or cancelled within them.
    let mut sorted: Vec<&Props> = events.iter().collect();
    sorted.sort_by_key(|e| e.contains_key("RECURRENCE-ID"));
    for ev in sorted {
        let text = |name: &str| prop(ev, name).map(|(_, v)| v.as_str());
        let title: String = Some(unescape(text("SUMMARY").unwrap_or_default()).trim().to_string()).filter(|t| !t.is_empty()).unwrap_or("(no title)".into()).chars().take(200).collect();
        let uid = text("UID").map(str::to_string).unwrap_or_else(|| title.clone());
        let found = (|| -> Result<(Vec<String>, Vec<NaiveDateTime>, NaiveDateTime, NaiveDateTime, Kind), String> {
            let (p, v) = prop(ev, "DTSTART").filter(|(_, v)| !v.is_empty()).ok_or("it has no start")?;
            let (start, kind) = when(p, v)?;
            let end = match prop(ev, "DTEND") {
                Some((p, v)) => when(p, v)?.0,
                None => start + duration(text("DURATION"), kind)?,
            };
            if let Some((p, v)) = prop(ev, "RECURRENCE-ID") {
                return Ok((vec![format!("{uid}/{}", when(p, v)?.0.format(STAMP))], vec![start], start, end, kind));
            }
            if let Some(rule) = text("RRULE") {
                let mut skip = vec![];
                for (p, vals) in ev.get("EXDATE").into_iter().flatten() {
                    for v in vals.split(',') {
                        skip.push(when(p, v)?.0);
                    }
                }
                let starts: Vec<NaiveDateTime> = series(start, rule, hi_naive).into_iter().filter(|t| !skip.contains(t)).collect();
                return Ok((starts.iter().map(|t| format!("{uid}/{}", t.format(STAMP))).collect(), starts, start, end, kind));
            }
            Ok((vec![uid.clone()], vec![start], start, end, kind))
        })();
        let Ok((keys, starts, start, end, kind)) = found else { continue }; // ponytail: skipped events aren't reported
        if prop(ev, "RECURRENCE-ID").is_some() {
            out.remove(&keys[0]);
        }
        let private = matches!(text("CLASS"), Some("PRIVATE" | "CONFIDENTIAL"));
        if private || text("STATUS") == Some("CANCELLED") {
            continue;
        }
        for (key, t) in keys.into_iter().zip(starts) {
            let (s, e) = span(t, t + (end - start), kind);
            if lo <= s && s < hi {
                out.insert(key, (title.clone(), s, e, kind == Kind::Date));
            }
        }
    }
    out
}

// ---- The shared calendar ----

#[derive(Deserialize, Clone, Debug)]
struct Row {
    id: String,
    title: String,
    starts_at: String,
    ends_at: String,
    all_day: bool,
    #[serde(default)]
    note: String,
}

fn row_want(r: &Row) -> Option<Want> {
    let t = |s: &str| DateTime::parse_from_rfc3339(s).ok().map(|t| t.to_utc());
    Some((r.title.clone(), t(&r.starts_at)?, t(&r.ends_at)?, r.all_day))
}

struct Plan<'a> {
    adds: Vec<(&'a String, &'a Want)>,
    updates: Vec<(&'a Row, &'a Want)>,
    removes: Vec<&'a Row>,
}

/// What to add, update, and remove. A copy is only removed while it's still ahead: something
/// already started is history.
fn plan<'a>(want: &'a BTreeMap<String, Want>, have: &'a BTreeMap<String, Row>, now: DateTime<Utc>) -> Plan<'a> {
    Plan {
        adds: want.iter().filter(|(k, _)| !have.contains_key(*k)).collect(),
        updates: want.iter().filter_map(|(k, w)| have.get(k).filter(|r| row_want(r).as_ref() != Some(w)).map(|r| (r, w))).collect(),
        removes: have.iter().filter(|(k, r)| !want.contains_key(*k) && row_want(r).is_some_and(|w| w.1 >= now)).map(|(_, r)| r).collect(),
    }
}

fn stamp(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn fields(w: &Want) -> Value {
    json!({ "title": w.0, "starts_at": stamp(w.1), "ends_at": stamp(w.2), "all_day": w.3 })
}

fn download(n: usize, link: &str) -> Result<String, String> {
    let link = link.trim();
    let url = match link.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => link.to_string(),
    };
    if !url.starts_with("https://") {
        return Err(format!("Calendar address {n} should start with https:// or webcal://. Copy the secret address in iCal format again."));
    }
    // Errors never include the address: it holds a private token.
    let r = ureq::get(&url).timeout(std::time::Duration::from_secs(30)).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("Google refused calendar {n} (HTTP {code}). Copy its secret address in iCal format again."),
        ureq::Error::Transport(t) => format!("Couldn't reach Google for calendar {n} ({}). Nothing was changed.", t.kind()),
    })?;
    r.into_string().map_err(|e| format!("Couldn't read calendar {n}: {e}. Nothing was changed."))
}

// ---- The calendars you track ----
// The addresses are one secret (`gcal_feed_urls`, separated by spaces). Settings only ever sees a
// label for each: the calendar's id from the address, never its private token.

fn feeds() -> Vec<String> {
    crate::settings::secret("gcal_feed_urls").map(|s| s.split_whitespace().map(str::to_string).collect()).unwrap_or_default()
}

/// "someone@gmail.com" for https://calendar.google.com/calendar/ical/someone%40gmail.com/private-…/basic.ics.
pub fn label(url: &str) -> String {
    let id = url.split("/ical/").nth(1).and_then(|rest| rest.split('/').next()).filter(|id| !id.is_empty());
    match id {
        Some(id) => id.replace("%40", "@"),
        None => url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or("A calendar").to_string(),
    }
}

pub fn labels() -> Vec<String> {
    feeds().iter().map(|u| label(u)).collect()
}

/// Adds calendar addresses to the ones you track. Returns the labels of them all.
pub fn add(urls: &[String]) -> Result<Vec<String>, String> {
    let mut all = feeds();
    for (n, url) in urls.iter().map(|u| u.trim()).filter(|u| !u.is_empty()).enumerate() {
        if !(url.starts_with("https://") || url.starts_with("webcal://")) || url.contains(char::is_whitespace) {
            return Err(format!("Address {} should start with https:// or webcal://. Copy the Secret address in iCal format again.", n + 1));
        }
        if !all.iter().any(|u| u == url) {
            all.push(url.to_string());
        }
    }
    crate::settings::keep("gcal_feed_urls", &all.join(" "))?;
    Ok(labels())
}

/// Stops tracking one calendar. Returns the labels of the rest.
pub fn remove(index: usize) -> Result<Vec<String>, String> {
    let mut all = feeds();
    if index >= all.len() {
        return Err("That calendar is already gone.".into());
    }
    all.remove(index);
    if all.is_empty() {
        crate::settings::clear_secret("gcal_feed_urls")?;
    } else {
        crate::settings::keep("gcal_feed_urls", &all.join(" "))?;
    }
    Ok(labels())
}

pub struct Synced {
    pub found: usize,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
}

/// Reads every feed (addresses separated by spaces or lines) and makes your copies match it.
pub fn sync(conn: &Connection, feeds: &str) -> Result<Synced, String> {
    let p = ready(conn).map_err(|_| "Sign in to the shared calendar first, in its card below.".to_string())?;
    let me = me(conn).ok_or("Sign in to the shared calendar first, in its card below.")?;
    let mut events = vec![];
    for (n, link) in feeds.split_whitespace().enumerate() {
        let got = parse_ics(&download(n + 1, link)?);
        // Every calendar must answer, or its events would look cancelled and be removed.
        if got.is_empty() {
            return Err(format!("Calendar {} has no events, so nothing was changed: that would look like everything in it was cancelled. Check its address.", n + 1));
        }
        events.extend(got);
    }
    let now = Utc::now();
    let lo = to_utc(Local::now().date_naive().and_hms_opt(0, 0, 0).expect("midnight"), Kind::Local);
    let hi = now + Duration::weeks(WEEKS);
    let want = wanted(&events, lo, hi);

    let query = [
        ("select", "id,title,starts_at,ends_at,all_day,note"),
        ("created_by", &format!("eq.{me}")),
        ("note", &format!("like.*{MARK}*")),
        ("starts_at", &format!("gte.{}", stamp(lo))),
        ("starts_at", &format!("lt.{}", stamp(hi))),
    ];
    let rows: Vec<Row> = from_json(rest(conn, &p, "GET", "/rest/v1/events", &query, None, None)?)?;
    let have: BTreeMap<String, Row> = rows
        .into_iter()
        .filter_map(|r| {
            let key = r.note.split(MARK).nth(1)?.split_whitespace().next()?.trim_end_matches(')').to_string();
            Some((key, r))
        })
        .collect();
    let plan = plan(&want, &have, now);

    if !plan.adds.is_empty() {
        let body: Vec<Value> = plan
            .adds
            .iter()
            .map(|(k, w)| {
                let mut f = fields(w);
                f["note"] = json!(format!("Copied from Google Calendar ({MARK}{k})"));
                f
            })
            .collect();
        rest(conn, &p, "POST", "/rest/v1/events", &[], Some("return=minimal"), Some(Value::Array(body)))?;
    }
    for (row, w) in &plan.updates {
        rest(conn, &p, "PATCH", "/rest/v1/events", &[("id", &format!("eq.{}", row.id))], Some("return=minimal"), Some(fields(w)))?;
    }
    if !plan.removes.is_empty() {
        let ids = plan.removes.iter().map(|r| r.id.as_str()).collect::<Vec<_>>().join(",");
        rest(conn, &p, "DELETE", "/rest/v1/events", &[("id", &format!("in.({ids})"))], Some("return=minimal"), None)?;
    }
    Ok(Synced { found: want.len(), added: plan.adds.len(), updated: plan.updates.len(), removed: plan.removes.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn z(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("{s}Z")).unwrap().to_utc()
    }
    fn naive(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    // The same cases the removed sync_gcal.py checked with --selftest.
    const ICS: &str = "BEGIN:VCALENDAR\r\n\
BEGIN:VEVENT\r\nUID:one\r\nSUMMARY:Tandlæge\\, Vesterbro\r\nDTSTART;TZID=Europe/Copenhagen:20261028T100000\r\nDTEND;TZID=Europe/Copenhagen:20261028T110000\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:utc\r\nSUMMARY:Call\r\nDTSTART:20261102T120000Z\r\nDURATION:PT30M\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:trip\r\nSUMMARY:Aarhus\r\nDTSTART;VALUE=DATE:20261030\r\nDTEND;VALUE=DATE:20261101\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:w\r\nSUMMARY:Træning\r\nDTSTART;TZID=Europe/Copenhagen:20261026T080000\r\nDTEND;TZID=Europe/Copenhagen:20261026T090000\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=4\r\nEXDATE;TZID=Europe/Copenhagen:20261028T080000\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:w\r\nSUMMARY:Træning, flyttet\r\nRECURRENCE-ID;TZID=Europe/Copenhagen:20261102T080000\r\nDTSTART;TZID=Europe/Copenhagen:20261102T090000\r\nDTEND;TZID=Europe/Copenhagen:20261102T100000\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:secret\r\nSUMMARY:Læge\r\nCLASS:PRIVATE\r\nDTSTART:20261029T090000Z\r\nDTEND:20261029T100000Z\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:old\r\nSUMMARY:Last week\r\nDTSTART:20261019T090000Z\r\nEND:VEVENT\r\n\
END:VCALENDAR";

    #[test]
    fn a_calendar_shows_as_its_id_never_its_token() {
        assert_eq!(label("https://calendar.google.com/calendar/ical/someone%40gmail.com/private-abc123/basic.ics"), "someone@gmail.com");
        assert_eq!(label("webcal://example.com/feed.ics"), "example.com");
    }

    #[test]
    fn reads_folded_lines_and_copenhagen_time_across_the_clock_change() {
        assert_eq!(parse_ics("BEGIN:VEVENT\r\nSUMMARY:Lang\r\n  titel\r\nEND:VEVENT")[0]["SUMMARY"][0].1, "Lang titel");
        // The clocks went back on 25 Oct 2026 at 03:00 CEST.
        assert_eq!((cet_offset(naive("2026-10-25T01:00:00")), cet_offset(naive("2026-10-25T03:00:00"))), (2, 1));
        assert_eq!(cet_offset(naive("2026-03-29T03:00:00")), 2);
    }

    #[test]
    fn expands_repeats_skips_private_and_keeps_moved_times() {
        let want = wanted(&parse_ics(ICS), z("2026-10-25T00:00:00"), z("2027-01-01T00:00:00"));
        assert_eq!(want["one"], ("Tandlæge, Vesterbro".into(), z("2026-10-28T09:00:00"), z("2026-10-28T10:00:00"), false));
        assert_eq!((want["utc"].1, want["utc"].2), (z("2026-11-02T12:00:00"), z("2026-11-02T12:30:00")));
        assert_eq!(want["trip"], ("Aarhus".into(), z("2026-10-29T23:00:00"), z("2026-10-30T23:00:00"), true));
        // 28 Oct is excluded but still counts towards COUNT=4, so the series ends on 4 Nov.
        let weekly: Vec<&String> = want.keys().filter(|k| k.starts_with("w/")).collect();
        assert_eq!(weekly, ["w/20261026T080000", "w/20261102T080000", "w/20261104T080000"]);
        assert_eq!(want["w/20261026T080000"].1, z("2026-10-26T07:00:00"));
        // 2 Nov was moved an hour later and renamed; it keeps its key, so its copy is updated, not doubled.
        assert_eq!((want["w/20261102T080000"].0.as_str(), want["w/20261102T080000"].1), ("Træning, flyttet", z("2026-11-02T08:00:00")));
        assert!(!want.contains_key("secret") && !want.contains_key("old"));

        let row = |id: &str, w: &Want| Row { id: id.into(), title: w.0.clone(), starts_at: stamp(w.1), ends_at: stamp(w.2), all_day: w.3, note: String::new() };
        let mut renamed = want["utc"].clone();
        renamed.0 = "Old title".into();
        let have: BTreeMap<String, Row> = [
            ("one", row("a", &want["one"])),
            ("utc", row("b", &renamed)),
            ("gone", row("c", &("Cancelled".into(), z("2026-11-10T09:00:00"), z("2026-11-10T10:00:00"), false))),
            ("w/20261026T080000", row("d", &want["w/20261026T080000"])),
            ("past", row("e", &("Already started".into(), z("2026-10-27T11:00:00"), z("2026-10-27T13:00:00"), false))),
        ]
        .into_iter()
        .map(|(k, r)| (k.to_string(), r))
        .collect();
        let p = plan(&want, &have, z("2026-10-27T12:00:00"));
        assert_eq!(p.adds.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["trip", "w/20261102T080000", "w/20261104T080000"]);
        assert_eq!(p.updates.iter().map(|(r, _)| r.id.as_str()).collect::<Vec<_>>(), ["b"]);
        assert_eq!(p.removes.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["c"]);
    }
}
