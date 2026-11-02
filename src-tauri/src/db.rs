use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

// Each entry runs once, in order. PRAGMA user_version records how many have run.
// Add new schema changes at the end; never edit an entry that has shipped.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE cards (
        id INTEGER PRIMARY KEY,
        title TEXT NOT NULL,
        col TEXT NOT NULL CHECK (col IN ('inbox', 'today', 'doing', 'done')),
        position INTEGER NOT NULL,
        due TEXT,
        source TEXT NOT NULL DEFAULT 'manual'
            CHECK (source IN ('manual', 'email', 'discord', 'itslearning')),
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );",
    // A note can belong to one card. Deleting the card keeps the note.
    "CREATE TABLE notes (
        id INTEGER PRIMARY KEY,
        body TEXT NOT NULL DEFAULT '',
        card_id INTEGER UNIQUE REFERENCES cards(id) ON DELETE SET NULL,
        updated_at TEXT NOT NULL DEFAULT (datetime('now'))
    );",
    // Plain settings, plus bookkeeping such as how far events.jsonl has been read. Never secrets.
    "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
     CREATE TABLE sessions (
        id TEXT PRIMARY KEY,
        cwd TEXT,
        first_prompt TEXT,
        started_at TEXT NOT NULL,
        last_event_at TEXT NOT NULL,
        ended_at TEXT
     );
     CREATE TABLE agents (
        id TEXT PRIMARY KEY,
        session_id TEXT NOT NULL,
        description TEXT,
        agent_type TEXT,
        started_at TEXT NOT NULL,
        ended_at TEXT
     );
     CREATE INDEX agents_by_session ON agents (session_id);
     -- Agent launches seen in PreToolUse that no SubagentStart has claimed yet.
     CREATE TABLE pending_agents (
        tool_use_id TEXT PRIMARY KEY,
        session_id TEXT NOT NULL,
        description TEXT,
        subagent_type TEXT,
        ts TEXT NOT NULL
     );",
    // Raw items from the sources, waiting to be turned into cards. Cards remember where they came from.
    "CREATE TABLE inbox_items (
        id INTEGER PRIMARY KEY,
        source TEXT NOT NULL CHECK (source IN ('email', 'discord', 'itslearning')),
        external_id TEXT NOT NULL,
        sender TEXT,
        subject TEXT,
        received_at TEXT,
        body TEXT,
        processed INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        UNIQUE (source, external_id)
     );
     ALTER TABLE cards ADD COLUMN inbox_item_id INTEGER REFERENCES inbox_items(id) ON DELETE SET NULL;
     ALTER TABLE cards ADD COLUMN external_id TEXT;
     CREATE UNIQUE INDEX cards_by_external_id ON cards (external_id) WHERE external_id IS NOT NULL;",
    // How many times extraction failed on an item, so a message that always fails is given up on.
    "ALTER TABLE inbox_items ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;",
    // itslearning event UIDs that already got a card, so a card the owner deletes stays deleted.
    "CREATE TABLE synced_events (uid TEXT PRIMARY KEY);",
    // The date (or 'cancelled') itslearning last gave each event, so a change in itslearning moves
    // the card, while a date the owner set stays put.
    "ALTER TABLE synced_events ADD COLUMN due TEXT;",
    // This computer's copy of the shared calendar in Supabase (calendar.rs), replaced at every
    // sync, so the Calendar view opens at once and works offline. A card shared to the calendar
    // remembers its event.
    "CREATE TABLE shared_members (id TEXT PRIMARY KEY, name TEXT NOT NULL);
     CREATE TABLE shared_events (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        starts_at TEXT NOT NULL,
        ends_at TEXT NOT NULL,
        all_day INTEGER NOT NULL,
        note TEXT NOT NULL,
        created_by TEXT,
        created_at TEXT NOT NULL,
        updated_by TEXT,
        updated_at TEXT NOT NULL
     );
     CREATE TABLE shared_log (
        id INTEGER PRIMARY KEY,
        event_id TEXT NOT NULL,
        action TEXT NOT NULL,
        by_user TEXT,
        at TEXT NOT NULL,
        title TEXT NOT NULL,
        starts_at TEXT,
        all_day INTEGER NOT NULL,
        old_title TEXT,
        old_starts_at TEXT
     );
     ALTER TABLE cards ADD COLUMN shared_event_id TEXT;",
    // The start and end (UTC, RFC 3339) of a card made from a timed itslearning event, so the
    // Calendar view shows a lesson at its hours instead of all day.
    "ALTER TABLE cards ADD COLUMN starts_at TEXT;
     ALTER TABLE cards ADD COLUMN ends_at TEXT;",
    // A hand-in deadline, which the board colors as it gets close. Set from the card sheet, or by
    // extraction for an assignment email.
    "ALTER TABLE cards ADD COLUMN deadline INTEGER NOT NULL DEFAULT 0;",
    // A task another member gave you (tasks.rs): the card's task, whether you haven't opened it
    // yet, and the copy of every member's tasks for the Given list.
    "ALTER TABLE cards ADD COLUMN task_id TEXT;
     ALTER TABLE cards ADD COLUMN task_new INTEGER NOT NULL DEFAULT 0;
     CREATE TABLE shared_tasks (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        note TEXT NOT NULL,
        assignee TEXT NOT NULL,
        due TEXT NOT NULL,
        done INTEGER NOT NULL,
        created_by TEXT,
        created_at TEXT NOT NULL,
        updated_by TEXT,
        updated_at TEXT NOT NULL
     );",
];

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let mut conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    // Every transaction here writes. Taking the write lock at BEGIN lets the busy timeout apply;
    // a transaction that reads first fails at once if another connection writes in between.
    conn.set_transaction_behavior(rusqlite::TransactionBehavior::Immediate);
    let done: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(done as usize) {
        conn.execute_batch(&format!("BEGIN; {sql} PRAGMA user_version = {}; COMMIT;", i + 1))?;
    }
    Ok(conn)
}

#[derive(Serialize, Deserialize)]
pub struct Card {
    pub id: i64,
    pub title: String,
    pub col: String,
    pub position: i64,
    pub due: Option<String>,
    pub source: String,
    pub note_id: Option<i64>,
    pub inbox_item_id: Option<i64>,
    pub external_id: Option<String>,
    /// The shared calendar event this card was shared as, if any.
    #[serde(default)]
    pub shared_event_id: Option<String>,
    /// When the itslearning event it came from starts and ends, if it has a time.
    #[serde(default)]
    pub starts_at: Option<String>,
    #[serde(default)]
    pub ends_at: Option<String>,
    /// A hand-in deadline, colored on the board as it gets close.
    #[serde(default)]
    pub deadline: bool,
    /// The task another member gave you, if this card is one, and whether you haven't opened it yet.
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub task_new: bool,
}

const CARD_SELECT: &str = "SELECT c.id, c.title, c.col, c.position, c.due, c.source, n.id, c.inbox_item_id, c.external_id, c.shared_event_id,
    c.starts_at, c.ends_at, c.deadline, c.task_id, c.task_new FROM cards c LEFT JOIN notes n ON n.card_id = c.id";

fn card_row(r: &rusqlite::Row) -> rusqlite::Result<Card> {
    Ok(Card {
        id: r.get(0)?,
        title: r.get(1)?,
        col: r.get(2)?,
        position: r.get(3)?,
        due: r.get(4)?,
        source: r.get(5)?,
        note_id: r.get(6)?,
        inbox_item_id: r.get(7)?,
        external_id: r.get(8)?,
        shared_event_id: r.get(9)?,
        starts_at: r.get(10)?,
        ends_at: r.get(11)?,
        deadline: r.get(12)?,
        task_id: r.get(13)?,
        task_new: r.get(14)?,
    })
}

pub fn list_cards(conn: &Connection) -> rusqlite::Result<Vec<Card>> {
    let mut stmt = conn.prepare(&format!("{CARD_SELECT} ORDER BY c.position, c.id"))?;
    let rows = stmt.query_map([], card_row)?;
    rows.collect()
}

/// Puts a deleted card back as it was, including the link to its note, and returns its new id.
/// The old id may already belong to a newer card, so the card gets a fresh one.
pub fn restore_card(conn: &Connection, c: &Card) -> rusqlite::Result<i64> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO cards (title, col, position, due, source, inbox_item_id, external_id, shared_event_id, starts_at, ends_at, deadline, task_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![c.title, c.col, c.position, c.due, c.source, c.inbox_item_id, c.external_id, c.shared_event_id, c.starts_at, c.ends_at, c.deadline, c.task_id],
    )?;
    let id = tx.last_insert_rowid();
    if let Some(note) = c.note_id {
        tx.execute("UPDATE notes SET card_id = ?1 WHERE id = ?2 AND card_id IS NULL", params![id, note])?;
    }
    tx.commit()?;
    Ok(id)
}

pub fn set_deadline(conn: &Connection, id: i64, on: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE cards SET deadline = ?1 WHERE id = ?2", params![on, id])?;
    Ok(())
}

pub fn set_due(conn: &Connection, id: i64, due: Option<&str>) -> rusqlite::Result<()> {
    conn.execute("UPDATE cards SET due = ?1 WHERE id = ?2", params![due, id])?;
    Ok(())
}

#[derive(Serialize)]
pub struct Note {
    pub id: i64,
    pub body: String,
    pub card_title: Option<String>,
    pub updated_at: String,
}

const NOTE_SELECT: &str = "SELECT n.id, n.body, c.title, n.updated_at
    FROM notes n LEFT JOIN cards c ON c.id = n.card_id";

fn note_row(r: &rusqlite::Row) -> rusqlite::Result<Note> {
    Ok(Note { id: r.get(0)?, body: r.get(1)?, card_title: r.get(2)?, updated_at: r.get(3)? })
}

pub fn list_notes(conn: &Connection) -> rusqlite::Result<Vec<Note>> {
    let mut stmt = conn.prepare(&format!("{NOTE_SELECT} ORDER BY n.updated_at DESC, n.id DESC"))?;
    let rows = stmt.query_map([], note_row)?;
    rows.collect()
}

pub fn card_note(conn: &Connection, card_id: i64) -> rusqlite::Result<Option<Note>> {
    conn.query_row(&format!("{NOTE_SELECT} WHERE n.card_id = ?1"), [card_id], note_row).optional()
}

pub fn create_note(conn: &Connection, card_id: Option<i64>) -> rusqlite::Result<Note> {
    conn.execute("INSERT INTO notes (card_id) VALUES (?1)", [card_id])?;
    let id = conn.last_insert_rowid();
    conn.query_row(&format!("{NOTE_SELECT} WHERE n.id = ?1"), [id], note_row)
}

pub fn save_note(conn: &Connection, id: i64, body: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE notes SET body = ?1, updated_at = datetime('now') WHERE id = ?2", params![body, id])?;
    Ok(())
}

pub fn delete_note(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM notes WHERE id = ?1", [id])?;
    Ok(())
}

/// Adds a card at the top of a column and returns its id.
pub fn add_card(conn: &Connection, title: &str, col: &str, due: Option<&str>, source: &str) -> rusqlite::Result<i64> {
    add_source_card(conn, title, col, due, source, None, None)
}

/// Adds a card at the top of its column, linked to the item or event it came from, and returns its id.
pub fn add_source_card(
    conn: &Connection,
    title: &str,
    col: &str,
    due: Option<&str>,
    source: &str,
    inbox_item_id: Option<i64>,
    external_id: Option<&str>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO cards (title, col, position, due, source, inbox_item_id, external_id)
         VALUES (?1, ?2, (SELECT COALESCE(MIN(position), 0) - 1 FROM cards WHERE col = ?2), ?3, ?4, ?5, ?6)",
        params![title, col, due, source, inbox_item_id, external_id],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Stores one inbox item unless it is already stored. Returns 1 if it was new.
pub fn add_inbox_item(
    conn: &Connection, source: &str, external_id: &str, sender: &str, subject: &str, received_at: &str, body: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT OR IGNORE INTO inbox_items (source, external_id, sender, subject, received_at, body) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![source, external_id, sender, subject, received_at, body],
    )
}

#[derive(Serialize)]
pub struct InboxItem {
    pub sender: Option<String>,
    pub subject: Option<String>,
    pub received_at: Option<String>,
}

pub fn inbox_item(conn: &Connection, id: i64) -> rusqlite::Result<Option<InboxItem>> {
    conn.query_row(
        "SELECT sender, subject, received_at FROM inbox_items WHERE id = ?1",
        [id],
        |r| Ok(InboxItem { sender: r.get(0)?, subject: r.get(1)?, received_at: r.get(2)? }),
    )
    .optional()
}

pub fn rename_card(conn: &Connection, id: i64, title: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE cards SET title = ?1 WHERE id = ?2", params![title, id])?;
    Ok(())
}

/// Deletes a card and returns it as it was just before, so Undo can put it back.
pub fn delete_card(conn: &Connection, id: i64) -> rusqlite::Result<Option<Card>> {
    let tx = conn.unchecked_transaction()?;
    let card = tx.query_row(&format!("{CARD_SELECT} WHERE c.id = ?1"), [id], card_row).optional()?;
    tx.execute("DELETE FROM cards WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(card)
}

/// Puts a card into `col` at `index`, counted among the other cards in that column,
/// and renumbers that column's positions from 0.
pub fn move_card(conn: &mut Connection, id: i64, col: &str, index: usize) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    let mut ids: Vec<i64> = tx
        .prepare("SELECT id FROM cards WHERE col = ?1 AND id != ?2 ORDER BY position, id")?
        .query_map(params![col, id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    ids.insert(index.min(ids.len()), id);
    tx.execute("UPDATE cards SET col = ?1 WHERE id = ?2", params![col, id])?;
    for (pos, card) in ids.iter().enumerate() {
        tx.execute("UPDATE cards SET position = ?1 WHERE id = ?2", params![pos as i64, card])?;
    }
    tx.commit()
}

pub fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
pub fn temp_path(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("todo-helper-{name}-{nanos}.db"))
}

#[cfg(test)]
pub fn day(s: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_file_gets_the_schema_and_reopening_keeps_data() {
        let path = temp_path("schema");
        let conn = open(&path).unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());

        let first = add_card(&conn, "first", "inbox", None, "manual").unwrap();
        let second = add_card(&conn, "second", "inbox", Some("2026-10-23"), "manual").unwrap();
        rename_card(&conn, first, "renamed").unwrap();
        drop(conn);

        // Reopening runs no migration twice and keeps the cards; the newest card is on top.
        let conn = open(&path).unwrap();
        let cards = list_cards(&conn).unwrap();
        assert_eq!(cards.iter().map(|c| c.id).collect::<Vec<_>>(), vec![second, first]);
        assert_eq!(cards[1].title, "renamed");
        assert_eq!(cards[0].due.as_deref(), Some("2026-10-23"));

        delete_card(&conn, first).unwrap();
        assert_eq!(list_cards(&conn).unwrap().len(), 1);
        assert!(add_card(&conn, "bad", "nowhere", None, "manual").is_err());
    }

    #[test]
    fn notes_link_to_cards_and_outlive_them() {
        let conn = open(&temp_path("notes")).unwrap();
        let card = add_card(&conn, "essay", "inbox", None, "manual").unwrap();
        let note = create_note(&conn, Some(card)).unwrap();
        save_note(&conn, note.id, "first line
second").unwrap();
        assert_eq!(list_cards(&conn).unwrap()[0].note_id, Some(note.id));
        assert_eq!(card_note(&conn, card).unwrap().unwrap().body, "first line
second");
        assert!(create_note(&conn, Some(card)).is_err(), "one note per card");

        let saved = delete_card(&conn, card).unwrap().expect("the deleted row");
        assert_eq!(saved.note_id, Some(note.id));
        let notes = list_notes(&conn).unwrap();
        assert_eq!((notes.len(), notes[0].card_title.as_deref()), (1, None));

        // A new card takes the freed id; Undo still works and keeps the note link.
        let newer = add_card(&conn, "newer", "inbox", None, "manual").unwrap();
        assert_eq!(newer, card);
        let back = restore_card(&conn, &saved).unwrap();
        assert_ne!(back, newer);
        assert_eq!(card_note(&conn, back).unwrap().map(|n| n.id), Some(note.id));
        delete_card(&conn, back).unwrap();
        delete_note(&conn, note.id).unwrap();
        assert!(list_notes(&conn).unwrap().is_empty());
    }

    #[test]
    fn moving_cards_between_and_within_columns() {
        let mut conn = open(&temp_path("move")).unwrap();
        let c = add_card(&conn, "c", "inbox", None, "manual").unwrap();
        let b = add_card(&conn, "b", "inbox", None, "manual").unwrap();
        let a = add_card(&conn, "a", "inbox", None, "manual").unwrap();
        let ids = |conn: &Connection, col: &str| -> Vec<i64> {
            list_cards(conn).unwrap().into_iter().filter(|x| x.col == col).map(|x| x.id).collect()
        };
        assert_eq!(ids(&conn, "inbox"), vec![a, b, c]);

        move_card(&mut conn, a, "inbox", 2).unwrap(); // to the bottom
        assert_eq!(ids(&conn, "inbox"), vec![b, c, a]);
        move_card(&mut conn, a, "inbox", 1).unwrap(); // between b and c
        assert_eq!(ids(&conn, "inbox"), vec![b, a, c]);

        move_card(&mut conn, b, "today", 0).unwrap();
        move_card(&mut conn, c, "today", 99).unwrap(); // past the end goes last
        assert_eq!(ids(&conn, "today"), vec![b, c]);
        assert_eq!(ids(&conn, "inbox"), vec![a]);
        assert!(move_card(&mut conn, a, "nowhere", 0).is_err());
        assert_eq!(ids(&conn, "inbox"), vec![a]);
    }
}
