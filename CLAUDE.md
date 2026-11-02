# todo-helper

A desktop Kanban app for Windows and macOS (Tauri 2, SQLite, Tailwind) that collects todos from Outlook, Discord, and itslearning, shows live Claude Code sessions, keeps notes, and has a calendar shared with a few people through Supabase.

Built on ICM: folders carry order, each working folder's `CONTEXT.md` states its job, and files carry state. If something needs explaining, it goes in that folder's `CONTEXT.md`, not here.

## Where things live

| Path | What it holds |
|---|---|
| `.claude/skills/` | project skill: `icm-architect` |
| `src/` | the user interface (TypeScript, Tailwind). Start at `src/CONTEXT.md` |
| `src-tauri/` | the Rust backend: database, background jobs, commands. Start at `src-tauri/CONTEXT.md` |
| `hooks/` | the Claude Code hook that logs session events for the Sessions panel. Start at `hooks/CONTEXT.md` |
| `supabase/` | the shared calendar's tables, rules, and their check. Start at `supabase/CONTEXT.md` |
| `SETUP.md` | how to install the app and connect each source, with a check per section |
| `PRODUCT.md`, `DESIGN.md` | who the app is for, and the visual system (tokens, rules). Read `DESIGN.md` before any UI change |
| `.impeccable/` | `design.json` (DESIGN.md's sidecar) and `review/audit.md`, the latest audit |

## Rules

- Load only what the change needs: the folder's `CONTEXT.md`, its files, and the decisions it depends on.
- Secrets never go in the repository or in SQLite.
- Run `cargo test` in `src-tauri/` and `npm run build` before you open a pull request.
