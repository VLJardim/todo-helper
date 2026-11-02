# src-tauri — the Rust backend

One job: own the SQLite database and the background jobs, and expose them to the UI as Tauri commands.

## What's here
- `src/lib.rs`: builds the Tauri app, opens the database, registers the commands, and delays the real close by 0.5 seconds so the last note save lands.
- `src/db.rs`: the schema (`MIGRATIONS`, applied in order and tracked in `PRAGMA user_version`) and the queries for cards and notes. A note belongs to at most one card and outlives it.
- `src/sessions.rs`: reads new lines from the hooks' `events.jsonl` every 2 seconds (`read_new`, which remembers its byte offset in `settings`), turns them into sessions and agents (`apply_line`), and lists them for the UI. A session with no events for 2 hours is stale.
- `tests/sample-events.jsonl`: real hook events from two sessions, used by the tests.
- `src/settings.rs`: secrets in the system's password store (Windows: generic credentials named `todo-helper:<name>`; macOS: Keychain items for the service `todo-helper`), plain settings in the `settings` table with validation, and each source's last status line.
- `src/jobs.rs`: one thread per source (outlook on Windows only, discord, itslearning, and calendar every minute) that checks on the interval from Settings, plus `check` for Check now. Each check writes the source's status line.
- `src/outlook.rs` and `src/outlook.ps1`: reads new Inbox mail from every account in the running classic Outlook through its COM interface (first run: the last 3 days), read-only, and stores it in `inbox_items`.
- `src/extract.rs`: sends unprocessed `inbox_items` to a locked-down `claude -p` (Haiku, no tools, no MCP, no user settings, JSON schema) and adds the cards it suggests. An item that fails 3 times is skipped.
- `tests/sample-claude-reply.json`: a real reply from that call.
- `src/calendar.rs`: the shared calendar in the owner's Supabase project (see `supabase/`). Sign-in with email and password: the refresh token goes to the password store, the access token stays in memory and is renewed a minute before it runs out. A sync every minute (`jobs.rs`) replaces this computer's copy in `shared_members`, `shared_events` (4 weeks back to 26 weeks ahead), and `shared_log` (the newest 50 changes). Saving a change only lands on the version the form opened (`updated_at`); otherwise the newer version comes back as a conflict. A card shared to the calendar (`cards.shared_event_id`) pushes its title and date to its event.
- `src/tasks.rs`: tasks members give each other (`supabase/schema.sql`, `tasks`). The calendar's minute sync also reads every task into `shared_tasks`; a task given to you becomes a card (`cards.task_id`, marked `task_new` until opened) whose title, deadline, and done follow Supabase. `synced_events` with uid `task:<id>` stops a card you deleted from coming back. A task card's title, date, and Done go back through `calendar_push_card`. A project without the tasks table keeps the calendar working.
- `src/gcal.rs`: copies Google calendars (secret `gcal_feed_urls`, addresses separated by spaces) into the shared calendar for the next 26 weeks under your sign-in, every `gcal_interval_min` (default 720). The same rules as the script it replaced (`scripts/sync_gcal.py`, removed 2026-10-27): only copies whose note says `gcal:<key>` and that you added are changed or removed, private events aren't copied, nothing already started is removed, and an empty feed changes nothing. Repeats (DAILY, WEEKLY with BYDAY, MONTHLY, YEARLY, with EXDATE and moved times) are expanded; Central European time comes from the EU rule.
- `tests/sample-supabase-sign-in.json`: a real sign-in reply from Supabase, with the tokens replaced.
- `src/itslearning.rs`: downloads the iCal feed (webcal links become https), parses its events, and keeps one card per event in the next 14 days, matched by UID. A timed event also stores its start and end on the card (`starts_at`, `ends_at`). A moved event updates its card's date, and new times on the same day update only the times, so a date you set stays; a card the owner deleted is not added again (`synced_events`).
- `src/discord.rs`: reads the chosen channels through the Discord REST API with the bot token, newer than the last message id per channel (first check: the last 2 days), and stores messages in `inbox_items`. Errors name the missing permission.
- `tauri.conf.json`: window, bundle, and security settings. `tauri.macos.conf.json` replaces the bundle targets and icons on macOS.
- `capabilities/default.json`: what the window is allowed to call.

## Rules
- The database is `todo.db` in the app data folder. Set `TODO_HELPER_DB` to a file path to use a throwaway database instead, and `TODO_HELPER_EVENTS` to read a test events file.
- Only one copy runs at a time (`tauri-plugin-single-instance`): a second launch shows the open window and quits. The dev build and the installed app count as the same app, so close the installed app before `npm run tauri dev`.
- A schema change is a new entry at the end of `MIGRATIONS`. Never edit an entry that has shipped.
- Only `calendar.rs`, `tasks.rs`, and `gcal.rs` talk to Supabase. Commands that wait on it are `#[tauri::command(async)]` and open their own connection, so the window never freezes.
- Text from emails and messages is untrusted. It only ever reaches Claude through `extract.rs`, never with tools.
- Secrets go to the system's password store, never to SQLite or the repository.
- The app builds on Windows and macOS. Windows-only code sits behind `#[cfg(windows)]`; Outlook is off on macOS (`settings::OUTLOOK_WORKS`).
- Background jobs never hold the database while they wait on the network. Each job thread opens its own connection.

## Check
- `cargo test --manifest-path src-tauri/Cargo.toml` passes.
- Against a Supabase project with `supabase/schema.sql` and two members: `cargo test -- --ignored calendar_live --test-threads 1` with `TODO_HELPER_SUPABASE_URL`, `TODO_HELPER_SUPABASE_KEY`, and `TODO_HELPER_SUPABASE_A`/`_B` set to "email password". It signs out the calendar on this computer.
