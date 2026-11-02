//! Reads Claude Code hook events from events.jsonl into the sessions and agents tables.
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Applies one event line. Lines that aren't JSON or have no session are skipped.
pub fn apply_line(conn: &Connection, line: &str) -> rusqlite::Result<()> {
    let Ok(e) = serde_json::from_str::<Value>(line) else { return Ok(()) };
    let Some(sid) = e["session_id"].as_str() else { return Ok(()) };
    let ts = e["ts"].as_str().unwrap_or_default();
    let is_agent_tool = e["tool_name"] == "Agent";

    // Any event keeps its session alive, even one that started before logging did.
    conn.execute(
        "INSERT INTO sessions (id, cwd, started_at, last_event_at) VALUES (?1, ?2, ?3, ?3)
         ON CONFLICT(id) DO UPDATE SET last_event_at = MAX(last_event_at, excluded.last_event_at),
                                       cwd = COALESCE(sessions.cwd, excluded.cwd)",
        params![sid, e["cwd"].as_str(), ts],
    )?;

    match e["hook_event_name"].as_str().unwrap_or_default() {
        "SessionStart" => {
            conn.execute("UPDATE sessions SET ended_at = NULL WHERE id = ?1", [sid])?;
        }
        "UserPromptSubmit" => {
            // Agent hand-backs, notifications, and slash-command wrappers arrive as prompts too.
            const INJECTED: [&str; 5] = ["<agent-message", "<task-notification", "<command-", "<local-command", "<system-reminder"];
            if let Some(p) = e["prompt"].as_str().filter(|p| !INJECTED.iter().any(|x| p.trim_start().starts_with(x))) {
                conn.execute(
                    "UPDATE sessions SET first_prompt = ?2 WHERE id = ?1 AND first_prompt IS NULL",
                    params![sid, p],
                )?;
            }
        }
        "PreToolUse" if is_agent_tool => {
            conn.execute(
                "INSERT OR IGNORE INTO pending_agents (tool_use_id, session_id, description, subagent_type, ts)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    e["tool_use_id"].as_str(),
                    sid,
                    e["tool_input"]["description"].as_str(),
                    e["tool_input"]["subagent_type"].as_str(),
                    ts
                ],
            )?;
        }
        "SubagentStart" => {
            let (Some(id), kind) = (e["agent_id"].as_str(), e["agent_type"].as_str()) else { return Ok(()) };
            conn.execute(
                "INSERT INTO agents (id, session_id, agent_type, started_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET agent_type = COALESCE(agents.agent_type, excluded.agent_type)",
                params![id, sid, kind, ts],
            )?;
            // PostToolUse may have named this agent already: hooks run as separate processes, so
            // their lines can land in either order.
            let named: bool = conn.query_row("SELECT description IS NOT NULL FROM agents WHERE id = ?1", [id], |r| r.get(0))?;
            if named {
                return Ok(());
            }
            // SubagentStart has no description. Take it from the oldest launch in this session from the
            // last 30 seconds (an agent starts about a second after its launch; an older launch was
            // denied or failed), preferring the same agent type.
            let pending: Option<(String, Option<String>)> = conn
                .query_row(
                    "SELECT tool_use_id, description FROM pending_agents WHERE session_id = ?1 AND ts >= ?3
                     ORDER BY subagent_type IS ?2 DESC, ts LIMIT 1",
                    params![sid, kind, seconds_before(ts, 30)],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((tool_use_id, description)) = pending {
                conn.execute("UPDATE agents SET description = ?2 WHERE id = ?1", params![id, description])?;
                conn.execute("DELETE FROM pending_agents WHERE tool_use_id = ?1", [tool_use_id])?;
            }
        }
        "PostToolUse" if is_agent_tool => {
            let r = &e["tool_response"];
            // A background agent's PostToolUse names it right after launch. It can arrive before
            // SubagentStart, so create the agent here when needed; this description always wins.
            if let Some(id) = r["agentId"].as_str() {
                let description = r["description"].as_str().or(e["tool_input"]["description"].as_str());
                conn.execute(
                    "INSERT INTO agents (id, session_id, description, agent_type, started_at) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(id) DO UPDATE SET description = COALESCE(excluded.description, agents.description)",
                    params![id, sid, description, e["tool_input"]["subagent_type"].as_str(), ts],
                )?;
            }
            conn.execute("DELETE FROM pending_agents WHERE tool_use_id = ?1", params![e["tool_use_id"].as_str()])?;
        }
        "SubagentStop" => {
            conn.execute(
                "UPDATE agents SET ended_at = ?2 WHERE id = ?1 AND ended_at IS NULL",
                params![e["agent_id"].as_str(), ts],
            )?;
        }
        "SessionEnd" => {
            conn.execute("UPDATE sessions SET ended_at = ?2 WHERE id = ?1", params![sid, ts])?;
            conn.execute("UPDATE agents SET ended_at = ?2 WHERE session_id = ?1 AND ended_at IS NULL", params![sid, ts])?;
            conn.execute("DELETE FROM pending_agents WHERE session_id = ?1", [sid])?;
        }
        _ => {}
    }
    Ok(())
}

/// Forgets sessions (and their agents) with no events for 30 days. Their prompts are personal data.
pub fn prune(conn: &Connection, now: &str) -> rusqlite::Result<usize> {
    let before = seconds_before(now, 30 * 24 * 3600);
    conn.execute("DELETE FROM agents WHERE session_id IN (SELECT id FROM sessions WHERE last_event_at < ?1)", [&before])?;
    conn.execute("DELETE FROM pending_agents WHERE ts < ?1", [&before])?;
    conn.execute("DELETE FROM sessions WHERE last_event_at < ?1", [&before])
}

/// The timestamp `secs` seconds before `ts`, in the same format, or "" when `ts` doesn't parse.
fn seconds_before(ts: &str, secs: i64) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|t| (t - chrono::Duration::seconds(secs)).to_utc().to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

/// Reads the complete lines added to `path` since the last call and applies them.
/// Returns true when anything changed. A new file (another creation time) or a shorter one
/// means it was replaced, so reading starts over.
// ponytail: the events file only grows (about 1 KB per event); rotate it if it ever gets large.
pub const TAIL: u64 = 5 * 1024 * 1024;

pub fn read_new(conn: &Connection, path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let Ok(mut file) = File::open(path) else { return Ok(false) };
    let meta = file.metadata()?;
    let len = meta.len();
    let file_id = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_default();
    let mut offset: u64 = crate::db::get_setting(conn, "events_offset")?.and_then(|v| v.parse().ok()).unwrap_or(0);
    if len < offset || crate::db::get_setting(conn, "events_file_id")?.is_some_and(|id| id != file_id) {
        offset = 0;
    }
    // Starting over on a big file reads only its last 5 MB; the first line there is cut, so it's skipped.
    // ponytail: a session that started before those 5 MB shows without its first prompt.
    let from_tail = offset == 0 && len > TAIL;
    if from_tail {
        offset = len - TAIL;
    }
    if len == offset {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    file.take(len - offset).read_to_end(&mut buf)?;
    // A line still being written has no newline yet; leave it for the next read.
    let Some(end) = buf.iter().rposition(|&b| b == b'\n') else { return Ok(false) };
    let start = if from_tail { buf[..end].iter().position(|&b| b == b'\n').map_or(end, |i| i + 1) } else { 0 };

    let tx = conn.unchecked_transaction()?;
    for line in String::from_utf8_lossy(&buf[start..end]).lines() {
        apply_line(&tx, line)?;
    }
    crate::db::set_setting(&tx, "events_offset", &(offset + end as u64 + 1).to_string())?;
    crate::db::set_setting(&tx, "events_file_id", &file_id)?;
    tx.commit()?;
    Ok(true)
}

#[derive(Serialize)]
pub struct Agent {
    pub description: Option<String>,
    pub agent_type: Option<String>,
    pub started_at: String,
}

#[derive(Serialize)]
pub struct Session {
    pub id: String,
    pub cwd: Option<String>,
    pub first_prompt: Option<String>,
    pub last_event_at: String,
    pub ended_at: Option<String>,
    /// No events for 2 hours and no SessionEnd.
    pub stale: bool,
    pub agents_total: i64,
    /// Agents still running. Empty for ended and stale sessions.
    pub running: Vec<Agent>,
}

/// Every session, newest activity first. `now` is an RFC 3339 UTC timestamp like the ones in the file.
pub fn list(conn: &Connection, now: &str) -> rusqlite::Result<Vec<Session>> {
    let stale_before = seconds_before(now, 2 * 3600);
    let mut stmt = conn.prepare(
        "SELECT s.id, s.cwd, s.first_prompt, s.last_event_at, s.ended_at,
                (SELECT COUNT(*) FROM agents a WHERE a.session_id = s.id)
         FROM sessions s ORDER BY s.last_event_at DESC LIMIT 100",
    )?;
    let mut sessions: Vec<Session> = stmt
        .query_map([], |r| {
            let ended_at: Option<String> = r.get(4)?;
            let last_event_at: String = r.get(3)?;
            Ok(Session {
                id: r.get(0)?,
                cwd: r.get(1)?,
                first_prompt: r.get(2)?,
                stale: ended_at.is_none() && last_event_at < stale_before,
                last_event_at,
                ended_at,
                agents_total: r.get(5)?,
                running: vec![],
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut agents = conn.prepare(
        "SELECT description, agent_type, started_at FROM agents
         WHERE session_id = ?1 AND ended_at IS NULL ORDER BY started_at",
    )?;
    for s in sessions.iter_mut().filter(|s| s.ended_at.is_none() && !s.stale) {
        s.running = agents
            .query_map([&s.id], |r| {
                Ok(Agent { description: r.get(0)?, agent_type: r.get(1)?, started_at: r.get(2)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
    }
    Ok(sessions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use serde_json::json;

    const SAMPLE: &str = include_str!("../tests/sample-events.jsonl");
    const NOW: &str = "2026-10-18T19:00:00.000Z";

    fn running(conn: &Connection, sid: &str) -> Vec<String> {
        let s = list(conn, NOW).unwrap().into_iter().find(|s| s.id == sid).unwrap();
        s.running.into_iter().map(|a| a.description.unwrap_or_default()).collect()
    }

    #[test]
    fn sample_events_give_the_right_sessions_and_agents() {
        let conn = db::open(&db::temp_path("sessions")).unwrap();
        let lines: Vec<&str> = SAMPLE.lines().collect();
        let with_agents = "1b6dd8a2-67bc-4f97-8ac8-86d9d8700dc0";

        // Up to both launches: two agents running, each with its own description.
        for l in &lines[..11] {
            apply_line(&conn, l).unwrap();
        }
        assert_eq!(running(&conn, with_agents), vec!["List top-level files", "Count text files"]);
        let sessions = list(&conn, NOW).unwrap();
        assert_eq!(sessions.len(), 2);
        let s = sessions.iter().find(|s| s.id == with_agents).unwrap();
        assert!(s.first_prompt.as_deref().unwrap().starts_with("Use the Agent tool"));
        assert_eq!(s.cwd.as_deref(), Some(r"C:\Users\me\school\essay"));

        // The first agent stops; the hand-back prompt does not replace the first prompt.
        for l in &lines[11..13] {
            apply_line(&conn, l).unwrap();
        }
        assert_eq!(running(&conn, with_agents), vec!["Count text files"]);

        for l in &lines[13..] {
            apply_line(&conn, l).unwrap();
        }
        let sessions = list(&conn, NOW).unwrap();
        assert!(sessions.iter().all(|s| s.ended_at.is_some() && s.running.is_empty()));
        let s = sessions.iter().find(|s| s.id == with_agents).unwrap();
        assert_eq!(s.agents_total, 2);
        assert!(s.first_prompt.as_deref().unwrap().starts_with("Use the Agent tool"));
    }

    #[test]
    fn descriptions_survive_hook_lines_arriving_out_of_order() {
        let conn = db::open(&db::temp_path("order")).unwrap();
        let lines: Vec<&str> = SAMPLE.lines().collect();
        let with_agents = "1b6dd8a2-67bc-4f97-8ac8-86d9d8700dc0";
        // Swap each SubagentStart with the PostToolUse that follows it (lines 7/8 and 9/10).
        let order = [0, 1, 2, 3, 4, 5, 6, 8, 7, 10, 9];
        for i in order {
            apply_line(&conn, lines[i]).unwrap();
        }
        assert_eq!(running(&conn, with_agents), vec!["List top-level files", "Count text files"]);
    }

    #[test]
    fn a_launch_that_never_started_does_not_name_the_next_agent() {
        let conn = db::open(&db::temp_path("denied")).unwrap();
        let s = "s1";
        let pre = |t: &str, id: &str, d: &str| {
            json!({"ts": t, "session_id": s, "hook_event_name": "PreToolUse", "tool_name": "Agent", "tool_use_id": id,
                   "tool_input": {"description": d, "subagent_type": "general-purpose"}}).to_string()
        };
        apply_line(&conn, &pre("2026-10-18T18:00:00.000Z", "t1", "Denied launch")).unwrap();
        apply_line(&conn, &pre("2026-10-18T18:02:00.000Z", "t2", "Real work")).unwrap();
        apply_line(&conn, &json!({"ts": "2026-10-18T18:02:01.000Z", "session_id": s, "hook_event_name": "SubagentStart",
                                  "agent_id": "f1", "agent_type": "general-purpose"}).to_string()).unwrap();
        assert_eq!(running(&conn, s), vec!["Real work"]);
    }

    #[test]
    fn injected_prompts_never_become_the_first_prompt() {
        let conn = db::open(&db::temp_path("prompts")).unwrap();
        let prompt = |p: &str| json!({"ts": "2026-10-18T18:59:00.000Z", "session_id": "s", "hook_event_name": "UserPromptSubmit", "prompt": p}).to_string();
        apply_line(&conn, &prompt("<task-notification>done</task-notification>")).unwrap();
        apply_line(&conn, &prompt("<div> is my favourite tag, explain it")).unwrap();
        apply_line(&conn, &prompt("second real prompt")).unwrap();
        assert_eq!(list(&conn, NOW).unwrap()[0].first_prompt.as_deref(), Some("<div> is my favourite tag, explain it"));
    }

    #[test]
    fn a_shorter_events_file_is_read_from_the_start() {
        let conn = db::open(&db::temp_path("shrink")).unwrap();
        let path = db::temp_path("events-shrink").with_extension("jsonl");
        let lines: Vec<&str> = SAMPLE.lines().collect();
        std::fs::write(&path, format!("{}\n{}\n", lines[0], lines[2])).unwrap();
        read_new(&conn, &path).unwrap();
        // The file is replaced by a shorter one holding only the other session's start.
        std::fs::write(&path, format!("{}\n", lines[1])).unwrap();
        assert!(read_new(&conn, &path).unwrap());
        assert_eq!(list(&conn, NOW).unwrap().len(), 2);
    }

    #[test]
    fn a_big_events_file_is_read_from_its_last_5_mb() {
        let conn = db::open(&db::temp_path("tail")).unwrap();
        let path = db::temp_path("events-tail").with_extension("jsonl");
        let lines: Vec<&str> = SAMPLE.lines().collect();
        // An old session's start, filler past 5 MB, then a new session's start.
        let filler = format!("{}\n", r#"{"pad":"x"}"#).repeat((TAIL as usize) / 12 + 1);
        std::fs::write(&path, format!("{}\n{filler}{}\n", lines[0], lines[1])).unwrap();
        assert!(read_new(&conn, &path).unwrap());
        let ids: Vec<String> = list(&conn, NOW).unwrap().into_iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), 1, "only the session inside the last 5 MB: {ids:?}");
    }

    #[test]
    fn sessions_quiet_for_30_days_are_forgotten() {
        let conn = db::open(&db::temp_path("prune")).unwrap();
        for l in SAMPLE.lines() {
            apply_line(&conn, l).unwrap();
        }
        assert_eq!(prune(&conn, "2026-11-01T00:00:00.000Z").unwrap(), 0);
        assert_eq!(prune(&conn, "2026-12-01T00:00:00.000Z").unwrap(), 2);
        let agents: i64 = conn.query_row("SELECT COUNT(*) FROM agents", [], |r| r.get(0)).unwrap();
        assert_eq!(agents, 0);
    }

    #[test]
    fn quiet_sessions_turn_stale_after_two_hours() {
        let conn = db::open(&db::temp_path("stale")).unwrap();
        apply_line(&conn, SAMPLE.lines().next().unwrap()).unwrap();
        assert!(!list(&conn, NOW).unwrap()[0].stale);
        assert!(list(&conn, "2026-10-18T21:00:00.000Z").unwrap()[0].stale);
    }

    #[test]
    fn reader_applies_only_complete_lines_and_resumes() {
        let conn = db::open(&db::temp_path("reader")).unwrap();
        let path = db::temp_path("events").with_extension("jsonl");
        let lines: Vec<&str> = SAMPLE.lines().collect();

        // Two full lines and half of the third.
        let partial = format!("{}\n{}\n{}", lines[0], lines[1], &lines[2][..40]);
        std::fs::write(&path, &partial).unwrap();
        assert!(read_new(&conn, &path).unwrap());
        assert_eq!(list(&conn, NOW).unwrap().len(), 2);
        assert!(list(&conn, NOW).unwrap().iter().all(|s| s.first_prompt.is_none()));

        // The rest of the third line arrives.
        std::fs::write(&path, format!("{}\n{}\n{}\n", lines[0], lines[1], lines[2])).unwrap();
        assert!(read_new(&conn, &path).unwrap());
        assert!(!read_new(&conn, &path).unwrap(), "nothing new");
        assert_eq!(list(&conn, NOW).unwrap().iter().filter(|s| s.first_prompt.is_some()).count(), 1);
    }
}
