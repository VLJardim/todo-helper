import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { deadlineStage, iso, openCard, STAGE, type Card } from "./board";
import { btn, confirmDialog, h, redraw, toast } from "./dom";
import { icon } from "./icons";
import type { Task } from "./tasks";

// The Calendar view: one week of everyone's shared events, your own cards with a date, and the
// Activity list. It reads the backend's copy of the shared calendar (synced every minute); every
// change goes through the backend to Supabase, which logs who made it.

export type Member = { id: string; name: string };
export type SharedEvent = {
  id: string;
  title: string;
  starts_at: string;
  ends_at: string; // for an all-day event, the start of its last day
  all_day: boolean;
  note: string;
  created_by: string | null;
  created_at: string;
  updated_by: string | null;
  updated_at: string;
};
type Change = {
  id: number;
  event_id: string;
  action: "added" | "changed" | "deleted";
  by_user: string | null;
  at: string;
  title: string;
  starts_at: string | null;
  all_day: boolean;
  old_title: string | null;
  old_starts_at: string | null;
};
export type Calendar = {
  status: "not_set_up" | "signed_out" | "needs_name" | "ready";
  me: string | null;
  email: string | null;
  members: Member[];
  hidden: string[];
  events: SharedEvent[];
  log: Change[];
  tasks: Task[]; // every member's tasks (src/tasks.ts)
  synced_at: string | null;
  problem: string | null; // why the last sync failed, if it did
  from: string; // the days the copy covers
  to: string;
};
type Saved = { outcome: "saved"; event: SharedEvent } | { outcome: "conflict"; event: SharedEvent | null };

const HOUR = 48; // px per hour in the time grid

export const loadCalendar = () => invoke<Calendar>("calendar_state");

const time = (ts: string) => new Date(ts).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
const dayLabel = (day: string) => new Date(`${day}T00:00:00`).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
const when = (ts: string) =>
  new Date(ts).toLocaleString("en-GB", { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });

export function nameOf(cal: Calendar, id: string | null) {
  if (!id) return "Someone";
  if (id === cal.me) return "You";
  return cal.members.find((m) => m.id === id)?.name ?? "A former member";
}

export const eventTime = (e: SharedEvent) => (e.all_day ? "All day" : `${time(e.starts_at)}–${time(e.ends_at)}`);

// When an event starts, as seen from `day`: one that began the day before says when it ends.
export const startOn = (e: SharedEvent, day: string) => (iso(new Date(e.starts_at)) < day ? `until ${time(e.ends_at)}` : time(e.starts_at));

// The local days an event covers. A timed event that ends at midnight doesn't reach into the next day.
function span(e: SharedEvent): [string, string] {
  const start = iso(new Date(e.starts_at));
  if (e.all_day) return [start, iso(new Date(e.ends_at))];
  const end = Math.max(Date.parse(e.ends_at) - 1, Date.parse(e.starts_at));
  return [start, iso(new Date(end))];
}

// Shared events on a day, without the members turned off in the legend and the events in `skip`.
export function eventsOn(cal: Calendar, day: string, skip: Set<string> = new Set()) {
  return cal.events.filter((e) => {
    const [first, last] = span(e);
    return first <= day && day <= last && !skip.has(e.id) && !cal.hidden.includes(e.created_by ?? "");
  });
}

let root: HTMLElement;
let cal: Calendar;
let week = 0; // weeks from this one
let scrolled = false; // the hours start at 07:00 the first time, then stay where you left them

export function mountCalendar(el: HTMLElement) {
  root = el;
  scrolled = false;
  return render();
}

// A sync, a sign-in, or a board change redraws the open view, except while a form is open.
function refresh() {
  if (!root?.isConnected || document.querySelector("dialog[open]")) return;
  render();
}
listen("calendar-changed", refresh);
listen("settings-changed", refresh);
listen("board-changed", refresh);

function weekDays(offset: number) {
  const monday = new Date();
  monday.setHours(0, 0, 0, 0);
  monday.setDate(monday.getDate() - ((monday.getDay() + 6) % 7) + offset * 7);
  return Array.from({ length: 7 }, (_, i) => {
    const d = new Date(monday);
    d.setDate(monday.getDate() + i);
    return iso(d);
  });
}

async function render() {
  const [state, cards] = await Promise.all([loadCalendar(), invoke<Card[]>("list_cards")]);
  cal = state;
  if (!root.isConnected) return;
  if (cal.status !== "ready") return redraw(root, notReady());
  const days = weekDays(week);
  redraw(root, h("div", { class: "flex h-full flex-col" }, header(days), statusLine(), grid(days, cards)));
  if (!scrolled) {
    root.querySelector("#hours")!.scrollTop = 7 * HOUR - HOUR / 4; // 07:00, with its label in view
    scrolled = true;
  }
}

function notReady() {
  const text = {
    not_set_up:
      "The shared calendar isn't set up on this computer yet. In Settings, add the Project URL and publishable key you got from the calendar owner, then sign in.",
    signed_out: "Sign in to the shared calendar in Settings to see everyone's events.",
    needs_name: "In Settings, under Shared calendar, type the name the others should see. Then the calendar opens here.",
    ready: "",
  }[cal.status];
  return h(
    "div",
    { class: "flex h-full flex-col items-center justify-center gap-3 p-6 text-center" },
    h("h2", { class: "text-lg font-semibold" }, "Calendar"),
    h("p", { class: "max-w-md text-sm text-ink-2" }, text),
    h("button", { class: btn.primary, onclick: () => window.dispatchEvent(new CustomEvent("show-view", { detail: "settings" })) }, "Open Settings"),
  );
}

function header(days: string[]) {
  const first = new Date(`${days[0]}T00:00:00`).toLocaleDateString("en-GB", { day: "numeric", month: "short" });
  const last = new Date(`${days[6]}T00:00:00`).toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" });
  const go = (w: number) => {
    week = w;
    render();
  };
  const arrow = `${btn.icon} disabled:opacity-40 disabled:hover:bg-transparent`;
  return h(
    "header",
    { class: "flex flex-wrap items-center gap-x-3 gap-y-2 px-4 pt-4" },
    h("h2", { class: "text-lg font-semibold" }, "Calendar"),
    h(
      "div",
      { class: "flex items-center gap-1" },
      // The copy covers 4 weeks back and 26 ahead, so the arrows stop there.
      h("button", { class: arrow, title: "Earlier week", "aria-label": "Show the earlier week", disabled: days[6] < cal.from, onclick: () => go(week - 1) }, icon("chevron-left")),
      h("button", { class: btn.secondary, disabled: week === 0, onclick: () => go(0) }, "Today"),
      h("button", { class: arrow, title: "Later week", "aria-label": "Show the later week", disabled: weekDays(week + 1)[0] > cal.to, onclick: () => go(week + 1) }, icon("chevron-right")),
    ),
    h("p", { class: "text-sm font-semibold", "aria-live": "polite" }, `${first} – ${last}`),
    h(
      "div",
      { class: "ml-auto flex items-center gap-2" },
      h("button", { class: btn.secondary, onclick: openActivity }, icon("history"), "Activity"),
      h("button", { class: btn.primary, onclick: () => openEvent(cal, null, firstFreeHour(days), render) }, icon("plus"), "New event"),
    ),
  );
}

// Who is shown, and when the copy was last updated (or why it couldn't be).
function statusLine() {
  const at = cal.synced_at ? time(cal.synced_at) : null;
  return h(
    "div",
    { class: "flex flex-wrap items-center gap-x-4 gap-y-1 px-4 pt-2 pb-3" },
    h(
      "div",
      { class: "flex flex-wrap items-center gap-1.5", role: "group", "aria-label": "Show events added by" },
      h("span", { class: "text-xs text-ink-2" }, "Showing"),
      ...cal.members.map((m) => {
        const shown = !cal.hidden.includes(m.id);
        const name = m.id === cal.me ? `${m.name} (you)` : m.name;
        return h(
          "button",
          {
            class: `flex items-center gap-1 rounded-md border px-2 py-0.5 text-xs transition-colors hover:bg-panel ${
              shown ? "border-line bg-card text-ink" : "border-dashed border-line text-ink-2 line-through"
            }`,
            "aria-pressed": String(shown),
            title: shown ? `Hide the events ${name} added` : `Show the events ${name} added`,
            onclick: () => invoke("calendar_set_hidden", { ids: shown ? [...cal.hidden, m.id] : cal.hidden.filter((id) => id !== m.id) }),
          },
          shown ? icon("check", "size-3") : null,
          name,
        );
      }),
    ),
    h(
      "p",
      { class: `ml-auto text-xs ${cal.problem ? "text-danger" : "text-ink-2"}`, role: "status" },
      cal.problem
        ? `${cal.problem} Showing the calendar as of ${at ?? "the last update"}.`
        : at
          ? `Updated ${at}. Checks every minute.`
          : "Not updated yet.",
    ),
  );
}

// The members turned on in the legend, you first: each gets a column in every day.
function people() {
  const shown = cal.members.filter((m) => !cal.hidden.includes(m.id));
  return [...shown.filter((m) => m.id === cal.me), ...shown.filter((m) => m.id !== cal.me)];
}

// Whose column an event goes in: the member who added it. An event by someone who is no longer
// a member goes in the first column.
function ownerOf(e: SharedEvent, ids: string[]) {
  return e.created_by && ids.includes(e.created_by) ? e.created_by : ids[0];
}

// The time gutter, then one column per member per day: Monday you, Monday Ana, Monday
// Kim, Tuesday you, and so on. A column is at least 6rem wide, so with several members the grid
// scrolls sideways inside its panel instead of squeezing the events.
const cols = (n: number) => `grid-template-columns: 3.5rem repeat(${7 * n}, minmax(6rem, 1fr))`;
// The first column of each day gets a darker line, so the days stand apart.
const dayEdge = (i: number) => (i === 0 ? "border-field" : "border-line");
// Pinned to the left while the grid scrolls sideways, like the hours.
const PIN = "sticky left-0 z-10 bg-panel";
const label = (m: Member) => (m.id === cal.me ? "You" : m.name);

function grid(days: string[], cards: Card[]) {
  const today = iso(new Date());
  const who = people();
  if (!who.length)
    return h("p", { class: "mx-4 mb-4 rounded-xl bg-panel p-6 text-center text-sm text-ink-2" }, "Everyone is turned off. Select a name next to Showing to see their events.");
  const ids = who.map((m) => m.id);
  // A shared card shows as its event, not twice. Your cards sit in your column only.
  const inCalendar = new Set(cal.events.map((e) => e.id));
  const shown = cards.filter((c) => !(c.shared_event_id && inCalendar.has(c.shared_event_id)));
  const cardsOn = (day: string, m: Member) => (m.id === cal.me ? shown.filter((c) => c.due === day && !timedOn(c, day)) : []);
  // Every member's column on every day, in order.
  const each = <T>(f: (day: string, m: Member, i: number) => T) => days.flatMap((d) => who.map((m, i) => f(d, m, i)));
  // One scroll area both ways: the days and the all-day row stay on top, the hours on the left.
  return h(
    "div",
    { id: "hours", class: "mx-4 mb-4 min-h-0 flex-1 overflow-auto rounded-xl bg-panel" },
    h(
      "div",
      // At least 6rem per column, but never wider because of a long title: "max-content" would
      // size every column to its longest event.
      { style: `min-width: calc(3.5rem + ${7 * who.length * 6}rem)` },
      h(
        "div",
        { class: "sticky top-0 z-20 bg-panel" },
        h(
          "div",
          { class: "grid border-b border-line", style: cols(who.length) },
          h("div", { class: PIN }),
          ...days.map((d) =>
            h(
              "h3",
              { class: "flex items-baseline gap-2 truncate px-2 pt-2.5 pb-0.5 text-xs text-ink-2", style: `grid-column: span ${who.length}` },
              d === today ? h("span", { class: "-mx-1 rounded bg-mark px-1 text-sm font-semibold text-mark-ink" }, "Today") : null,
              dayLabel(d),
            ),
          ),
          h("div", { class: PIN }),
          ...each((_, m, i) => h("span", { class: `truncate border-l px-2 pb-1.5 text-xs text-ink-2 ${dayEdge(i)}` }, label(m))),
        ),
        h(
          "div",
          { class: "grid border-b border-line", style: cols(who.length) },
          h("div", { class: `${PIN} px-2 py-2 text-right text-xs text-ink-2` }, "All day"),
          ...each((d, m, i) =>
            h(
              "ol",
              {
                class: `flex min-h-10 min-w-0 cursor-cell flex-col gap-1 border-l p-1 ${dayEdge(i)}`,
                "aria-label": `All day, ${dayLabel(d)}, ${label(m)}`,
                onclick: (e: MouseEvent) => e.target === e.currentTarget && openEvent(cal, null, { date: d, allDay: true, start: "09:00", end: "10:00" }, render),
              },
              ...eventsOn(cal, d)
                .filter((e) => e.all_day && ownerOf(e, ids) === m.id)
                .map((e) => h("li", {}, eventButton(e))),
              ...cardsOn(d, m).map((c) => h("li", {}, cardButton(c))),
            ),
          ),
        ),
      ),
      hours(days, who, shown),
    ),
  );
}

// A card with times (an itslearning lesson) sits in the hour grid, as long as it is still on the
// day those times are on. Moved to another day, it shows all day there.
const timedOn = (c: Card, day: string) => !!(c.starts_at && c.ends_at && c.due === day && iso(new Date(c.starts_at)) === day);

function hours(days: string[], who: Member[], cards: Card[]) {
  const gutter = h(
    "div",
    { class: PIN, "aria-hidden": "true" },
    ...Array.from({ length: 23 }, (_, i) => {
      const label = h("span", { class: "absolute right-2 -translate-y-1/2 text-xs tabular-nums text-ink-2" }, `${String(i + 1).padStart(2, "0")}:00`);
      label.style.top = `${(i + 1) * HOUR}px`;
      return label;
    }),
  );
  const ids = who.map((m) => m.id);
  const body = h(
    "div",
    { class: "grid", style: cols(who.length) },
    gutter,
    ...days.flatMap((d) => who.map((m, i) => dayColumn(d, m, ids, i, m.id === cal.me ? cards : []))),
  );
  body.style.height = `${24 * HOUR}px`;
  return body;
}

// Minutes since midnight on `day` by the clock, from 0 to 1440. Clock time, not time passed, because
// on the days the clocks change, 14:00 is 13 or 15 hours after midnight.
function clock(ts: number, day: string) {
  const d = new Date(ts);
  const on = iso(d);
  return on < day ? 0 : on > day ? 24 * 60 : d.getHours() * 60 + d.getMinutes();
}

type Placed<T> = { e: T; from: number; to: number; lane: number; lanes: number };

// Events that overlap sit side by side: each takes the first free lane, and a group of
// overlapping events shares the day's width evenly.
function lanes<T>(items: { e: T; from: number; to: number }[]): Placed<T>[] {
  const out: Placed<T>[] = [];
  let group: Placed<T>[] = [];
  let ends: number[] = [];
  let groupEnd = -Infinity;
  const close = () => {
    for (const p of group) p.lanes = ends.length;
    group = [];
    ends = [];
  };
  for (const item of [...items].sort((a, b) => a.from - b.from || b.to - a.to)) {
    if (item.from >= groupEnd) close();
    let lane = ends.findIndex((end) => end <= item.from);
    if (lane < 0) lane = ends.push(item.to) - 1;
    else ends[lane] = item.to;
    const placed = { ...item, lane, lanes: 1 };
    group.push(placed);
    out.push(placed);
    groupEnd = Math.max(groupEnd, item.to);
  }
  close();
  return out;
}

// One member's column on one day; `i` is the member's place in the day, for the day's edge line.
function dayColumn(day: string, m: Member, ids: string[], i: number, cards: Card[]) {
  const at = (starts: string, ends: string) => ({ from: clock(Date.parse(starts), day), to: clock(Date.parse(ends), day) });
  const timed: { e: SharedEvent | Card; from: number; to: number }[] = [
    ...eventsOn(cal, day)
      .filter((e) => !e.all_day && ownerOf(e, ids) === m.id)
      .map((e) => ({ e, ...at(e.starts_at, e.ends_at) })),
    ...cards.filter((c) => timedOn(c, day)).map((c) => ({ e: c, ...at(c.starts_at!, c.ends_at!) })),
  ];
  const px = (minutes: number) => (minutes / 60) * HOUR;
  const col = h(
    "div",
    {
      class: `relative min-w-0 cursor-cell border-l ${dayEdge(i)}`,
      "aria-label": `${dayLabel(day)}, ${label(m)}`,
      // A click on an empty spot adds an hour-long event from that half hour.
      onclick: (e: MouseEvent) => {
        if (e.target !== e.currentTarget) return;
        const minutes = Math.min(Math.floor(e.offsetY / (HOUR / 2)) * 30, 23 * 60);
        openEvent(cal, null, hourAt(day, minutes), render);
      },
    },
    ...lanes(timed).map(({ e, from, to, lane, lanes }) => {
      // Under 45 minutes there is room for one line only.
      const kind = to - from < 45 ? "short" : "timed";
      const b = "created_by" in e ? eventButton(e, kind, day) : cardButton(e, kind);
      b.classList.add("absolute");
      b.style.top = `${px(from)}px`;
      b.style.height = `${Math.max(px(to - from), HOUR / 2) - 2}px`;
      b.style.left = `calc(${(lane / lanes) * 100}% + 2px)`;
      b.style.width = `calc(${100 / lanes}% - 4px)`;
      return b;
    }),
  );
  col.style.backgroundImage = "linear-gradient(to bottom, var(--line) 1px, transparent 1px)";
  col.style.backgroundSize = `100% ${HOUR}px`;
  // A thin line at the current time today.
  if (day === iso(new Date())) {
    const now = h("div", { class: "pointer-events-none absolute inset-x-0 h-px bg-ink-2", "aria-hidden": "true" });
    now.style.top = `${px(clock(Date.now(), day))}px`;
    col.append(now);
  }
  return col;
}

const pad = (n: number) => String(n).padStart(2, "0");
type Preset = { date: string; allDay: boolean; start: string; end: string };

function hourAt(day: string, minutes: number): Preset {
  const end = Math.min(minutes + 60, 23 * 60 + 59);
  return { date: day, allDay: false, start: `${pad(Math.floor(minutes / 60))}:${pad(minutes % 60)}`, end: `${pad(Math.floor(end / 60))}:${pad(end % 60)}` };
}

// New event: the next full hour today when this week is shown, otherwise 09:00 on its Monday.
function firstFreeHour(days: string[]): Preset {
  const today = iso(new Date());
  return days.includes(today) ? hourAt(today, Math.min(new Date().getHours() + 1, 23) * 60) : hourAt(days[0], 9 * 60);
}

function eventButton(e: SharedEvent, kind: "all-day" | "timed" | "short" = "all-day", day = "") {
  const who = nameOf(cal, e.created_by);
  const short = kind === "short";
  return h(
    "button",
    {
      class: `group flex w-full min-w-0 overflow-hidden rounded-md bg-card px-1.5 text-left text-xs leading-tight shadow-card ${
        short ? "items-baseline gap-1 py-0.5" : "flex-col py-1"
      }`,
      title: `${e.title}. ${eventTime(e)}. Added by ${who}.`,
      onclick: () => openEvent(cal, e, undefined, render),
    },
    short ? h("span", { class: "shrink-0 tabular-nums text-ink-2" }, startOn(e, day)) : null,
    h("span", { class: "truncate font-medium group-hover:underline" }, e.title),
    short ? null : h("span", { class: "truncate text-ink-2" }, kind === "timed" ? `${startOn(e, day)} · ${who}` : who),
  );
}

// Your own card with a date: only on this computer, so it looks quieter than a shared event. In
// the hour grid it is an outline instead of a filled block.
function cardButton(c: Card, kind: "all-day" | "timed" | "short" = "all-day") {
  const done = c.col === "done";
  const stage = deadlineStage(c);
  const hours = c.starts_at && c.ends_at ? `${time(c.starts_at)}–${time(c.ends_at)}` : "";
  const mark = icon(done ? "check" : "kanban", `size-3 ${done ? "text-accent" : "text-ink-2"}`);
  const title = h("span", { class: `truncate ${done ? "text-ink-2" : ""}` }, c.title);
  const grid = "flex w-full min-w-0 overflow-hidden rounded-md border border-line px-1.5 text-left text-xs leading-tight transition-colors hover:bg-card";
  return h(
    "button",
    {
      // A deadline card carries the board's colors, so it stands out here too.
      class: `${
        kind === "all-day"
          ? "flex w-full min-w-0 items-center gap-1 rounded-md px-1.5 py-1 text-left text-xs transition-colors hover:bg-card"
          : `${grid} ${kind === "short" ? "items-baseline gap-1 py-0.5" : "flex-col py-1"}`
      } ${stage ? STAGE[stage].card : ""}`,
      title: `Your card: ${c.title}.${hours ? ` ${hours}.` : ""} Only you see it.`,
      onclick: () => openCard(c, render),
    },
    kind === "all-day" ? mark : null,
    kind === "short" ? h("span", { class: "shrink-0 tabular-nums text-ink-2" }, time(c.starts_at!)) : null,
    kind === "timed" ? h("span", { class: "flex min-w-0 items-center gap-1" }, mark, title) : title,
    kind === "timed" ? h("span", { class: "truncate tabular-nums text-ink-2" }, hours) : null,
  );
}

// ---- The event form ----

// Opens an event, or a new one from `preset`, in a sheet at the right edge. Anyone can change or
// delete any event; the sheet says who added it and who changed it last.
export function openEvent(c: Calendar, e: SharedEvent | null, preset?: Preset, onClose?: () => void) {
  if (document.querySelector("dialog[open]")) return;
  let current = e; // the version the form is changing, so a save never overwrites a newer one
  const field = "rounded-md border border-field bg-card px-2 py-1 focus:border-accent focus:outline-none";
  const title = h("input", {
    class: "min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-2 py-1 -ml-2 text-lg font-semibold hover:border-line focus:border-accent focus:outline-none",
    placeholder: "Event title",
    maxlength: 200,
    "aria-label": "Event title",
  });
  const date = h("input", { id: "ev-date", type: "date", class: field });
  const allDay = h("input", { type: "checkbox", class: "size-4 accent-accent" });
  const until = h("input", { id: "ev-until", type: "date", class: field });
  const from = h("input", { id: "ev-from", type: "time", class: field });
  const to = h("input", { id: "ev-to", type: "time", class: field });
  const note = h("textarea", { id: "ev-note", class: `${field} min-h-24 resize-y py-1.5`, maxlength: 4000, placeholder: "Where, what to bring, a link" });
  const byline = h("p", { class: "text-xs text-ink-2" });
  const message = h("p", { class: "min-h-5 text-sm text-danger", role: "alert" });
  const save = h("button", { type: "submit", class: btn.primary }, "Save");
  const heading = h("h2", { class: "sr-only" });

  const allDayRows = h("div", { class: "contents" }, h("label", { for: "ev-until", class: "text-ink-2" }, "Until"), until);
  const timeRows = h(
    "div",
    { class: "contents" },
    h("label", { for: "ev-from", class: "text-ink-2" }, "Time"),
    h("div", { class: "flex items-center gap-2" }, from, h("span", { "aria-hidden": "true" }, "–"), to, h("span", { class: "sr-only" }, "end time")),
  );
  const showRows = () => {
    allDayRows.className = allDay.checked ? "contents" : "hidden";
    timeRows.className = allDay.checked ? "hidden" : "contents";
  };
  allDay.addEventListener("change", showRows);

  const fill = (ev: SharedEvent | null, p?: Preset) => {
    heading.textContent = ev ? "Shared event" : "New shared event";
    title.value = ev?.title ?? "";
    note.value = ev?.note ?? "";
    date.value = ev ? iso(new Date(ev.starts_at)) : (p?.date ?? iso(new Date()));
    allDay.checked = ev?.all_day ?? p?.allDay ?? false;
    until.value = ev?.all_day ? iso(new Date(ev.ends_at)) : date.value;
    from.value = ev && !ev.all_day ? time(ev.starts_at) : (p?.start ?? "09:00");
    to.value = ev && !ev.all_day ? time(ev.ends_at) : (p?.end ?? "10:00");
    byline.textContent = ev
      ? [
          `Added by ${nameOf(c, ev.created_by)}, ${when(ev.created_at)}`,
          ev.updated_at !== ev.created_at ? `Changed by ${nameOf(c, ev.updated_by)}, ${when(ev.updated_at)}` : null,
        ]
          .filter(Boolean)
          .join(". ") + "."
      : "Everyone in the shared calendar sees it and can change it.";
    remove.hidden = !ev;
    showRows();
  };

  // The times as the backend wants them. A timed event that ends before it starts ends the next day.
  const times = () => {
    if (!title.value.trim()) throw "An event needs a title.";
    if (!date.value) throw "Pick a date.";
    if (allDay.checked) {
      const last = until.value || date.value;
      if (last < date.value) throw "The last day is before the first.";
      return { starts_at: new Date(`${date.value}T00:00`).toISOString(), ends_at: new Date(`${last}T00:00`).toISOString(), all_day: true };
    }
    if (!from.value || !to.value) throw "Pick a start and an end time.";
    const start = new Date(`${date.value}T${from.value}`);
    const end = new Date(`${date.value}T${to.value}`);
    if (end < start) end.setDate(end.getDate() + 1);
    return { starts_at: start.toISOString(), ends_at: end.toISOString(), all_day: false };
  };

  const submit = async (ev: SubmitEvent) => {
    ev.preventDefault();
    message.textContent = "";
    let t;
    try {
      t = times();
    } catch (m) {
      message.textContent = String(m);
      return;
    }
    save.disabled = true;
    try {
      const draft = { id: current?.id ?? null, updated_at: current?.updated_at ?? null, title: title.value, note: note.value, ...t };
      const saved = await invoke<Saved>("calendar_save", { draft });
      if (saved.outcome === "saved") return dialog.close();
      // Someone else saved first: show their version instead of overwriting it.
      current = saved.event;
      fill(current);
      message.textContent = current
        ? `${nameOf(c, current.updated_by)} changed this event while you had it open. This is their version; make your change again if it's still needed.`
        : "Someone deleted this event while you had it open. Save adds it again.";
    } catch (err) {
      message.textContent = String(err); // the form keeps what you typed
    } finally {
      save.disabled = false;
    }
  };

  const remove = h(
    "button",
    {
      type: "button",
      class: btn.danger,
      onclick: async () => {
        if (!current) return;
        const gone = current;
        if (!(await confirmDialog(`Delete "${gone.title}" for everyone? The Activity list keeps a record of it.`, "Delete"))) return;
        try {
          await invoke("calendar_delete", { id: gone.id });
          dialog.close();
          toast(`Deleted "${gone.title}".`);
        } catch (err) {
          message.textContent = String(err);
        }
      },
    },
    icon("trash"),
    "Delete",
  );

  const dialog = h(
    "dialog",
    {
      class: "fixed inset-y-0 right-0 left-auto m-0 h-dvh max-h-dvh w-[28rem] max-w-full border-l border-line bg-card text-ink shadow-raised",
      "aria-label": e ? e.title : "New shared event",
    },
    h(
      "form",
      { class: "flex h-full flex-col gap-4 p-5", onsubmit: submit },
      heading,
      h("div", { class: "flex items-start gap-2" }, title, h("button", { type: "button", class: btn.icon, title: "Close", "aria-label": "Close", onclick: () => dialog.close() }, icon("x"))),
      byline,
      h(
        "div",
        { class: "grid grid-cols-[5rem_1fr] items-center gap-x-3 gap-y-3 text-sm" },
        h("label", { for: "ev-date", class: "text-ink-2" }, "Date"),
        h("div", { class: "flex flex-wrap items-center gap-x-4 gap-y-2" }, date, h("label", { class: "flex items-center gap-2" }, allDay, "All day")),
        allDayRows,
        timeRows,
        h("label", { for: "ev-note", class: "self-start pt-1.5 text-ink-2" }, "Note"),
        note,
      ),
      message,
      h(
        "div",
        { class: "mt-auto flex items-center gap-2 border-t border-line pt-4" },
        remove,
        h("button", { type: "button", class: `${btn.secondary} ml-auto`, onclick: () => dialog.close() }, "Cancel"),
        save,
      ),
    ),
  );
  fill(e, preset);
  dialog.addEventListener("close", () => {
    dialog.remove();
    onClose?.();
  });
  document.body.append(dialog);
  dialog.showModal();
  title.focus();
}

// ---- Activity ----

// The latest 50 changes, newest first, including what was deleted and by whom.
function openActivity() {
  if (document.querySelector("dialog[open]")) return;
  const start = (ts: string | null, allDay: boolean) => (ts ? (allDay ? dayLabel(iso(new Date(ts))) : when(ts)) : null);
  const items = cal.log.map((c) => {
    const event = c.action === "deleted" ? undefined : cal.events.find((e) => e.id === c.event_id);
    const details = [
      c.old_title && c.old_title !== c.title ? `was called "${c.old_title}"` : null,
      c.old_starts_at && c.old_starts_at !== c.starts_at ? `moved from ${start(c.old_starts_at, c.all_day)}` : null,
    ].filter(Boolean);
    const at = start(c.starts_at, c.all_day);
    return h(
      "li",
      { class: "flex flex-col gap-0.5 border-b border-line py-2.5" },
      h(
        "p",
        { class: "text-sm" },
        h("span", { class: "font-semibold" }, nameOf(cal, c.by_user)),
        ` ${c.action} `,
        event
          ? h(
              "button",
              {
                class: "font-medium underline underline-offset-2 hover:no-underline",
                onclick: () => {
                  dialog.close();
                  openEvent(cal, event, undefined, render);
                },
              },
              c.title,
            )
          : h("span", { class: "font-medium" }, c.title),
        at ? ` (${at})` : "",
      ),
      details.length ? h("p", { class: "text-xs text-ink-2" }, `${details.join(", ")}.`) : null,
      h("p", { class: "text-xs text-ink-2" }, when(c.at)),
    );
  });
  const dialog = h(
    "dialog",
    {
      class: "fixed inset-y-0 right-0 left-auto m-0 h-dvh max-h-dvh w-[28rem] max-w-full border-l border-line bg-card text-ink shadow-raised",
      "aria-label": "Activity",
    },
    h(
      "div",
      { class: "flex h-full flex-col gap-3 p-5" },
      h(
        "div",
        { class: "flex items-start gap-2" },
        h("h2", { class: "flex-1 text-lg font-semibold" }, "Activity"),
        h("button", { class: btn.icon, title: "Close", "aria-label": "Close", onclick: () => dialog.close() }, icon("x")),
      ),
      h("p", { class: "text-xs text-ink-2" }, "The latest 50 changes in the shared calendar, newest first. Supabase records them; nobody can edit this list."),
      items.length
        ? h("ol", { class: "min-h-0 flex-1 overflow-y-auto" }, ...items)
        : h("p", { class: "text-sm text-ink-2" }, "No changes yet. Add an event and it shows up here."),
    ),
  );
  dialog.addEventListener("close", () => dialog.remove());
  document.body.append(dialog);
  dialog.showModal();
}
