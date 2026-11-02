//! Tasks members give each other, in the shared calendar's Supabase project (supabase/schema.sql,
//! `tasks`). A task given to you becomes a card in Inbox on its deadline; its title, deadline, and
//! done follow Supabase, and the card's changes go back. Any member may change any task.
use crate::calendar::{db_err, from_json, me, ready, rest};
use crate::db;
use chrono::{Local, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub note: String,
    pub assignee: String,
    /// The deadline, YYYY-MM-DD.
    pub due: String,
    pub done: bool,
    /// Who gave the task.
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_by: Option<String>,
    pub updated_at: String,
}

/// The uid in `synced_events` that says a card was made for a task, so a card you delete stays deleted.
fn uid(task: &str) -> String {
    format!("task:{task}")
}

/// Stores every task, and makes your board match the tasks given to you. Returns true when a card changed.
pub fn apply(conn: &Connection, me: &str, tasks: &[Task]) -> rusqlite::Result<bool> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM shared_tasks", [])?;
    let mut changed = false;
    for t in tasks {
        tx.execute(
            "INSERT INTO shared_tasks (id, title, note, assignee, due, done, created_by, created_at, updated_by, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![t.id, t.title, t.note, t.assignee, t.due, t.done, t.created_by, t.created_at, t.updated_by, t.updated_at],
        )?;
        if t.assignee != me {
            continue;
        }
        let card: Option<(i64, String, String, Option<String>)> = tx
            .query_row("SELECT id, col, title, due FROM cards WHERE task_id = ?1", [&t.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .optional()?;
        let made = tx.query_row("SELECT 1 FROM synced_events WHERE uid = ?1", [uid(&t.id)], |_| Ok(())).optional()?.is_some();
        match card {
            None if !made && !t.done => {
                let id = db::add_card(&tx, &t.title, "inbox", Some(&t.due), "manual")?;
                tx.execute("UPDATE cards SET task_id = ?2, task_new = 1 WHERE id = ?1", params![id, t.id])?;
                tx.execute("INSERT OR REPLACE INTO synced_events (uid, due) VALUES (?1, ?2)", params![uid(&t.id), t.due])?;
                changed = true;
            }
            None => {}
            Some((id, col, title, due)) => {
                if title != t.title || due.as_deref() != Some(t.due.as_str()) {
                    tx.execute("UPDATE cards SET title = ?2, due = ?3 WHERE id = ?1", params![id, t.title, t.due])?;
                    changed = true;
                }
                if t.done && col != "done" {
                    tx.execute(
                        "UPDATE cards SET col = 'done', position = (SELECT COALESCE(MIN(position), 0) - 1 FROM cards WHERE col = 'done') WHERE id = ?1",
                        [id],
                    )?;
                    changed = true;
                }
            }
        }
    }
    // A task that was deleted, or given to someone else, takes its open card with it.
    let mine: Vec<&str> = tasks.iter().filter(|t| t.assignee == me).map(|t| t.id.as_str()).collect();
    let stale: Vec<i64> = tx
        .prepare("SELECT id, task_id FROM cards WHERE task_id IS NOT NULL AND col != 'done'")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(Result::ok)
        .filter(|(_, task)| !mine.contains(&task.as_str()))
        .map(|(id, _)| id)
        .collect();
    for id in &stale {
        tx.execute("DELETE FROM cards WHERE id = ?1", [id])?;
    }
    tx.commit()?;
    Ok(changed || !stale.is_empty())
}

/// Every task, from the copy, for the Given list and the "Fra" line on a card.
pub fn list(conn: &Connection) -> rusqlite::Result<Vec<Task>> {
    conn.prepare("SELECT id, title, note, assignee, due, done, created_by, created_at, updated_by, updated_at FROM shared_tasks ORDER BY due, title")?
        .query_map([], |r| {
            Ok(Task {
                id: r.get(0)?,
                title: r.get(1)?,
                note: r.get(2)?,
                assignee: r.get(3)?,
                due: r.get(4)?,
                done: r.get(5)?,
                created_by: r.get(6)?,
                created_at: r.get(7)?,
                updated_by: r.get(8)?,
                updated_at: r.get(9)?,
            })
        })?
        .collect()
}

/// Reads every task from Supabase and applies it. Called by the calendar's minute sync.
pub fn sync(conn: &Connection, p: &crate::calendar::Project) -> Result<bool, String> {
    let Some(me) = me(conn) else { return Ok(false) };
    let tasks: Vec<Task> = from_json(rest(conn, p, "GET", "/rest/v1/tasks", &[("select", "*"), ("order", "due")], None, None)?)?;
    apply(conn, &me, &tasks).map_err(db_err)
}

#[derive(Deserialize, Debug)]
pub struct Draft {
    pub title: String,
    pub note: String,
    pub assignee: String,
    pub due: String,
}

/// Checks a new task before it's sent: a title, a member, and a deadline from today on.
pub fn check(d: &Draft, today: NaiveDate) -> Result<Value, String> {
    let title = d.title.trim();
    if title.is_empty() {
        return Err("A task needs a title.".into());
    }
    if d.assignee.is_empty() {
        return Err("Pick who the task is for.".into());
    }
    let due = NaiveDate::parse_from_str(&d.due, "%Y-%m-%d").map_err(|_| "Pick a deadline.".to_string())?;
    if due < today {
        return Err("The deadline is in the past. Pick today or a later day.".into());
    }
    Ok(json!({ "title": title.chars().take(200).collect::<String>(), "note": d.note.chars().take(4000).collect::<String>(), "assignee": d.assignee, "due": d.due }))
}

/// Gives a task to a member, then syncs so the Given list shows it.
pub fn give(conn: &Connection, d: &Draft) -> Result<(), String> {
    let body = check(d, Local::now().date_naive())?;
    let p = ready(conn)?;
    rest(conn, &p, "POST", "/rest/v1/tasks", &[], Some("return=minimal"), Some(body))?;
    sync(conn, &p).map(|_| ())
}

/// Sends a task card's title, deadline, and whether it's done back to its task.
pub fn push_card(conn: &Connection, id: i64) -> Result<(), String> {
    let (task, title, due, col): (Option<String>, String, Option<String>, String) = conn
        .query_row("SELECT task_id, title, due, col FROM cards WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(|_| "That card no longer exists.".to_string())?;
    let Some(task) = task else { return Ok(()) };
    let mut body = json!({ "title": title.trim().chars().take(200).collect::<String>(), "done": col == "done" });
    if let Some(due) = due {
        body["due"] = json!(due);
    }
    let p = ready(conn)?;
    rest(conn, &p, "PATCH", "/rest/v1/tasks", &[("id", &format!("eq.{task}"))], Some("return=minimal"), Some(body))?;
    sync(conn, &p).map(|_| ())
}

/// Opening a task card takes away its "New" mark.
pub fn seen(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE cards SET task_new = 0 WHERE id = ?1", [id]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, assignee: &str, due: &str, done: bool) -> Task {
        Task {
            id: id.into(),
            title: format!("Task {id}"),
            note: String::new(),
            assignee: assignee.into(),
            due: due.into(),
            done,
            created_by: Some("sam".into()),
            created_at: "2026-10-27T10:00:00Z".into(),
            updated_by: Some("sam".into()),
            updated_at: "2026-10-27T10:00:00Z".into(),
        }
    }

    fn card(conn: &Connection, task: &str) -> Option<db::Card> {
        db::list_cards(conn).unwrap().into_iter().find(|c| c.task_id.as_deref() == Some(task))
    }

    #[test]
    fn tasks_given_to_you_become_cards_and_follow_supabase() {
        let conn = db::open(&db::temp_path("tasks")).unwrap();
        let mut tasks = vec![task("t1", "me", "2026-10-30", false), task("t2", "kim", "2026-10-30", false)];

        // Only your task becomes a card: in Inbox, on its deadline, marked new.
        assert!(apply(&conn, "me", &tasks).unwrap());
        let c = card(&conn, "t1").unwrap();
        assert_eq!((c.col.as_str(), c.due.as_deref(), c.task_new), ("inbox", Some("2026-10-30"), true));
        assert!(card(&conn, "t2").is_none());
        assert_eq!(list(&conn).unwrap().len(), 2, "every task is kept for the Given list");

        // Nothing changed: the card stays where you put it, and nothing is reported.
        conn.execute("UPDATE cards SET col = 'doing' WHERE task_id = 't1'", []).unwrap();
        assert!(!apply(&conn, "me", &tasks).unwrap());
        assert_eq!(card(&conn, "t1").unwrap().col, "doing");

        // Someone moves the deadline and marks it done: the card follows.
        tasks[0].due = "2026-11-02".into();
        tasks[0].done = true;
        assert!(apply(&conn, "me", &tasks).unwrap());
        let c = card(&conn, "t1").unwrap();
        assert_eq!((c.col.as_str(), c.due.as_deref()), ("done", Some("2026-11-02")));

        // A card you deleted doesn't come back.
        db::delete_card(&conn, c.id).unwrap();
        apply(&conn, "me", &tasks).unwrap();
        assert!(card(&conn, "t1").is_none());

        // A task given to someone else takes its open card with it.
        let mut later = vec![task("t3", "me", "2026-11-05", false)];
        apply(&conn, "me", &later).unwrap();
        assert!(card(&conn, "t3").is_some());
        later[0].assignee = "kim".into();
        assert!(apply(&conn, "me", &later).unwrap());
        assert!(card(&conn, "t3").is_none());
    }

    #[test]
    fn a_new_task_needs_a_title_a_member_and_a_deadline_from_today() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 27).unwrap();
        let d = |title: &str, assignee: &str, due: &str| Draft { title: title.into(), note: String::new(), assignee: assignee.into(), due: due.into() };
        assert!(check(&d("Write the offer", "kim", "2026-10-27"), today).is_ok());
        assert_eq!(check(&d(" ", "kim", "2026-10-28"), today).unwrap_err(), "A task needs a title.");
        assert_eq!(check(&d("x", "", "2026-10-28"), today).unwrap_err(), "Pick who the task is for.");
        assert_eq!(check(&d("x", "kim", ""), today).unwrap_err(), "Pick a deadline.");
        assert!(check(&d("x", "kim", "2026-10-26"), today).unwrap_err().starts_with("The deadline is in the past"));
    }
}
