# todo-helper

A desktop Kanban board that fills itself. It collects todos from Outlook, Discord, and itslearning, shows your running Claude Code sessions, keeps notes, and has a calendar you can share with a few people.

Runs on Windows and macOS. Everything stays on your computer in one SQLite file, except the optional shared calendar.

## What it does

- **Board**: Inbox, the next four weeks by day, Doing, and Done. Cards from sources land in Inbox; cards due today land in Today.
- **Sources**: new Outlook mail and Discord messages go through Claude Code once, which turns them into card suggestions. itslearning hand-in deadlines come from its calendar feed.
- **Sessions**: a live list of your Claude Code sessions and the agents they run, through a Claude Code hook.
- **Calendar**: your itslearning and Google or iCloud calendars, plus an optional shared calendar on your own Supabase project.
- **Notes**: plain-text notes that save on every keystroke, and can link to a card.

## Download

Get the installer from the [latest release](https://github.com/VLJardim/todo-helper/releases/latest):

- **Windows**: `todo-helper_..._x64-setup.exe`
- **macOS** (Apple silicon and Intel): `todo-helper_..._universal.dmg`

The installers aren't code-signed, so your computer warns you the first time. On Windows, select **More info**, then **Run anyway**. On macOS, open **System Settings**, then **Privacy & Security**, and select **Open Anyway**.

## Get started

After you install the app, connect your sources with [SETUP.md](SETUP.md), from section 2. Each section takes 2 to 10 minutes and ends with a check. Skip any source you don't use.

The email and Discord sources need Claude Code. Outlook works only with classic Outlook on Windows.

## Build it yourself

You don't have to use the installer. You can always download the whole project and build the app on your own computer, so you can read every line of code that runs.

1. Get the code: `git clone https://github.com/VLJardim/todo-helper.git`, or select **Code**, then **Download ZIP** at the top of this page.
2. Install Node.js 22 or later and Rust.
3. Follow [SETUP.md](SETUP.md) section 1. The build takes about 10 minutes.

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
