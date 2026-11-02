# supabase — the shared calendar's database

One job: define the shared calendar's tables and rules in the `todo_helper` schema of the calendar owner's Supabase project, and check that the rules hold. The schema keeps them apart from the project's other tables; the app picks it with the Accept-Profile and Content-Profile headers.

## What's here
- `schema.sql`: the `profiles`, `events`, and `event_log` tables, plus `tasks` and `task_log` for tasks members give each other (same rules, `log_task` trigger), the row-level security rules, and two triggers. `stamp_event` sets `created_by` and `updated_by` from the signed-in member, so the app can't fake them. `log_event` writes every add, change, and delete to `event_log`, with the event before and after. Safe to run again.
- `check.sql`: acts as two made-up members and as someone who isn't signed in, stops with "FAILED: ..." when a rule lets too much or too little through, and removes what it made. Ends with "All checks passed".

## Rules
- Who may do what: someone who isn't signed in gets nothing; a member reads everything, adds events, changes and deletes any event, and changes only their own name; nobody writes to `event_log` except the trigger.
- Sign-up stays off in the dashboard. The owner adds members under Authentication, Users (see `SETUP.md`, section 7).
- A schema change goes into `schema.sql` so it can run again on the live project, and gets a matching check in `check.sql`.
- The app only talks to Supabase from Rust (`src-tauri/src/calendar.rs`), with the publishable key and the member's own sign-in. The secret key never leaves the dashboard.

## Check
- In the SQL editor (or locally with `supabase start` and `psql`), run `schema.sql`, then `check.sql`, and the result is "All checks passed".
- Without Docker: PGlite (Postgres in Node) with a stand-in `auth` schema runs both files too; that's how the task rules were checked on 2026-10-27.
