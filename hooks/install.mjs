// Adds the todo-helper hooks to your user Claude Code settings (%USERPROFILE%\.claude\settings.json),
// keeping every other hook and setting. Run it yourself: node hooks/install.mjs
// Running it again replaces its own entries, so run it again after moving this folder.
// node hooks/install.mjs --uninstall removes them. It saves a backup before writing, and stops
// without writing if settings.json isn't valid JSON.
// node hooks/install.mjs --print prints the hooks block without writing anything.
import { copyFileSync, existsSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const script = join(here, "log-event.mjs").replaceAll("\\", "/");
const settingsPath = join(homedir(), ".claude", "settings.json");
const command = { type: "command", command: `node "${script}"`, timeout: 5 };
// Tool events log only Agent launches. Stop is logged so a session that works for hours without agents still counts as active.
const events = ["SessionStart", "SessionEnd", "UserPromptSubmit", "PreToolUse", "PostToolUse", "SubagentStart", "SubagentStop", "Stop"];
const snippet = { hooks: Object.fromEntries(events.map((e) => [e, [e.endsWith("ToolUse") ? { matcher: "Agent", hooks: [command] } : { hooks: [command] }]])) };
if (process.argv.includes("--print")) { console.log(JSON.stringify(snippet, null, 2)); process.exit(0); }
const uninstall = process.argv.includes("--uninstall");

let settings = {};
if (existsSync(settingsPath)) {
  try {
    // PowerShell 5.1 saves UTF-8 with a byte order mark, which JSON.parse rejects.
    settings = JSON.parse(readFileSync(settingsPath, "utf8").replace(/^﻿/, ""));
  } catch (e) {
    console.error(`${settingsPath} isn't valid JSON (${e.message}). Nothing was changed. Fix the file, then run this again.`);
    process.exit(1);
  }
}

// Our entries run hooks/log-event.mjs, from wherever this folder was when they were added.
const ours = (group) => group.hooks?.some((h) => h.command?.endsWith('hooks/log-event.mjs"'));
const before = JSON.stringify(settings);
settings.hooks ??= {};
for (const [event, groups] of Object.entries(snippet.hooks)) {
  const kept = (settings.hooks[event] ?? []).filter((g) => !ours(g));
  const list = uninstall ? kept : [...kept, ...groups];
  if (list.length) settings.hooks[event] = list;
  else delete settings.hooks[event];
}
if (Object.keys(settings.hooks).length === 0) delete settings.hooks;

if (JSON.stringify(settings) === before) {
  console.log(`The todo-helper hooks are ${uninstall ? "not" : "already"} installed. Nothing changed.`);
  process.exit(0);
}
const backup = `${settingsPath}.before-todo-helper`;
if (existsSync(settingsPath)) copyFileSync(settingsPath, backup);
// Write a copy, then swap it in, so a failed write never leaves settings.json half written.
writeFileSync(`${settingsPath}.tmp`, JSON.stringify(settings, null, 2) + "\n");
renameSync(`${settingsPath}.tmp`, settingsPath);
console.log(`${uninstall ? "Removed the todo-helper hooks from" : "Added the todo-helper hooks to"} ${settingsPath}.`);
if (existsSync(backup)) console.log(`Your previous file is saved as ${backup}.`);
