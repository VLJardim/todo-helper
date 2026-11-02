# todo-helper

A desktop Kanban board that fills itself. It collects todos from Outlook, Discord, and itslearning, shows your running Claude Code sessions, keeps notes, and has a calendar you can share with a few people.

Runs on Windows and macOS. Everything stays on your computer in one SQLite file, except the optional shared calendar.

## What it does

- **Board**: Inbox, the next four weeks by day, Doing, and Done. Cards from sources land in Inbox; cards due today land in Today.
- **Sources**: new Outlook mail and Discord messages go through Claude Code once, which turns them into card suggestions. itslearning hand-in deadlines come from its calendar feed.
- **Sessions**: a live list of your Claude Code sessions and the agents they run, through a Claude Code hook.
- **Calendar**: your itslearning and Google or iCloud calendars, plus an optional shared calendar on your own Supabase project.
- **Notes**: plain-text notes that save on every keystroke, and can link to a card.

## Get started

Follow [SETUP.md](SETUP.md). Section 1 builds and installs the app in about 10 minutes; each later section connects one source and ends with a check. Skip any source you don't use.

You need Node.js 22 or later, Rust, and Claude Code for the email and Discord sources. Outlook works only with classic Outlook on Windows.

## Stack

| Part | Tech |
|---|---|
| App shell | [Tauri 2](https://tauri.app) |
| Backend | Rust, SQLite (`rusqlite`) |
| Interface | TypeScript, Tailwind CSS, Vite |
| Shared calendar | Supabase (Postgres with row-level security) |
| Todo extraction | Claude Code (`claude -p`) |

Secrets (bot tokens, feed links, sign-ins) go to Windows Credential Manager or the macOS Keychain, never to the database or the repository.

## Run it in development

```bash
npm ci
npm run tauri dev
```

Run the tests with `cargo test` in `src-tauri/`.

## How the code is organized

Each folder has a `CONTEXT.md` that states its job and lists its files. Start at [CLAUDE.md](CLAUDE.md) for the map. Read [DESIGN.md](DESIGN.md) before you change the interface.

## Contributing

Issues and pull requests are welcome. Before you open a pull request, run `cargo test` in `src-tauri/` and `npm run build`.

## License

[MIT](LICENSE)
