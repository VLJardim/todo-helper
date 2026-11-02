// Claude Code hook. Appends the event JSON it receives on stdin to one line of
// %USERPROFILE%\.claude\todo-helper\events.jsonl, with a timestamp, for the app to read.
// TODO_HELPER_EVENTS overrides the file path (used for tests).
import { appendFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

const file = process.env.TODO_HELPER_EVENTS || join(homedir(), ".claude", "todo-helper", "events.jsonl");

// Prompts and agent results can be long. The app needs only the start of them.
const trim = (v) =>
  typeof v === "string" ? v.slice(0, 500).replace(/[\ud800-\udbff]$/, "")
  : Array.isArray(v) ? v.slice(0, 20).map(trim)
  : v && typeof v === "object" ? Object.fromEntries(Object.entries(v).map(([k, x]) => [k, trim(x)]))
  : v;

// Only the fields the app's sessions.rs reads. Other tools' input (commands, file contents) is never stored.
const keep = (e) => {
  const agent = e.tool_name === "Agent";
  return {
    session_id: e.session_id,
    cwd: e.cwd,
    hook_event_name: e.hook_event_name,
    prompt: e.prompt,
    tool_name: e.tool_name,
    tool_use_id: e.tool_use_id,
    agent_id: e.agent_id,
    agent_type: e.agent_type,
    tool_input: agent ? { description: e.tool_input?.description, subagent_type: e.tool_input?.subagent_type } : undefined,
    tool_response: agent ? { agentId: e.tool_response?.agentId, description: e.tool_response?.description } : undefined,
  };
};

let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => (input += chunk));
process.stdin.on("end", () => {
  try {
    const event = trim(keep(JSON.parse(input)));
    mkdirSync(dirname(file), { recursive: true });
    // One write call per line, so lines from parallel sessions never interleave.
    appendFileSync(file, JSON.stringify({ ...event, ts: new Date().toISOString() }) + "\n");
  } catch {
    // Never block Claude Code because logging failed.
  }
});
