//! The shared calendar in the owner's Supabase project (supabase/schema.sql): signing in, copying
//! everyone's events into SQLite, and adding, changing, and deleting events. Only this module
//! talks to Supabase, with the publishable key and the member's own sign-in.
//!
//! The refresh token is a secret in the system's password store; the access token lives only in
//! memory. Supabase's rules decide what a member may do, and its triggers log every change.
use crate::db::{get_setting, set_setting};
use crate::settings;
use chrono::{DateTime, Duration, Local, NaiveDate, SecondsFormat, TimeZone, Utc};
use rusqlite::{params, Connection};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;

/// The secret that keeps you signed in.
const REFRESH: &str = "calendar_refresh_token";
/// The copy on this computer covers these days, so the Calendar view can page through them.
const DAYS_BACK: i64 = 28;
const DAYS_AHEAD: i64 = 182;

pub const OFFLINE: &str = "Couldn't reach the shared calendar. Check the internet connection and the Project URL.";
const WRONG_SIGN_IN: &str = "Wrong email or password. If you haven't been added yet, ask the calendar's owner to add you.";
const SIGNED_OUT: &str = "You're signed out of the shared calendar. Sign in again in Settings.";

struct Session {
    access: String,
    expires: DateTime<Utc>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

pub(crate) struct Project {
    url: String,
    key: String,
}

fn project(conn: &Connection) -> Option<Project> {
    let (url, key) = (settings::plain(conn, "calendar_url"), settings::plain(conn, "calendar_key"));
    (!url.is_empty() && !key.is_empty()).then_some(Project { url, key })
}

/// The signed-in member's id. Set at sign-in and removed at sign-out.
pub(crate) fn me(conn: &Connection) -> Option<String> {
    get_setting(conn, "calendar.user_id").ok().flatten()
}

pub(crate) fn ready(conn: &Connection) -> Result<Project, String> {
    let p = project(conn).ok_or("Add the Project URL and the publishable key in Settings first.")?;
    me(conn).ok_or(SIGNED_OUT)?;
    Ok(p)
}

pub(crate) fn db_err(e: rusqlite::Error) -> String {
    format!("Couldn't save the calendar on this computer: {e}")
}

pub(crate) fn from_json<T: DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("Supabase sent something unexpected: {e}"))
}

fn stamp(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

// ---- Talking to Supabase ----

/// What went wrong: no answer at all, or an answer that isn't a success.
#[derive(Debug)]
enum Fail {
    Offline,
    Answer(u16, Value),
}

fn call(
    p: &Project,
    method: &str,
    path: &str,
    query: &[(&str, &str)],
    token: Option<&str>,
    prefer: Option<&str>,
    body: Option<Value>,
) -> Result<Value, Fail> {
    // The tables live in the todo_helper schema (supabase/schema.sql): Accept-Profile picks it for
    // reads, Content-Profile for writes.
    let mut req = ureq::request(method, &format!("{}{path}", p.url))
        .timeout(std::time::Duration::from_secs(20))
        .set("apikey", &p.key)
        .set("Accept-Profile", "todo_helper")
        .set("Content-Profile", "todo_helper");
    for (k, v) in query {
        req = req.query(k, v);
    }
    if let Some(t) = token {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    if let Some(pr) = prefer {
        req = req.set("Prefer", pr);
    }
    let reply = match body {
        Some(b) => req.send_json(b),
        None => req.call(),
    };
    match reply {
        // A delete or a minimal write answers with no body.
        Ok(r) => Ok(r.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)),
        Err(ureq::Error::Status(code, r)) => Err(Fail::Answer(code, r.into_json().unwrap_or(Value::Null))),
        Err(ureq::Error::Transport(_)) => Err(Fail::Offline),
    }
}

/// The message the person sees, with what to do about it.
fn describe(f: Fail) -> String {
    let (status, v) = match f {
        Fail::Offline => return OFFLINE.into(),
        Fail::Answer(status, v) => (status, v),
    };
    // Auth answers carry `error_code`; the REST API carries a string `code` such as "42501".
    let code = v["error_code"].as_str().or(v["code"].as_str()).unwrap_or_default();
    let text = ["msg", "message", "error_description", "error"].iter().find_map(|k| v[*k].as_str()).unwrap_or("no details");
    match (status, code) {
        (_, "invalid_credentials") => WRONG_SIGN_IN.into(),
        (_, "weak_password" | "same_password") => text.into(),
        _ if text.contains("API key") => "Supabase didn't accept the publishable key. Copy it again from Project Settings, API Keys.".into(),
        (_, "42501") => "Supabase refused. Check the publishable key in Settings, and that supabase/schema.sql has been run.".into(),
        (401, _) | (_, "PGRST301" | "PGRST303") => SIGNED_OUT.into(),
        (404, _) | (_, "PGRST205" | "42P01") => {
            "The calendar's tables are missing. Run supabase/schema.sql in the project (SETUP.md, section 7).".into()
        }
        (429, _) | (_, "over_request_rate_limit") => "Too many tries in a short time. Wait a minute, then try again.".into(),
        _ => format!("Supabase said: {text} ({status})."),
    }
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    user: User,
}

#[derive(Deserialize)]
struct User {
    id: String,
    email: Option<String>,
}

fn parse_tokens(v: Value) -> Result<Tokens, String> {
    serde_json::from_value(v).map_err(|e| format!("Supabase's sign-in reply didn't read as expected: {e}"))
}

/// An access token is renewed a minute before it runs out.
fn expiring(expires: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    expires - now < Duration::seconds(60)
}

fn remember(t: &Tokens, session: &mut Option<Session>) -> Result<(), String> {
    settings::keep(REFRESH, &t.refresh_token)?;
    *session = Some(Session { access: t.access_token.clone(), expires: Utc::now() + Duration::seconds(t.expires_in) });
    Ok(())
}

/// The signed-in member's access token, renewed with the refresh token when it runs out or `renew`
/// is set. The lock is held while renewing, so two threads never spend the same refresh token.
fn access(conn: &Connection, p: &Project, renew: bool) -> Result<String, String> {
    let mut session = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = session.as_ref().filter(|s| !renew && !expiring(s.expires, Utc::now())) {
        return Ok(s.access.clone());
    }
    let refresh = settings::secret(REFRESH).ok_or(SIGNED_OUT)?;
    match call(p, "POST", "/auth/v1/token", &[("grant_type", "refresh_token")], None, None, Some(json!({ "refresh_token": refresh }))) {
        Ok(v) => {
            let t = parse_tokens(v)?;
            remember(&t, &mut session)?;
            Ok(t.access_token)
        }
        // Supabase no longer knows this sign-in: signed out elsewhere, or the member was removed.
        Err(Fail::Answer(code, _)) if (400..500).contains(&code) && code != 429 => {
            *session = None;
            drop(session);
            forget_sign_in(conn)?;
            Err(SIGNED_OUT.into())
        }
        Err(f) => Err(describe(f)),
    }
}

/// A call as the signed-in member. An access token Supabase turns down is renewed once.
pub(crate) fn rest(conn: &Connection, p: &Project, method: &str, path: &str, query: &[(&str, &str)], prefer: Option<&str>, body: Option<Value>) -> Result<Value, String> {
    let token = access(conn, p, false)?;
    match call(p, method, path, query, Some(&token), prefer, body.clone()) {
        Err(Fail::Answer(401, _)) => {
            let token = access(conn, p, true)?;
            call(p, method, path, query, Some(&token), prefer, body).map_err(describe)
        }
        r => r.map_err(describe),
    }
}

// ---- Signing in ----

pub fn sign_in(conn: &Connection, email: &str, password: &str) -> Result<(), String> {
    let p = project(conn).ok_or("Add the Project URL and the publishable key first.")?;
    let email = email.trim().to_lowercase();
    if email.is_empty() || password.is_empty() {
        return Err("Type your email and password.".into());
    }
    let v = call(&p, "POST", "/auth/v1/token", &[("grant_type", "password")], None, None, Some(json!({ "email": email, "password": password })))
        .map_err(describe)?;
    let t = parse_tokens(v)?;
    // The copy belongs to whoever was signed in before; the next sync fills it again.
    if me(conn).is_some_and(|id| id != t.user.id) {
        clear_copy(conn)?;
    }
    remember(&t, &mut SESSION.lock().unwrap_or_else(|e| e.into_inner()))?;
    set_setting(conn, "calendar.user_id", &t.user.id).map_err(db_err)?;
    set_setting(conn, "calendar.email", t.user.email.as_deref().unwrap_or(&email)).map_err(db_err)
}

/// Ends the sign-in on this computer only, and removes this computer's copy of the calendar.
pub fn sign_out(conn: &Connection) -> Result<(), String> {
    let token = SESSION.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.access.clone());
    if let (Some(p), Some(token)) = (project(conn), token) {
        let _ = call(&p, "POST", "/auth/v1/logout", &[("scope", "local")], Some(&token), None, None);
    }
    forget_sign_in(conn)
}

fn forget_sign_in(conn: &Connection) -> Result<(), String> {
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = None;
    settings::forget(REFRESH)?;
    for key in ["calendar.user_id", "calendar.email", "calendar.synced_at", "calendar.fingerprint"] {
        conn.execute("DELETE FROM settings WHERE key = ?1", [key]).map_err(db_err)?;
    }
    clear_copy(conn)
}

fn clear_copy(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("DELETE FROM shared_events; DELETE FROM shared_members; DELETE FROM shared_log; DELETE FROM shared_tasks;").map_err(db_err)
}

/// A name the others see: 1 to 40 characters.
pub fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    match name.chars().count() {
        0 => Err("Type the name the others should see.".into()),
        1..=40 => Ok(name.to_string()),
        _ => Err("Keep the name to 40 characters.".into()),
    }
}

pub fn set_name(conn: &Connection, name: &str) -> Result<(), String> {
    let name = clean_name(name)?;
    let p = ready(conn)?;
    let id = me(conn).ok_or(SIGNED_OUT)?;
    let body = json!({ "id": id, "name": name });
    rest(conn, &p, "POST", "/rest/v1/profiles", &[("on_conflict", "id")], Some("resolution=merge-duplicates,return=minimal"), Some(body))?;
    conn.execute("INSERT OR REPLACE INTO shared_members (id, name) VALUES (?1, ?2)", params![id, name]).map_err(db_err)?;
    Ok(())
}

pub fn change_password(conn: &Connection, password: &str) -> Result<(), String> {
    if password.chars().count() < 8 {
        return Err("Use at least 8 characters.".into());
    }
    let p = ready(conn)?;
    rest(conn, &p, "PUT", "/auth/v1/user", &[], None, Some(json!({ "password": password })))?;
    Ok(())
}

// ---- The copy on this computer ----

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash)]
pub struct Event {
    pub id: String,
    pub title: String,
    pub starts_at: String,
    /// For an all-day event, the start of its last day.
    pub ends_at: String,
    pub all_day: bool,
    pub note: String,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_by: Option<String>,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash)]
pub struct Member {
    pub id: String,
    pub name: String,
}

/// One line of the Activity list.
#[derive(Serialize, Debug, PartialEq, Hash)]
pub struct Change {
    pub id: i64,
    pub event_id: String,
    /// "added", "changed", or "deleted".
    pub action: String,
    pub by_user: Option<String>,
    pub at: String,
    /// The event after the change, or as it was when it was deleted.
    pub title: String,
    pub starts_at: Option<String>,
    pub all_day: bool,
    /// For a change: the title and start before it.
    pub old_title: Option<String>,
    pub old_starts_at: Option<String>,
}

/// Reads one row of Supabase's event_log.
pub fn parse_change(row: &Value) -> Option<Change> {
    let (after, before) = (&row["after"], &row["before"]);
    let now = if after.is_object() { after } else { before };
    let changed = after.is_object() && before.is_object();
    let text = |v: &Value| v.as_str().map(str::to_string);
    Some(Change {
        id: row["id"].as_i64()?,
        event_id: text(&row["event_id"])?,
        action: text(&row["action"])?,
        by_user: text(&row["by_user"]),
        at: text(&row["at"])?,
        title: text(&now["title"]).unwrap_or_else(|| "an event".into()),
        starts_at: text(&now["starts_at"]),
        all_day: now["all_day"].as_bool().unwrap_or(false),
        old_title: if changed { text(&before["title"]) } else { None },
        old_starts_at: if changed { text(&before["starts_at"]) } else { None },
    })
}

fn put_event(conn: &Connection, e: &Event) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT OR REPLACE INTO shared_events (id, title, starts_at, ends_at, all_day, note, created_by, created_at, updated_by, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![e.id, e.title, e.starts_at, e.ends_at, e.all_day, e.note, e.created_by, e.created_at, e.updated_by, e.updated_at],
    )
}

/// Replaces the copy with what Supabase sent. Returns false, and writes nothing, when nothing changed.
pub fn store(conn: &Connection, members: &[Member], events: &[Event], changes: &[Change]) -> Result<bool, String> {
    let mut h = DefaultHasher::new();
    (members, events, changes).hash(&mut h);
    let fingerprint = h.finish().to_string();
    if get_setting(conn, "calendar.fingerprint").map_err(db_err)?.as_deref() == Some(fingerprint.as_str()) {
        return Ok(false);
    }
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    clear_copy(&tx)?;
    for m in members {
        tx.execute("INSERT INTO shared_members (id, name) VALUES (?1, ?2)", params![m.id, m.name]).map_err(db_err)?;
    }
    for e in events {
        put_event(&tx, e).map_err(db_err)?;
    }
    for c in changes {
        tx.execute(
            "INSERT INTO shared_log (id, event_id, action, by_user, at, title, starts_at, all_day, old_title, old_starts_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![c.id, c.event_id, c.action, c.by_user, c.at, c.title, c.starts_at, c.all_day, c.old_title, c.old_starts_at],
        )
        .map_err(db_err)?;
    }
    set_setting(&tx, "calendar.fingerprint", &fingerprint).map_err(db_err)?;
    tx.commit().map_err(db_err)?;
    Ok(true)
}

pub struct Synced {
    pub events: usize,
    pub members: usize,
    pub changed: bool,
}

/// Copies the members, everyone's events in the window, and the newest 50 changes into SQLite.
/// Ok(None) when the calendar isn't set up or nobody is signed in.
pub fn sync(conn: &Connection) -> Result<Option<Synced>, String> {
    let (Some(p), Some(_)) = (project(conn), me(conn)) else { return Ok(None) };
    let now = Utc::now();
    let (from, to) = (stamp(now - Duration::days(DAYS_BACK)), stamp(now + Duration::days(DAYS_AHEAD)));
    let members: Vec<Member> = from_json(rest(conn, &p, "GET", "/rest/v1/profiles", &[("select", "id,name")], None, None)?)?;
    let window = [("select", "*"), ("starts_at", &format!("lt.{to}")), ("ends_at", &format!("gte.{from}")), ("order", "starts_at")];
    let events: Vec<Event> = from_json(rest(conn, &p, "GET", "/rest/v1/events", &window, None, None)?)?;
    let log = rest(conn, &p, "GET", "/rest/v1/event_log", &[("select", "*"), ("order", "id.desc"), ("limit", "50")], None, None)?;
    let changes: Vec<Change> = log.as_array().into_iter().flatten().filter_map(parse_change).collect();
    let changed = store(conn, &members, &events, &changes)?;
    // Tasks ride along. A project without the tasks table yet (schema.sql not run again) keeps the calendar working.
    let tasks_changed = crate::tasks::sync(conn, &p).unwrap_or(false);
    let changed = changed || tasks_changed;
    set_setting(conn, "calendar.synced_at", &stamp(now)).map_err(db_err)?;
    Ok(Some(Synced { events: events.len(), members: members.len(), changed }))
}

/// Everything the Calendar view and the board need, read from the copy.
#[derive(Serialize)]
pub struct State {
    /// "not_set_up", "signed_out", "needs_name", or "ready".
    pub status: &'static str,
    pub me: Option<String>,
    pub email: Option<String>,
    pub members: Vec<Member>,
    /// Members whose events you turned off in the legend.
    pub hidden: Vec<String>,
    pub events: Vec<Event>,
    pub log: Vec<Change>,
    /// Every member's tasks, for the Given list and a task card's "Fra" line.
    pub tasks: Vec<crate::tasks::Task>,
    pub synced_at: Option<String>,
    /// Why the last sync failed, when it did. The copy then shows the calendar as of `synced_at`.
    pub problem: Option<String>,
    /// The days the copy covers (YYYY-MM-DD), so the view knows how far it can page.
    pub from: String,
    pub to: String,
}

pub fn state(conn: &Connection) -> rusqlite::Result<State> {
    let me = me(conn);
    let members: Vec<Member> = conn
        .prepare("SELECT id, name FROM shared_members ORDER BY name COLLATE NOCASE")?
        .query_map([], |r| Ok(Member { id: r.get(0)?, name: r.get(1)? }))?
        .collect::<rusqlite::Result<_>>()?;
    let events = conn
        .prepare("SELECT id, title, starts_at, ends_at, all_day, note, created_by, created_at, updated_by, updated_at FROM shared_events ORDER BY starts_at")?
        .query_map([], |r| {
            Ok(Event {
                id: r.get(0)?,
                title: r.get(1)?,
                starts_at: r.get(2)?,
                ends_at: r.get(3)?,
                all_day: r.get(4)?,
                note: r.get(5)?,
                created_by: r.get(6)?,
                created_at: r.get(7)?,
                updated_by: r.get(8)?,
                updated_at: r.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let log = conn
        .prepare("SELECT id, event_id, action, by_user, at, title, starts_at, all_day, old_title, old_starts_at FROM shared_log ORDER BY id DESC")?
        .query_map([], |r| {
            Ok(Change {
                id: r.get(0)?,
                event_id: r.get(1)?,
                action: r.get(2)?,
                by_user: r.get(3)?,
                at: r.get(4)?,
                title: r.get(5)?,
                starts_at: r.get(6)?,
                all_day: r.get(7)?,
                old_title: r.get(8)?,
                old_starts_at: r.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let status = match (&me, project(conn)) {
        (_, None) => "not_set_up",
        (None, _) => "signed_out",
        (Some(id), _) if !members.iter().any(|m| &m.id == id) => "needs_name",
        _ => "ready",
    };
    let hidden = get_setting(conn, "calendar.hidden")?.unwrap_or_default();
    let last = get_setting(conn, "status.calendar")?.and_then(|v| serde_json::from_str::<settings::Status>(&v).ok());
    let today = Local::now().date_naive();
    Ok(State {
        status,
        me,
        email: get_setting(conn, "calendar.email")?,
        members,
        hidden: hidden.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect(),
        events,
        log,
        tasks: crate::tasks::list(conn)?,
        synced_at: get_setting(conn, "calendar.synced_at")?,
        problem: last.filter(|s| !s.ok).map(|s| s.message),
        from: (today - Duration::days(DAYS_BACK)).to_string(),
        to: (today + Duration::days(DAYS_AHEAD)).to_string(),
    })
}

pub fn set_hidden(conn: &Connection, ids: &[String]) -> rusqlite::Result<()> {
    set_setting(conn, "calendar.hidden", &ids.join(","))
}

// ---- Adding, changing, and deleting events ----

#[derive(Deserialize, Debug)]
pub struct Draft {
    /// None for a new event.
    pub id: Option<String>,
    pub title: String,
    pub starts_at: String,
    pub ends_at: String,
    pub all_day: bool,
    pub note: String,
    /// The version the form opened, so a save never overwrites someone else's newer change.
    pub updated_at: Option<String>,
}

/// Checks a draft and returns what to send. Supabase checks the same limits again.
pub fn check_draft(d: &Draft) -> Result<Value, String> {
    let title = d.title.trim();
    if title.is_empty() {
        return Err("An event needs a title.".into());
    }
    if title.chars().count() > 200 {
        return Err("Keep the title to 200 characters.".into());
    }
    let time = |s: &str, what: &str| {
        DateTime::parse_from_rfc3339(s).map(|t| t.to_utc()).map_err(|_| format!("The {what} isn't a date and time the app understands."))
    };
    let (start, end) = (time(&d.starts_at, "start")?, time(&d.ends_at, "end")?);
    if end < start {
        return Err("The end is before the start.".into());
    }
    if d.note.chars().count() > 4000 {
        return Err("Keep the note to 4,000 characters.".into());
    }
    Ok(json!({ "title": title, "starts_at": stamp(start), "ends_at": stamp(end), "all_day": d.all_day, "note": d.note }))
}

#[derive(Serialize, Debug)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Saved {
    Saved { event: Event },
    /// Someone else saved the event after the form opened. `event` is their version, or None if they deleted it.
    Conflict { event: Option<Event> },
}

pub fn save(conn: &Connection, d: &Draft) -> Result<Saved, String> {
    let body = check_draft(d)?;
    let p = ready(conn)?;
    let reply = match (&d.id, &d.updated_at) {
        (None, _) => rest(conn, &p, "POST", "/rest/v1/events", &[], Some("return=representation"), Some(body))?,
        // Only the version the form opened is changed; a newer one leaves no row to change.
        (Some(id), Some(version)) => {
            let only = [("id", format!("eq.{id}")), ("updated_at", format!("eq.{version}"))];
            let only: Vec<(&str, &str)> = only.iter().map(|(k, v)| (*k, v.as_str())).collect();
            rest(conn, &p, "PATCH", "/rest/v1/events", &only, Some("return=representation"), Some(body))?
        }
        (Some(_), None) => return Err("The form lost track of the event's version. Close it and open the event again.".into()),
    };
    let outcome = match from_json::<Vec<Event>>(reply)?.into_iter().next() {
        Some(event) => {
            put_event(conn, &event).map_err(db_err)?;
            Saved::Saved { event }
        }
        None => {
            let id = d.id.clone().unwrap_or_default();
            let theirs: Vec<Event> =
                from_json(rest(conn, &p, "GET", "/rest/v1/events", &[("select", "*"), ("id", &format!("eq.{id}"))], None, None)?)?;
            let event = theirs.into_iter().next();
            match &event {
                Some(e) => put_event(conn, e),
                None => conn.execute("DELETE FROM shared_events WHERE id = ?1", [&id]),
            }
            .map_err(db_err)?;
            Saved::Conflict { event }
        }
    };
    // So the Activity list shows the change at once. The save already happened if this fails.
    let _ = sync(conn);
    Ok(outcome)
}

pub fn delete(conn: &Connection, id: &str) -> Result<(), String> {
    let p = ready(conn)?;
    rest(conn, &p, "DELETE", "/rest/v1/events", &[("id", &format!("eq.{id}"))], Some("return=minimal"), None)?;
    conn.execute("DELETE FROM shared_events WHERE id = ?1", [id]).map_err(db_err)?;
    conn.execute("UPDATE cards SET shared_event_id = NULL WHERE shared_event_id = ?1", [id]).map_err(db_err)?;
    let _ = sync(conn);
    Ok(())
}

// ---- Cards in the shared calendar ----

/// The start of `day` in this computer's time zone, which is how an all-day event is stored.
pub fn day_start(day: NaiveDate) -> String {
    let midnight = day.and_hms_opt(0, 0, 0).expect("midnight exists");
    stamp(Local.from_local_datetime(&midnight).earliest().map(|t| t.to_utc()).unwrap_or_else(|| midnight.and_utc()))
}

fn card(conn: &Connection, id: i64) -> Result<(String, Option<NaiveDate>, Option<String>), String> {
    conn.query_row("SELECT title, due, shared_event_id FROM cards WHERE id = ?1", [id], |r| {
        let due: Option<String> = r.get(1)?;
        Ok((r.get(0)?, due.and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()), r.get(2)?))
    })
    .map_err(|_| "That card no longer exists.".to_string())
}

fn event_title(card_title: &str) -> String {
    card_title.trim().chars().take(200).collect()
}

/// Adds a card to the shared calendar as an all-day event on its date, and links the two.
pub fn share_card(conn: &Connection, id: i64) -> Result<Event, String> {
    let (title, due, linked) = card(conn, id)?;
    if linked.is_some() {
        return Err("This card is already in the shared calendar.".into());
    }
    let day = due.ok_or("Give the card a date first, then share it.")?;
    let start = day_start(day);
    let draft = Draft { id: None, title: event_title(&title), starts_at: start.clone(), ends_at: start, all_day: true, note: String::new(), updated_at: None };
    match save(conn, &draft)? {
        Saved::Saved { event } => {
            conn.execute("UPDATE cards SET shared_event_id = ?1 WHERE id = ?2", params![event.id, id]).map_err(db_err)?;
            Ok(event)
        }
        Saved::Conflict { .. } => Err("Supabase didn't add the event. Try again.".into()),
    }
}

/// Copies a shared card's title and date to its event, so moving the card moves the event. A card
/// without a date leaves its event where it is. The card wins over a change someone made to the event.
pub fn push_card(conn: &Connection, id: i64) -> Result<(), String> {
    let (title, due, linked) = card(conn, id)?;
    let (Some(event_id), Some(day)) = (linked, due) else { return Ok(()) };
    let p = ready(conn)?;
    let start = day_start(day);
    let body = json!({ "title": event_title(&title), "starts_at": start, "ends_at": start, "all_day": true });
    let reply = rest(conn, &p, "PATCH", "/rest/v1/events", &[("id", &format!("eq.{event_id}"))], Some("return=representation"), Some(body))?;
    match from_json::<Vec<Event>>(reply)?.into_iter().next() {
        Some(event) => {
            put_event(conn, &event).map_err(db_err)?;
            let _ = sync(conn);
            Ok(())
        }
        None => {
            conn.execute("UPDATE cards SET shared_event_id = NULL WHERE id = ?1", [id]).map_err(db_err)?;
            Err("Someone deleted this card's shared event, so the card isn't shared anymore.".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    const SIGN_IN: &str = include_str!("../tests/sample-supabase-sign-in.json");

    fn event(id: &str, title: &str, by: &str) -> Event {
        Event {
            id: id.into(),
            title: title.into(),
            starts_at: "2026-10-28T09:00:00+00:00".into(),
            ends_at: "2026-10-28T10:00:00+00:00".into(),
            all_day: false,
            note: String::new(),
            created_by: Some(by.into()),
            created_at: "2026-10-25T20:43:58.967072+00:00".into(),
            updated_by: Some(by.into()),
            updated_at: "2026-10-25T20:43:58.967072+00:00".into(),
        }
    }

    fn draft(title: &str, start: &str, end: &str) -> Draft {
        Draft { id: None, title: title.into(), starts_at: start.into(), ends_at: end.into(), all_day: false, note: String::new(), updated_at: None }
    }

    #[test]
    fn reads_a_real_sign_in_reply_and_renews_before_expiry() {
        let t = parse_tokens(serde_json::from_str(SIGN_IN).unwrap()).unwrap();
        assert_eq!((t.user.id.as_str(), t.user.email.as_deref()), ("2b98aae4-6ac5-4009-b002-0612e4efc654", Some("member@example.com")));
        assert_eq!((t.refresh_token.as_str(), t.expires_in), ("sample-refresh-token", 3600));
        assert!(parse_tokens(json!({ "code": 400 })).is_err());

        let now = Utc::now();
        assert!(!expiring(now + Duration::minutes(10), now));
        assert!(expiring(now + Duration::seconds(30), now), "renewed within the last minute");
        assert!(expiring(now - Duration::seconds(1), now));
    }

    #[test]
    fn supabase_errors_become_messages_with_a_fix() {
        let answer = |status, v: Value| describe(Fail::Answer(status, v));
        // Real replies from Supabase, captured on 2026-10-25.
        let wrong = json!({"code": 400, "error_code": "invalid_credentials", "msg": "Invalid login credentials"});
        assert_eq!(answer(400, wrong), WRONG_SIGN_IN);
        let jwt = json!({"code": "PGRST301", "details": null, "hint": null, "message": "JWT cryptographic operation failed"});
        assert_eq!(answer(401, jwt), SIGNED_OUT);
        let weak = json!({"code": 422, "error_code": "weak_password", "msg": "Password should be at least 6 characters."});
        assert_eq!(answer(422, weak), "Password should be at least 6 characters.");
        let refused = json!({"code": "42501", "message": "permission denied for table events"});
        assert!(answer(403, refused).contains("publishable key"));
        assert!(answer(401, json!({"message": "Invalid API key"})).contains("publishable key"));
        assert!(answer(404, json!({"code": "PGRST205", "message": "Could not find the table"})).contains("schema.sql"));
        assert_eq!(describe(Fail::Offline), OFFLINE);
        assert_eq!(answer(500, Value::Null), "Supabase said: no details (500).");
    }

    #[test]
    fn drafts_are_checked_before_they_are_sent() {
        let ok = check_draft(&draft("  Kundemøde ", "2026-10-28T10:00:00+01:00", "2026-10-28T11:00:00+01:00")).unwrap();
        assert_eq!(ok["title"], "Kundemøde");
        assert_eq!(ok["starts_at"], "2026-10-28T09:00:00Z", "sent in UTC");
        assert!(check_draft(&draft(" ", "2026-10-28T10:00:00Z", "2026-10-28T11:00:00Z")).unwrap_err().contains("title"));
        assert!(check_draft(&draft(&"x".repeat(201), "2026-10-28T10:00:00Z", "2026-10-28T11:00:00Z")).is_err());
        assert!(check_draft(&draft("x", "2026-10-28T11:00:00Z", "2026-10-28T10:00:00Z")).unwrap_err().contains("before the start"));
        assert!(check_draft(&draft("x", "tomorrow", "2026-10-28T10:00:00Z")).unwrap_err().contains("start"));
        let same = check_draft(&draft("x", "2026-10-28T00:00:00Z", "2026-10-28T00:00:00Z"));
        assert!(same.is_ok(), "a one-day all-day event starts and ends on the same moment");
        assert!(clean_name("  ").is_err());
        assert_eq!(clean_name(" Ana ").unwrap(), "Ana");
        assert!(clean_name(&"a".repeat(41)).is_err());
    }

    #[test]
    fn log_rows_become_activity_lines() {
        let row = |action: &str, before: Value, after: Value| {
            json!({"id": 7, "event_id": "e1", "action": action, "by_user": "u2", "at": "2026-10-25T21:00:00+00:00", "before": before, "after": after})
        };
        let v1 = json!({"title": "Kundemøde", "starts_at": "2026-10-28T09:00:00+00:00", "all_day": false});
        let v2 = json!({"title": "Kundemøde, flyttet", "starts_at": "2026-10-29T09:00:00+00:00", "all_day": false});
        let added = parse_change(&row("added", Value::Null, v1.clone())).unwrap();
        assert_eq!((added.title.as_str(), added.old_title), ("Kundemøde", None));
        let changed = parse_change(&row("changed", v1.clone(), v2)).unwrap();
        assert_eq!(changed.title, "Kundemøde, flyttet");
        assert_eq!(changed.old_title.as_deref(), Some("Kundemøde"));
        assert_eq!(changed.old_starts_at.as_deref(), Some("2026-10-28T09:00:00+00:00"));
        let deleted = parse_change(&row("deleted", v1, Value::Null)).unwrap();
        assert_eq!((deleted.action.as_str(), deleted.title.as_str(), deleted.by_user.as_deref()), ("deleted", "Kundemøde", Some("u2")));
        assert!(parse_change(&json!({"id": 1})).is_none());
    }

    #[test]
    fn the_copy_is_replaced_and_the_status_follows_the_sign_in() {
        let conn = db::open(&db::temp_path("calendar")).unwrap();
        assert_eq!(state(&conn).unwrap().status, "not_set_up");
        set_setting(&conn, "calendar_url", "https://x.supabase.co").unwrap();
        set_setting(&conn, "calendar_key", "sb_publishable_x").unwrap();
        assert_eq!(state(&conn).unwrap().status, "signed_out");
        set_setting(&conn, "calendar.user_id", "u1").unwrap();
        assert_eq!(state(&conn).unwrap().status, "needs_name");

        let members = vec![Member { id: "u1".into(), name: "Ana".into() }, Member { id: "u2".into(), name: "Kim".into() }];
        let both = vec![event("e1", "Kundemøde", "u2"), event("e2", "Frokost", "u1")];
        assert!(store(&conn, &members, &both, &[]).unwrap());
        assert!(!store(&conn, &members, &both, &[]).unwrap(), "nothing changed, nothing written");
        let s = state(&conn).unwrap();
        assert_eq!((s.status, s.events.len(), s.members[1].name.as_str()), ("ready", 2, "Kim"));
        assert_eq!(s.events[0], both[0]);

        // An event deleted in Supabase leaves the copy at the next sync.
        assert!(store(&conn, &members, &both[1..], &[]).unwrap());
        assert_eq!(state(&conn).unwrap().events.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["e2"]);

        set_hidden(&conn, &["u2".into()]).unwrap();
        assert_eq!(state(&conn).unwrap().hidden, vec!["u2"]);
    }

    #[test]
    fn cards_only_reach_supabase_when_shared_with_a_date() {
        // No Project URL is set, so any call to Supabase would fail with "Add the Project URL".
        let conn = db::open(&db::temp_path("calendar-cards")).unwrap();
        let plain = db::add_card(&conn, "Not shared", "inbox", Some("2026-10-28"), "manual").unwrap();
        assert_eq!(push_card(&conn, plain), Ok(()), "a card that isn't shared is never sent");
        let undated = db::add_card(&conn, "No date", "inbox", None, "manual").unwrap();
        assert_eq!(share_card(&conn, undated).unwrap_err(), "Give the card a date first, then share it.");
        conn.execute("UPDATE cards SET shared_event_id = 'e1' WHERE id = ?1", [undated]).unwrap();
        assert_eq!(push_card(&conn, undated), Ok(()), "a shared card without a date leaves its event alone");
        assert!(share_card(&conn, undated).unwrap_err().contains("already"));
        assert!(push_card(&conn, 999).is_err());
    }

    #[test]
    fn all_day_events_start_at_local_midnight() {
        let start = day_start(NaiveDate::from_ymd_opt(2026, 10, 28).unwrap());
        let back = DateTime::parse_from_rfc3339(&start).unwrap().with_timezone(&Local);
        assert_eq!(back.date_naive().to_string(), "2026-10-28");
        assert_eq!(back.time().to_string(), "00:00:00");
    }

    /// Against a real Supabase project with supabase/schema.sql applied and two members:
    /// TODO_HELPER_SUPABASE_URL, TODO_HELPER_SUPABASE_KEY, and TODO_HELPER_SUPABASE_A/_B as
    /// "email password". Signs in and out, so it ends the calendar sign-in on this computer.
    /// Run: `cargo test -- --ignored calendar_live --test-threads 1`.
    #[test]
    #[ignore]
    fn calendar_live() {
        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
        let conn = db::open(&db::temp_path("calendar-live")).unwrap();
        set_setting(&conn, "calendar_url", &var("TODO_HELPER_SUPABASE_URL")).unwrap();
        set_setting(&conn, "calendar_key", &var("TODO_HELPER_SUPABASE_KEY")).unwrap();
        let login = |who: &str| {
            let v = var(who);
            let (email, password) = v.split_once(' ').unwrap();
            sign_in(&conn, email, password).unwrap();
        };
        assert_eq!(sign_in(&conn, "nobody@example.com", "wrong-password").unwrap_err(), WRONG_SIGN_IN);

        // A adds an event.
        login("TODO_HELPER_SUPABASE_A");
        set_name(&conn, "Live A").unwrap();
        sync(&conn).unwrap();
        let mut d = draft("Live check", &stamp(Utc::now() + Duration::days(1)), &stamp(Utc::now() + Duration::days(1) + Duration::hours(1)));
        let Saved::Saved { event: mine } = save(&conn, &d).unwrap() else { panic!("not saved") };
        let a = me(&conn).unwrap();
        assert_eq!(mine.created_by.as_deref(), Some(a.as_str()));

        // B changes A's event; A's form, still on the old version, gets B's version back.
        login("TODO_HELPER_SUPABASE_B");
        set_name(&conn, "Live B").unwrap();
        sync(&conn).unwrap();
        let b = me(&conn).unwrap();
        d.id = Some(mine.id.clone());
        d.updated_at = Some(mine.updated_at.clone());
        d.title = "Live check, moved by B".into();
        let Saved::Saved { event: theirs } = save(&conn, &d).unwrap() else { panic!("B's change didn't save") };
        assert_eq!((theirs.created_by.as_deref(), theirs.updated_by.as_deref()), (Some(a.as_str()), Some(b.as_str())));
        d.title = "Live check, A's stale form".into();
        match save(&conn, &d).unwrap() {
            Saved::Conflict { event: Some(e) } => assert_eq!(e.title, "Live check, moved by B"),
            other => panic!("expected a conflict, got {other:?}"),
        }

        // B deletes it, and the log says who did what.
        delete(&conn, &mine.id).unwrap();
        let log: Vec<Change> = state(&conn).unwrap().log.into_iter().filter(|c| c.event_id == mine.id).collect();
        let lines: Vec<(&str, bool)> = log.iter().rev().map(|c| (c.action.as_str(), c.by_user.as_deref() == Some(b.as_str()))).collect();
        assert_eq!(lines, vec![("added", false), ("changed", true), ("deleted", true)]);

        sign_out(&conn).unwrap();
        assert_eq!(state(&conn).unwrap().status, "signed_out");
        assert!(state(&conn).unwrap().events.is_empty(), "sign-out removes the copy");
    }
}
