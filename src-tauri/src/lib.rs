mod calendar;
mod db;
mod discord;
mod extract;
mod gcal;
mod itslearning;
mod jobs;
mod outlook;
mod sessions;
mod settings;
mod tasks;

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

struct Db(Mutex<Connection>);
struct DbPath(PathBuf);

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn clean_title(title: &str) -> Res<String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("A card needs a title.".into());
    }
    Ok(title.chars().take(500).collect())
}

#[tauri::command]
fn list_cards(db: State<Db>) -> Res<Vec<db::Card>> {
    db::list_cards(&db.0.lock().unwrap()).map_err(err)
}

#[tauri::command]
fn add_card(db: State<Db>, title: String) -> Res<i64> {
    db::add_card(&db.0.lock().unwrap(), &clean_title(&title)?, "inbox", None, "manual").map_err(err)
}

#[tauri::command]
fn rename_card(db: State<Db>, id: i64, title: String) -> Res<()> {
    db::rename_card(&db.0.lock().unwrap(), id, &clean_title(&title)?).map_err(err)
}

#[tauri::command]
fn delete_card(db: State<Db>, id: i64) -> Res<Option<db::Card>> {
    db::delete_card(&db.0.lock().unwrap(), id).map_err(err)
}

#[tauri::command]
fn restore_card(db: State<Db>, card: db::Card) -> Res<i64> {
    db::restore_card(&db.0.lock().unwrap(), &card).map_err(err)
}

#[tauri::command]
fn move_card(db: State<Db>, id: i64, col: String, index: usize) -> Res<()> {
    db::move_card(&mut db.0.lock().unwrap(), id, &col, index).map_err(err)
}

#[tauri::command]
fn set_due(db: State<Db>, id: i64, due: Option<String>) -> Res<()> {
    let due = clean_due(due)?;
    db::set_due(&db.0.lock().unwrap(), id, due.as_deref()).map_err(err)
}

#[tauri::command]
fn set_deadline(db: State<Db>, id: i64, on: bool) -> Res<()> {
    db::set_deadline(&db.0.lock().unwrap(), id, on).map_err(err)
}

/// An empty date clears the due date; anything else must be a real YYYY-MM-DD date.
fn clean_due(due: Option<String>) -> Res<Option<String>> {
    match due.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(d) => chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map(|_| Some(d.to_string()))
            .map_err(|_| format!("\"{d}\" is not a date. Use the format YYYY-MM-DD.")),
    }
}

#[tauri::command]
fn inbox_item(db: State<Db>, id: i64) -> Res<Option<db::InboxItem>> {
    db::inbox_item(&db.0.lock().unwrap(), id).map_err(err)
}

#[tauri::command]
fn list_notes(db: State<Db>) -> Res<Vec<db::Note>> {
    db::list_notes(&db.0.lock().unwrap()).map_err(err)
}

#[tauri::command]
fn card_note(db: State<Db>, card_id: i64) -> Res<Option<db::Note>> {
    db::card_note(&db.0.lock().unwrap(), card_id).map_err(err)
}

#[tauri::command]
fn create_note(db: State<Db>, card_id: Option<i64>) -> Res<db::Note> {
    db::create_note(&db.0.lock().unwrap(), card_id).map_err(err)
}

#[tauri::command]
fn save_note(db: State<Db>, id: i64, body: String) -> Res<()> {
    db::save_note(&db.0.lock().unwrap(), id, &body).map_err(err)
}

#[tauri::command]
fn delete_note(db: State<Db>, id: i64) -> Res<()> {
    db::delete_note(&db.0.lock().unwrap(), id).map_err(err)
}

#[tauri::command]
fn list_sessions(db: State<Db>) -> Res<Vec<sessions::Session>> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    sessions::list(&db.0.lock().unwrap(), &now).map_err(err)
}

#[tauri::command]
fn get_settings(db: State<Db>) -> settings::View {
    settings::view(&db.0.lock().unwrap())
}

#[tauri::command]
fn save_setting(db: State<Db>, key: String, value: String) -> Res<String> {
    let value = settings::clean_plain(&key, &value)?;
    db::set_setting(&db.0.lock().unwrap(), &key, &value).map_err(err)?;
    Ok(value)
}

#[tauri::command]
fn set_secret(name: String, value: String) -> Res<()> {
    settings::set_secret(&name, &value)
}

#[tauri::command]
fn clear_secret(name: String) -> Res<()> {
    settings::clear_secret(&name)
}

#[tauri::command]
fn check_now(app: AppHandle, path: State<DbPath>, source: String) -> Res<()> {
    let source = *settings::SOURCES.iter().find(|s| **s == source).ok_or(format!("Unknown source \"{source}\"."))?;
    let path = path.0.clone();
    std::thread::spawn(move || jobs::check(&app, &path, source, true));
    Ok(())
}

// ---- Shared calendar ----
// Calls that wait on Supabase run off the main thread with their own database connection, so the
// window never freezes and the shared connection is never held while waiting on the network.

fn own_conn(path: &DbPath) -> Res<Connection> {
    db::open(&path.0).map_err(err)
}

/// Tells the Calendar view, the board, and Settings to redraw.
fn calendar_changed(app: &AppHandle) {
    let _ = app.emit("calendar-changed", ());
    let _ = app.emit("settings-changed", ());
}

#[tauri::command]
fn calendar_state(db: State<Db>) -> Res<calendar::State> {
    calendar::state(&db.0.lock().unwrap()).map_err(err)
}

#[tauri::command(async)]
fn calendar_sign_in(app: AppHandle, path: State<DbPath>, email: String, password: String) -> Res<()> {
    calendar::sign_in(&own_conn(&path)?, &email, &password)?;
    jobs::check(&app, &path.0, "calendar", true);
    calendar_changed(&app);
    Ok(())
}

#[tauri::command(async)]
fn calendar_sign_out(app: AppHandle, path: State<DbPath>) -> Res<()> {
    let conn = own_conn(&path)?;
    calendar::sign_out(&conn)?;
    settings::clear_status(&conn, "calendar");
    calendar_changed(&app);
    Ok(())
}

#[tauri::command(async)]
fn calendar_set_name(app: AppHandle, path: State<DbPath>, name: String) -> Res<()> {
    calendar::set_name(&own_conn(&path)?, &name)?;
    jobs::check(&app, &path.0, "calendar", true);
    calendar_changed(&app);
    Ok(())
}

#[tauri::command(async)]
fn calendar_change_password(path: State<DbPath>, password: String) -> Res<()> {
    calendar::change_password(&own_conn(&path)?, &password)
}

#[tauri::command(async)]
fn calendar_save(app: AppHandle, path: State<DbPath>, draft: calendar::Draft) -> Res<calendar::Saved> {
    let saved = calendar::save(&own_conn(&path)?, &draft)?;
    calendar_changed(&app);
    Ok(saved)
}

#[tauri::command(async)]
fn calendar_delete(app: AppHandle, path: State<DbPath>, id: String) -> Res<()> {
    calendar::delete(&own_conn(&path)?, &id)?;
    calendar_changed(&app);
    let _ = app.emit("board-changed", ()); // a card shared as this event isn't shared anymore
    Ok(())
}

#[tauri::command]
fn calendar_set_hidden(app: AppHandle, db: State<Db>, ids: Vec<String>) -> Res<()> {
    calendar::set_hidden(&db.0.lock().unwrap(), &ids).map_err(err)?;
    let _ = app.emit("calendar-changed", ());
    Ok(())
}

#[tauri::command(async)]
fn calendar_share_card(app: AppHandle, path: State<DbPath>, id: i64) -> Res<calendar::Event> {
    let event = calendar::share_card(&own_conn(&path)?, id)?;
    calendar_changed(&app);
    let _ = app.emit("board-changed", ());
    Ok(event)
}

#[tauri::command(async)]
fn calendar_push_card(app: AppHandle, path: State<DbPath>, id: i64) -> Res<()> {
    let conn = own_conn(&path)?;
    // A card can be a shared event, a task, or neither; each push skips what doesn't apply.
    let pushed = calendar::push_card(&conn, id).and_then(|_| tasks::push_card(&conn, id));
    calendar_changed(&app);
    pushed
}

#[tauri::command(async)]
fn task_give(app: AppHandle, path: State<DbPath>, draft: tasks::Draft) -> Res<()> {
    tasks::give(&own_conn(&path)?, &draft)?;
    calendar_changed(&app);
    Ok(())
}

#[tauri::command]
fn task_seen(db: State<Db>, id: i64) -> Res<()> {
    tasks::seen(&db.0.lock().unwrap(), id).map_err(err)
}

#[tauri::command]
fn gcal_feeds() -> Vec<String> {
    gcal::labels()
}

#[tauri::command]
fn gcal_add(urls: Vec<String>) -> Res<Vec<String>> {
    gcal::add(&urls)
}

#[tauri::command]
fn gcal_remove(index: usize) -> Res<Vec<String>> {
    gcal::remove(index)
}

/// Every 2 seconds, reads new hook events and tells the window when sessions changed.
fn watch_sessions(app: AppHandle, db_path: PathBuf, events: PathBuf) {
    std::thread::spawn(move || {
        let conn = match db::open(&db_path) {
            Ok(c) => c,
            Err(e) => return eprintln!("sessions: can't open the database: {e}"),
        };
        // Old sessions are forgotten at start and then every hour, so an app left open keeps the 30-day promise.
        for tick in 0u64.. {
            if tick % 1800 == 0 {
                let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                if let Err(e) = sessions::prune(&conn, &now) {
                    eprintln!("sessions: can't forget old sessions: {e}");
                }
            }
            match sessions::read_new(&conn, &events) {
                Ok(true) => {
                    let _ = app.emit("sessions-changed", ());
                }
                Ok(false) => {}
                Err(e) => eprintln!("sessions: can't read {}: {e}", events.display()),
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

pub fn run() {
    tauri::Builder::default()
        // A second launch shows the open window and quits, so two copies never poll the same
        // sources or apply the same hook events twice. It must be the first plugin.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .setup(|app| {
            // TODO_HELPER_DB points tests and manual checks at a throwaway database.
            let path = match std::env::var_os("TODO_HELPER_DB") {
                Some(p) => p.into(),
                None => {
                    let dir = app.path().app_data_dir()?;
                    std::fs::create_dir_all(&dir)?;
                    dir.join("todo.db")
                }
            };
            app.manage(Db(Mutex::new(db::open(&path)?)));

            // TODO_HELPER_EVENTS points at a test file instead of the hooks' real one.
            let events = match std::env::var_os("TODO_HELPER_EVENTS") {
                Some(p) => p.into(),
                None => app.path().home_dir()?.join(".claude").join("todo-helper").join("events.jsonl"),
            };
            watch_sessions(app.handle().clone(), path.clone(), events);
            jobs::start(app.handle().clone(), path.clone());
            app.manage(DbPath(path));
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing hides the window at once and ends it half a second later, so a note save that
            // is still on its way lands first. Done here, not in JavaScript, so a page that failed
            // to load can never keep the window open.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                let window = window.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(500));
                    let _ = window.destroy();
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_cards,
            add_card,
            rename_card,
            delete_card,
            restore_card,
            move_card,
            set_due,
            set_deadline,
            inbox_item,
            list_notes,
            card_note,
            create_note,
            save_note,
            delete_note,
            list_sessions,
            get_settings,
            save_setting,
            set_secret,
            clear_secret,
            check_now,
            calendar_state,
            calendar_sign_in,
            calendar_sign_out,
            calendar_set_name,
            calendar_change_password,
            calendar_save,
            calendar_delete,
            calendar_set_hidden,
            calendar_share_card,
            calendar_push_card,
            task_give,
            task_seen,
            gcal_feeds,
            gcal_add,
            gcal_remove,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_trimmed_required_and_capped() {
        assert!(clean_title("").is_err());
        assert!(clean_title("   ").is_err());
        assert_eq!(clean_title(" x ").unwrap(), "x");
        assert_eq!(clean_title(&"a".repeat(600)).unwrap().chars().count(), 500);
    }

    #[test]
    fn due_dates_must_be_real_dates() {
        assert_eq!(clean_due(None).unwrap(), None);
        assert_eq!(clean_due(Some("".into())).unwrap(), None);
        assert_eq!(clean_due(Some("2026-10-23".into())).unwrap().as_deref(), Some("2026-10-23"));
        assert!(clean_due(Some("2026-02-30".into())).is_err());
        assert!(clean_due(Some("23/10/2026".into())).is_err());
    }
}
