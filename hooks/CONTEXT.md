# hooks — Claude Code hooks that feed the Sessions panel

One job: turn Claude Code events into lines in `%USERPROFILE%\.claude\todo-helper\events.jsonl`, which the app reads.

## What's here
- `log-event.mjs`: the hook command. Reads one event from stdin, keeps only the fields `sessions.rs` reads (an Agent call's description and type, never another tool's input), cuts strings to 500 characters, and appends it as one JSON line with a `ts` timestamp. It always exits 0, so a logging problem never blocks Claude Code. `TODO_HELPER_EVENTS` overrides the output path.
- `install.mjs`: adds the 8 hook entries it defines (an Agent-only matcher on PreToolUse and PostToolUse; `Stop` is logged too, so a session that works for hours without launching agents still counts as active). Merges them into your user settings, keeps everything already there, saves a backup first, and refuses a settings file that isn't valid JSON. Running it again replaces its own entries (after a folder move), and `--uninstall` removes them.

## Rules
- Installing the hooks changes your global Claude Code settings, so it is a setup step you do yourself (see `SETUP.md`), never something the app or an agent does unasked.
- Requires Node.js on the PATH.

## Check
- Run `node hooks/install.mjs --print > test-settings.json`, then one `claude -p` session with `--settings test-settings.json` and `TODO_HELPER_EVENTS` set to a test file, and read the lines it writes.
