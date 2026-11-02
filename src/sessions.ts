import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { h, redraw } from "./dom";
import { icon } from "./icons";

type Agent = { description: string | null; agent_type: string | null; started_at: string };
type Session = {
  cwd: string | null;
  first_prompt: string | null;
  last_event_at: string;
  ended_at: string | null;
  stale: boolean;
  agents_total: number;
  running: Agent[];
};

let root: HTMLElement;

export function mountSessions(el: HTMLElement) {
  root = el;
  return render();
}

// New hook events re-render the open view; the timer keeps "5 min ago" and staleness current.
listen("sessions-changed", () => root?.isConnected && render());
setInterval(() => root?.isConnected && render(), 30_000);

function ago(iso: string) {
  const min = Math.round((Date.now() - Date.parse(iso)) / 60_000);
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  const hours = Math.round(min / 60);
  if (hours < 24) return `${hours} h ago`;
  return new Date(iso).toLocaleDateString("en-GB", { day: "numeric", month: "short" });
}

function folder(cwd: string | null) {
  return cwd?.split(/[\\/]/).filter(Boolean).pop() ?? "Unknown folder";
}

function plural(n: number, word: string) {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

async function render() {
  const all = await invoke<Session[]>("list_sessions");
  const active = all.filter((s) => !s.ended_at && !s.stale);
  const ended = all.filter((s) => s.ended_at || s.stale).slice(0, 20);

  redraw(
    root,
    h(
      "div",
      { class: "h-full overflow-y-auto" },
      h(
        "div",
        { class: "mx-auto flex max-w-3xl flex-col gap-8 p-6" },
        h(
          "header",
          {},
          h("h2", { class: "text-lg font-semibold" }, "Claude sessions"),
          h(
            "p",
            { class: "text-sm text-ink-2" },
            active.length ? `${plural(active.length, "session")} running on this computer.` : "Sessions running on this computer.",
          ),
        ),
        all.length
          ? null
          : h(
              "p",
              { class: "rounded-xl bg-panel p-4 text-sm leading-relaxed text-ink-2" },
              "No sessions yet. Sessions show up here once the Claude Code hooks from SETUP.md are installed.",
            ),
        all.length
          ? h(
              "section",
              { class: "flex flex-col gap-3", "aria-label": "Running sessions" },
              h("h3", { class: "text-sm font-semibold" }, "Running"),
              active.length
                ? h("ul", { class: "flex flex-col gap-3" }, ...active.map(activeSession))
                : h("p", { class: "text-sm text-ink-2" }, "No Claude Code sessions are running."),
            )
          : null,
        ended.length
          ? h(
              "section",
              { class: "flex flex-col gap-2", "aria-label": "Ended sessions" },
              h("h3", { class: "text-sm font-semibold" }, "Ended"),
              h("ul", { class: "divide-y divide-line rounded-xl bg-panel" }, ...ended.map(endedSession)),
            )
          : null,
      ),
    ),
  );
}

function activeSession(s: Session) {
  return h(
    "li",
    { class: "rounded-xl bg-card p-4 shadow-card" },
    h(
      "div",
      { class: "flex items-baseline gap-2" },
      h("span", { class: "truncate font-semibold", title: s.cwd ?? "" }, folder(s.cwd)),
      h("span", { class: "shrink-0 text-xs text-ink-2" }, `Active ${ago(s.last_event_at)}`),
      h(
        "span",
        {
          class: `ml-auto flex shrink-0 items-center gap-1.5 text-xs ${s.running.length ? "font-medium text-accent" : "text-ink-2"}`,
        },
        s.running.length ? h("span", { class: "size-1.5 rounded-full bg-accent", "aria-hidden": "true" }) : null,
        s.running.length ? `${plural(s.running.length, "agent")} running` : "No agents running",
      ),
    ),
    s.cwd ? h("p", { class: "truncate text-xs text-ink-2" }, s.cwd) : null,
    s.first_prompt ? h("p", { class: "mt-2 line-clamp-2 text-sm" }, s.first_prompt) : null,
    s.running.length
      ? h(
          "ul",
          { class: "mt-3 flex flex-col gap-1.5 border-t border-line pt-3", "aria-label": "Running agents" },
          ...s.running.map((a) =>
            h(
              "li",
              { class: "flex items-center gap-2 text-sm" },
              h("span", { class: "text-ink-2" }, icon("bot")),
              h("span", { class: "min-w-0 truncate" }, a.description ?? "Agent"),
              a.agent_type ? h("span", { class: "shrink-0 text-xs text-ink-2" }, a.agent_type) : null,
              h("span", { class: "ml-auto shrink-0 text-xs text-ink-2" }, `started ${ago(a.started_at)}`),
            ),
          ),
        )
      : null,
  );
}

function endedSession(s: Session) {
  return h(
    "li",
    { class: "flex items-baseline gap-3 px-4 py-2.5 text-sm" },
    h("span", { class: "w-40 shrink-0 truncate font-medium", title: s.cwd ?? "" }, folder(s.cwd)),
    h("span", { class: "min-w-0 flex-1 truncate text-ink-2" }, s.first_prompt ?? ""),
    h(
      "span",
      { class: "shrink-0 text-xs text-ink-2" },
      [s.agents_total ? plural(s.agents_total, "agent") : null, s.ended_at ? `ended ${ago(s.ended_at)}` : "no activity for 2 hours"]
        .filter(Boolean)
        .join(", "),
    ),
  );
}
