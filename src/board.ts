import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { btn, h, redraw, toast } from "./dom";
import { icon } from "./icons";
import { eventsOn, eventTime, loadCalendar, nameOf, openEvent, startOn, type Calendar, type SharedEvent } from "./calendar";
import { noteEditor, type Note } from "./notes";
import { giveTask, showGiven } from "./tasks";

export type Col = "inbox" | "today" | "doing" | "done";
export type Card = {
  id: number;
  title: string;
  col: Col;
  position: number;
  due: string | null;
  source: "manual" | "email" | "discord" | "itslearning";
  note_id: number | null;
  inbox_item_id: number | null;
  external_id: string | null;
  shared_event_id: string | null; // the shared calendar event this card was shared as
  starts_at: string | null; // a timed itslearning event's start and end (UTC)
  ends_at: string | null;
  deadline: boolean; // a hand-in deadline, colored as it gets close
  task_id: string | null; // a task another member gave you (src/tasks.ts)
  task_new: boolean; // given to you and not opened yet
};

// "today" is still a value in the database: a card dragged to Today before the week columns
// existed. It shows in today's day.
const COLUMNS: [Col, string][] = [
  ["inbox", "Inbox"],
  ["doing", "Doing"],
  ["done", "Done"],
];

const EMPTY: Record<string, string> = {
  inbox: "Cards without a date in the next 4 weeks land here. Type above to add your own.",
  doing: "Move a card here when you start it.",
  done: "Finished cards collect here.",
};

// ---- Days ----
// A slot is where a card shows: a column, or a day ("2026-10-26") in one of the 4 week columns.
// Cards that aren't in Doing or Done go to their due date: overdue in today, the next 28 days
// (today included) in their day, anything later (or undated) in Inbox.

const WEEKS = 4;
export const iso = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
// days()[0] is today, days()[27] is the last day of the 4th week.
function days() {
  return Array.from({ length: WEEKS * 7 }, (_, n) => {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    d.setDate(d.getDate() + n);
    return iso(d);
  });
}
const isDay = (slot: string) => slot.includes("-");

function slotOf(card: Card, ds = days()): string {
  if (card.col === "doing" || card.col === "done") return card.col;
  if (!card.due) return card.col === "today" ? ds[0] : "inbox";
  if (card.due <= ds[0]) return ds[0];
  if (card.due <= ds[ds.length - 1]) return card.due;
  return "inbox";
}

// Inbox, every day of the 4 weeks, Doing, Done: the order Alt+arrow keys move through.
const slots = () => ["inbox", ...days(), "doing", "done"];

function slotLabel(slot: string) {
  return isDay(slot)
    ? new Date(`${slot}T00:00:00`).toLocaleDateString("en-GB", { weekday: "long", day: "numeric", month: "long" })
    : COLUMNS.find(([c]) => c === slot)![1];
}

let root: HTMLElement;
let cards: Card[] = []; // as of the last render
let cal: Calendar | null = null; // the shared calendar, when you're signed in
let focusId: number | null = null; // card to focus after the next render
let swipeId: number | null = null; // card that just reached Done

export async function mountBoard(el: HTMLElement) {
  root = el;
  const saved = (await invoke<{ plain: Record<string, string> } | null>("get_settings"))?.plain.board_open_days ?? "all";
  openDays = saved === "all" ? "all" : new Set(saved.split(",").filter(Boolean));
  return render();
}

// Cards added in the background (from email, Discord, itslearning) show up without a reload.
// It waits while a card sheet is open, a card is being renamed, or a card is being dragged; the
// next change redraws it.
function refresh() {
  if (!root?.isConnected || dragId != null || document.querySelector("dialog[open]") || root.querySelector("li input:focus")) return;
  render();
}
listen("board-changed", refresh);
listen("calendar-changed", refresh);

// "Due today", "Overdue", and which day is today change at midnight.
let today = new Date().toDateString();
setInterval(() => {
  if (new Date().toDateString() === today) return;
  today = new Date().toDateString();
  refresh();
}, 60_000);

export async function render() {
  if (!root?.isConnected) return;
  const [list, calendar] = await Promise.all([invoke<Card[]>("list_cards"), loadCalendar()]);
  cards = list;
  cal = calendar.status === "ready" ? calendar : null;
  const ds = days();
  // The itslearning feed holds only lessons; the board shows one only if it's a deadline or you moved it
  // to Doing or Done. The Calendar view still shows them all.
  const onBoard = (c: Card) => c.source !== "itslearning" || c.deadline || c.col === "doing" || c.col === "done";
  const inSlot = (slot: string) => cards.filter((c) => onBoard(c) && slotOf(c, ds) === slot);
  const weekScroll = root.querySelector("#weeks")?.scrollLeft ?? 0;
  const refocusAdd = document.activeElement?.id === "add-card";
  const draft = root.querySelector<HTMLInputElement>("#add-card")?.value ?? "";
  // A background redraw keeps keyboard focus on the card that had it.
  const active = document.activeElement as HTMLElement | null;
  if (focusId == null && active?.dataset.id && root.contains(active)) focusId = Number(active.dataset.id);
  redraw(
    root,
    h(
      "div",
      {
        class:
          "grid h-full min-h-0 grid-cols-[minmax(12rem,1fr)_minmax(12rem,4fr)_minmax(12rem,1fr)_minmax(12rem,1fr)] grid-rows-[auto_minmax(0,1fr)] gap-3 overflow-x-auto p-4 pt-2",
      },
      foldButton(),
      column("inbox", "Inbox", inSlot("inbox")),
      weeks(Array.from({ length: WEEKS }, (_, w) => week(ds.slice(w * 7, w * 7 + 7), inSlot))),
      column("doing", "Doing", inSlot("doing")),
      column("done", "Done", inSlot("done")),
    ),
    h("p", { id: "card-keys", class: "sr-only" }, "Enter opens the card. F2 renames it. Delete removes it. Alt and an arrow key moves it."),
  );
  // A redraw keeps the week carousel where it was.
  root.querySelector("#weeks")!.scrollLeft = weekScroll;
  updateArrows();
  if (swipeId != null) {
    root.querySelector(`[data-id="${swipeId}"]`)?.classList.add("done-swipe");
    swipeId = null;
  }
  const add = root.querySelector<HTMLInputElement>("#add-card");
  if (add) add.value = draft; // a background refresh keeps what you were typing
  if (refocusAdd) add?.focus();
  if (focusId != null) {
    root.querySelector<HTMLElement>(`[data-id="${focusId}"]`)?.focus();
    focusId = null;
  }
}

// A column or a day a card can be dropped on. `data-slot` names where a drop moves the card;
// the pointer drag (startDrag) finds it under the pointer.
function dropzone(slot: string) {
  return { "data-slot": slot };
}

// The 4 week columns as a carousel: as many weeks show side by side as fit at 12rem or more
// each, and the arrows (shown only when weeks are hidden) slide one week at a time. Dragging a
// card onto an arrow slides too, so a card can reach a hidden week.
function weeks(cols: HTMLElement[]) {
  const strip = h(
    "div",
    {
      id: "weeks",
      class:
        "grid h-full grid-flow-col auto-cols-[100%] gap-3 overflow-x-auto snap-x snap-mandatory [scrollbar-width:none] @min-[24.75rem]:auto-cols-[calc((100%_-_0.75rem)/2)] @min-[37.5rem]:auto-cols-[calc((100%_-_1.5rem)/3)] @min-[50.25rem]:auto-cols-[calc((100%_-_2.25rem)/4)]",
      onscroll: updateArrows,
    },
    ...cols,
  );
  const arrow = (dir: -1 | 1) => {
    const slide = () => strip.scrollBy({ left: dir * (cols[0].offsetWidth + 12), behavior: "smooth" });
    return h(
      "button",
      {
        "data-dir": dir,
        class: `absolute top-2 ${dir < 0 ? "left-1.5" : "right-1.5"} rounded-full bg-card p-1 text-ink shadow-raised transition-colors hover:text-accent`,
        title: dir < 0 ? "Earlier week" : "Later week",
        "aria-label": dir < 0 ? "Show the earlier week" : "Show the later week",
        onclick: slide, // a card dragged onto an arrow clicks it too (startDrag)
      },
      icon(dir < 0 ? "chevron-left" : "chevron-right"),
    );
  };
  return h("div", { class: "@container relative min-h-0 min-w-0" }, strip, arrow(-1), arrow(1));
}

// Above the week columns: "Fold all" when every day is open; otherwise "Open all" gets all days back.
function foldButton() {
  const allOpen = days().every(isOpen);
  // A full row on the board's own columns, so the button sits over the weeks and nothing else moves.
  // Over Inbox: giving tasks, once you're signed in to the shared calendar.
  const c = cal;
  return h(
    "div",
    { class: "col-span-4 -mb-1 grid grid-cols-subgrid" },
    c
      ? h(
          "div",
          { class: "flex gap-2" },
          h("button", { class: `${btn.secondary} py-1 text-xs`, onclick: () => giveTask(c) }, icon("plus"), "Give a task"),
          h("button", { class: `${btn.secondary} py-1 text-xs`, onclick: () => showGiven(c) }, "Given"),
        )
      : null,
    h(
      "div",
      { class: "col-start-2 flex justify-end" },
      h("button", { class: `${btn.secondary} py-1 text-xs`, onclick: () => setOpen(allOpen ? new Set() : "all") }, allOpen ? "Fold all days" : "Open all days"),
    ),
  );
}

// ---- Folded days ----
// "all", or the days that are open. Clicking a day's header opens it alone, so it gets the room;
// clicking it again opens all days. Stored as the `board_open_days` setting.
let openDays: "all" | Set<string> = "all";
const isOpen = (day: string) => openDays === "all" || openDays.has(day);

function setOpen(next: "all" | Set<string>) {
  openDays = next;
  invoke("save_setting", { key: "board_open_days", value: next === "all" ? "all" : [...next].join(",") });
  render();
}

function toggleDay(day: string) {
  const alone = isOpen(day) && days().filter(isOpen).length === 1;
  setOpen(alone ? "all" : new Set([day]));
}

// Each arrow shows only when there is a week to slide to on its side.
function updateArrows() {
  const strip = root?.querySelector<HTMLElement>("#weeks");
  if (!strip) return;
  const end = strip.scrollWidth - strip.clientWidth;
  for (const b of strip.parentElement!.querySelectorAll<HTMLElement>("button[data-dir]")) {
    b.classList.toggle("invisible", b.dataset.dir === "-1" ? strip.scrollLeft <= 1 : strip.scrollLeft >= end - 1);
  }
}
window.addEventListener("resize", updateArrows);

const short = (day: string) => new Date(`${day}T00:00:00`).toLocaleDateString("en-GB", { day: "numeric", month: "short" });

// A week column: 7 days stacked, each with its date in the corner and its own cards.
function week(ds: string[], inSlot: (slot: string) => Card[]) {
  const count = ds.reduce((n, d) => n + inSlot(d).length, 0);
  const label = `${short(ds[0])} – ${short(ds[6])}`;
  return h(
    "section",
    { class: "flex min-h-0 snap-start flex-col rounded-xl bg-panel", "aria-label": label },
    h(
      "header",
      { class: "flex items-baseline gap-2 px-3 pt-3 pb-1 @max-[50.25rem]:px-10" },
      h("h2", { class: "text-sm font-semibold" }, label),
      h("span", { class: "ml-auto text-xs tabular-nums text-ink-2", title: `${count} cards` }, count),
    ),
    h(
      "div",
      { class: "flex min-h-0 flex-1 flex-col divide-y divide-line" },
      ...ds.map((day) => {
        const dayCards = inSlot(day);
        const open = isOpen(day);
        return h(
          "div",
          { class: `flex flex-col ${open ? "min-h-0 flex-1" : "flex-none"}`, "aria-label": slotLabel(day), ...dropzone(day) },
          h(
            "h3",
            {},
            // A folded day keeps its header and its card count, and still takes drops.
            h(
              "button",
              {
                class: "flex w-full items-baseline gap-2 truncate px-3 pt-1.5 pb-1 text-left text-xs text-ink-2 transition-colors hover:text-ink",
                "aria-expanded": String(open),
                title: open && days().filter(isOpen).length === 1 ? "Open all days" : "Open this day and fold the others",
                onclick: () => toggleDay(day),
              },
              // Today gets the highlighter, as the Today column did.
              day === days()[0] ? h("span", { class: "-mx-1 rounded bg-mark px-1 text-sm font-semibold text-mark-ink" }, "Today") : null,
              slotLabel(day),
              h("span", { class: "ml-auto tabular-nums", title: `${dayCards.length} cards` }, dayCards.length || ""),
            ),
          ),
          open
            ? h(
                "ol",
                { class: "flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-2 pb-1.5" },
                ...sharedOn(day),
                ...dayCards.map((c) => cardEl(c, day)),
              )
            : null,
        );
      }),
    ),
  );
}

// The shared calendar's events on a day, as small entries above the cards. They can't be dragged;
// opening one opens the event. A card you shared shows as itself, not twice.
function sharedOn(day: string) {
  if (!cal) return [];
  const c = cal;
  const mine = new Set(cards.map((card) => card.shared_event_id).filter((id): id is string => !!id));
  return eventsOn(c, day, mine).map((e) =>
    h(
      "li",
      {},
      h(
        "button",
        {
          class: "group flex w-full min-w-0 items-baseline gap-1.5 rounded-md px-1 py-0.5 text-left text-xs text-ink-2 transition-colors hover:bg-card hover:text-ink",
          title: `Shared event: ${e.title}. ${eventTime(e)}. Added by ${nameOf(c, e.created_by)}.`,
          onclick: () => openEvent(c, e, undefined, render),
        },
        icon("users", "size-3 shrink-0 self-center"),
        e.all_day ? null : h("span", { class: "shrink-0 tabular-nums" }, startOn(e, day)),
        h("span", { class: "truncate font-medium text-ink group-hover:underline" }, e.title),
        h("span", { class: "ml-auto shrink-0" }, nameOf(c, e.created_by)),
      ),
    ),
  );
}

function column(col: Col, label: string, cards: Card[]) {
  return h(
    "section",
    { class: "flex min-h-0 flex-col rounded-xl bg-panel", "aria-label": label, ...dropzone(col) },
    h(
      "header",
      { class: "flex items-baseline gap-2 px-3 pt-3 pb-2" },
      h("h2", { class: "text-sm font-semibold" }, label),
      h("span", { class: "ml-auto text-xs tabular-nums text-ink-2", title: `${cards.length} cards` }, cards.length),
    ),
    col === "inbox" ? addInput() : null,
    h(
      "ol",
      { class: "flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-2 pb-2" },
      ...cards.map((c) => cardEl(c, col)),
      cards.length ? null : h("li", { class: "px-1 py-1 text-xs leading-relaxed text-ink-2" }, EMPTY[col]),
    ),
  );
}

function addInput() {
  return h("input", {
    id: "add-card",
    class:
      "mx-2 mb-2 rounded-lg border border-field bg-card px-3 py-2 text-sm transition-colors hover:border-ink-2 focus:border-accent focus:outline-none",
    placeholder: "Add a card and press Enter",
    maxlength: 500,
    "aria-label": "Add a card to Inbox",
    onkeydown: async (e: KeyboardEvent) => {
      const input = e.currentTarget as HTMLInputElement;
      if (e.key !== "Enter" || !input.value.trim()) return;
      await invoke("add_card", { title: input.value });
      input.value = "";
      await render();
    },
  });
}

const SOURCE_LABEL = { email: "From email", discord: "From Discord", itslearning: "From itslearning" } as const;

// A single click opens the card after a short wait, so a double-click on the title can rename instead.
let clickTimer: number | undefined;

// "Due today" in highlighter, overdue in red, everything else quiet.
function dueChip(due: string, done: boolean) {
  const d = new Date(`${due}T00:00:00`);
  const days = Math.round((d.getTime() - new Date().setHours(0, 0, 0, 0)) / 86_400_000); // setHours returns local midnight in ms
  const date = d.toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
  const [text, cls] = done
    ? [date, ""]
    : days < 0
      ? [`Overdue, ${date}`, "bg-danger/12 font-medium text-danger"]
      : days === 0
        ? ["Due today", "bg-mark font-medium text-mark-ink"]
        : days === 1
          ? ["Due tomorrow", ""]
          : [`Due ${date}`, ""];
  return h("span", { class: `-mx-1 flex items-center gap-1 rounded px-1 ${cls}` }, icon("calendar", "size-3"), text);
}

// How close a deadline card is. Red on the day (and after), orange the day before, violet from the
// Monday of the deadline's week. Before that week, and once done, the card looks like any other.
type Stage = "passed" | "today" | "tomorrow" | "week";
export function deadlineStage(card: Card): Stage | null {
  if (!card.deadline || !card.due || card.col === "done") return null;
  const due = new Date(`${card.due}T00:00:00`);
  const today = new Date().setHours(0, 0, 0, 0);
  const days = Math.round((due.getTime() - today) / 86_400_000);
  const monday = new Date(due).setDate(due.getDate() - ((due.getDay() + 6) % 7));
  return days < 0 ? "passed" : days === 0 ? "today" : days === 1 ? "tomorrow" : today >= monday ? "week" : null;
}

// The card's colored edge and tint (over its own background, so text keeps its contrast), and its label.
export const STAGE: Record<Stage, { card: string; chip: string }> = {
  passed: { card: "border-l-4 border-danger", chip: "bg-danger/12 text-danger" },
  today: { card: "border-l-4 border-danger bg-linear-to-r from-danger/15 to-danger/15", chip: "bg-danger text-danger-ink" },
  tomorrow: { card: "border-l-4 border-soon bg-linear-to-r from-soon/15 to-soon/15", chip: "bg-soon text-soon-ink" },
  week: { card: "border-l-4 border-deadline bg-linear-to-r from-deadline/12 to-deadline/12", chip: "bg-deadline text-deadline-ink" },
};

export function deadlineChip(card: Card, stage: Stage) {
  const date = new Date(`${card.due}T00:00:00`).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
  const text = { passed: `Deadline passed, ${date}`, today: "Deadline today", tomorrow: "Deadline tomorrow", week: `Deadline ${date}` }[stage];
  return h("span", { class: `-mx-1 flex items-center gap-1 rounded px-1 font-medium ${STAGE[stage].chip}` }, icon("calendar", "size-3"), text);
}

// The day already says the date, so a card shows its due chip only where it adds something:
// overdue in today, and in Inbox, Doing, and Done.
function showsDue(card: Card, slot: string) {
  if (!card.due) return false;
  return !isDay(slot) || card.due < days()[0];
}

function cardEl(card: Card, slot: string) {
  const done = card.col === "done";
  const stage = deadlineStage(card);
  const title = h(
    "span",
    { class: `min-w-0 flex-1 hyphens-auto break-words ${done ? "text-ink-2" : ""}` },
    card.title,
  );
  // A given task says who gave it, and "New" in the accent color until you open it.
  const task = card.task_id && cal ? cal.tasks.find((t) => t.id === card.task_id) : undefined;
  const meta = [
    card.task_new ? h("span", { class: "-mx-1 rounded bg-accent px-1 font-medium text-accent-ink" }, "New") : null,
    task && cal ? h("span", { class: "flex items-center gap-1" }, icon("users", "size-3"), `Fra ${nameOf(cal, task.created_by)}`) : null,
    stage ? deadlineChip(card, stage) : showsDue(card, slot) ? dueChip(card.due!, done) : null,
    card.note_id ? h("span", { class: "flex items-center gap-1", title: "Has a note" }, icon("note", "size-3"), "Note") : null,
    card.source !== "manual"
      ? h("span", { class: "flex items-center gap-1", title: SOURCE_LABEL[card.source] }, icon(card.source, "size-3"), SOURCE_LABEL[card.source])
      : null,
  ].filter((x) => x);
  // The card moves forward with buttons: Doing and Done before it starts, Done while in Doing,
  // and Remove card once it's done. Moves use the same move as a drop; removing offers Undo.
  const step = (label: string, aria: string, run: () => void, cls = "hover:text-ink") =>
    h(
      "button",
      { class: `rounded-md px-1.5 py-0.5 text-xs text-ink-2 transition-colors duration-150 hover:bg-panel ${cls}`, "aria-label": aria, onclick: run },
      label,
    );
  const moves = done
    ? [step("Remove card", `Remove ${card.title}`, () => remove(card), "hover:text-danger")]
    : (["doing", "done"] as const)
        .filter((col) => card.col !== col)
        .map((col) => {
          const name = col === "doing" ? "Doing" : "Done";
          return step(name, `Move ${card.title} to ${name}`, () => move(card.id, col, 0));
        });
  return h(
    "li",
    {
      tabindex: 0,
      "data-id": card.id,
      "aria-describedby": "card-keys",
      class: `group relative cursor-grab rounded-lg bg-card px-3 py-2.5 text-sm leading-snug shadow-card transition-shadow duration-150 hover:shadow-raised active:cursor-grabbing ${
        stage ? STAGE[stage].card : ""
      }`,
      onkeydown: (e: KeyboardEvent) => cardKeys(e, card, slot, title),
      onclick: (e: MouseEvent) => {
        if (justDragged || (e.target as Element).closest("button, input")) return;
        clearTimeout(clickTimer);
        if (e.detail === 1) clickTimer = window.setTimeout(() => openCard(card), 220);
      },
      ondblclick: (e: MouseEvent) => {
        if ((e.target as Element).closest("button, input")) return;
        clearTimeout(clickTimer);
        startRename(card, title);
      },
      onpointerdown: (e: PointerEvent) => startDrag(e, card),
    },
    h(
      "div",
      { class: "flex items-start gap-2" },
      done ? h("span", { class: "mt-0.5 text-accent", title: "Done" }, icon("check")) : null,
      title,
      h(
        "button",
        {
          class:
            "absolute top-1.5 right-1.5 rounded-md bg-card p-1 text-ink-2 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100 hover:text-danger focus-visible:opacity-100",
          title: "Delete card",
          "aria-label": `Delete ${card.title}`,
          onclick: () => remove(card),
        },
        icon("x"),
      ),
    ),
    h(
      "div",
      { class: "mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-ink-2" },
      ...meta,
      h("span", { class: "-my-0.5 -mr-1.5 ml-auto flex gap-0.5" }, ...moves),
    ),
  );
}

// ---- Card details ----

type InboxItem = { sender: string | null; subject: string | null; received_at: string | null };

// Where a card came from: "Added by hand", or the email's sender, subject, and date.
function sourceDetails(card: Card, item: InboxItem | null) {
  if (card.source === "manual") return h("dd", {}, "Added by hand");
  if (!item) return h("dd", {}, SOURCE_LABEL[card.source]);
  const when = item.received_at ? new Date(item.received_at).toLocaleString("en-GB", { dateStyle: "medium", timeStyle: "short" }) : null;
  return h(
    "dd",
    { class: "flex min-w-0 flex-col" },
    h("span", { class: "flex items-center gap-1.5" }, icon(card.source, "size-3.5 text-ink-2"), SOURCE_LABEL[card.source]),
    item.subject ? h("span", { class: "font-medium break-words" }, item.subject) : null,
    h("span", { class: "text-xs text-ink-2 break-words" }, [item.sender, when].filter(Boolean).join(", ")),
  );
}

// The card sheet. The Calendar view opens it too, with `onClose` to redraw itself instead of the board.
export async function openCard(card: Card, onClose?: () => void) {
  if (card.task_new) {
    card.task_new = false;
    await invoke("task_seen", { id: card.id });
  }
  const note = card.note_id ? await invoke<Note | null>("card_note", { cardId: card.id }) : null;
  const item = card.inbox_item_id ? await invoke<InboxItem | null>("inbox_item", { id: card.inbox_item_id }) : null;
  const noteArea = h("section", { class: "flex min-h-0 flex-1 flex-col gap-2", "aria-label": "Note" });
  const showNote = (n: Note) =>
    noteArea.replaceChildren(h("h3", { class: "text-xs font-semibold text-ink-2" }, "Note"), noteEditor(n, "flex-1"));
  if (note) showNote(note);
  else
    noteArea.replaceChildren(
      h(
        "button",
        {
          class: `${btn.secondary} self-start`,
          onclick: async () => {
            showNote(await invoke<Note>("create_note", { cardId: card.id }));
            noteArea.querySelector("textarea")!.focus();
          },
        },
        icon("plus"),
        "Add a note",
      ),
    );

  // Sharing puts the card in everyone's calendar as an all-day event on its date, linked to the card.
  const shareRow = h("dd", { class: "flex flex-col items-start gap-1" });
  const shareHelp = h("span", { class: "text-xs text-ink-2", "aria-live": "polite" }, "Everyone sees it as an all-day event on the card's date.");
  const share = async () => {
    try {
      const event = await invoke<SharedEvent>("calendar_share_card", { id: card.id });
      card.shared_event_id = event.id;
      showShare();
    } catch (e) {
      shareHelp.className = "text-xs text-danger";
      shareHelp.textContent = String(e);
    }
  };
  const showShare = async () => {
    const calendar = await loadCalendar();
    if (card.shared_event_id)
      shareRow.replaceChildren(
        h("span", { class: "flex items-center gap-1.5" }, icon("users", "size-3.5 text-ink-2"), "In the shared calendar"),
        h("span", { class: "text-xs text-ink-2" }, "Renaming the card or changing its date changes the event too."),
      );
    else if (calendar.status !== "ready")
      shareRow.replaceChildren(h("span", { class: "text-xs text-ink-2" }, "Sign in to the shared calendar in Settings to share this card."));
    else
      shareRow.replaceChildren(
        h(
          "span",
          { class: "flex flex-wrap gap-2" },
          h("button", { class: btn.secondary, onclick: share }, icon("users"), "Add to shared calendar"),
          card.task_id ? null : h("button", { class: btn.secondary, onclick: () => giveTask(calendar, { title: card.title, due: card.due }) }, "Give to…"),
        ),
        shareHelp,
      );
  };
  await showShare();

  const dialog = h(
    "dialog",
    {
      class:
        "fixed inset-y-0 right-0 left-auto m-0 h-dvh max-h-dvh w-[28rem] max-w-full border-l border-line bg-card text-ink shadow-raised",
      "aria-label": card.title,
    },
    h(
      "div",
      { class: "flex h-full flex-col gap-5 p-5" },
      h(
        "div",
        { class: "flex items-start gap-2" },
        h("input", {
          class: "min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-2 py-1 -ml-2 text-lg font-semibold hover:border-line focus:border-accent focus:outline-none",
          value: card.title,
          maxlength: 500,
          "aria-label": "Card title",
          onchange: async (e: Event) => {
            const title = (e.currentTarget as HTMLInputElement).value.trim();
            if (!title) return;
            await invoke("rename_card", { id: card.id, title });
            pushShared(card);
          },
        }),
        h("button", { class: btn.icon, title: "Close", "aria-label": "Close", onclick: () => dialog.close() }, icon("x")),
      ),
      h(
        "dl",
        { class: "grid grid-cols-[6rem_1fr] items-center gap-y-3 text-sm" },
        h("dt", { class: "text-ink-2" }, "Due"),
        h(
          "dd",
          {},
          h("input", {
            type: "date",
            class: "rounded-md border border-field bg-card px-2 py-1 focus:border-accent focus:outline-none",
            value: card.due ?? "",
            "aria-label": "Due date",
            onchange: async (e: Event) => {
              await invoke("set_due", { id: card.id, due: (e.currentTarget as HTMLInputElement).value || null });
              pushShared(card);
            },
          }),
        ),
        h("dt", { class: "text-ink-2" }, "Deadline"),
        h(
          "dd",
          {},
          h(
            "label",
            { class: "flex items-center gap-2" },
            h("input", {
              type: "checkbox",
              class: "size-4 accent-[var(--deadline)]",
              checked: card.deadline,
              onchange: (e: Event) => invoke("set_deadline", { id: card.id, on: (e.currentTarget as HTMLInputElement).checked }),
            }),
            "Hand-in deadline: colored as it gets close",
          ),
        ),
        h("dt", { class: "self-start text-ink-2" }, "Source"),
        sourceDetails(card, item),
        h("dt", { class: "self-start text-ink-2" }, "Calendar"),
        shareRow,
      ),
      noteArea,
      h(
        "div",
        { class: "flex justify-end border-t border-line pt-4" },
        h(
          "button",
          {
            class: btn.danger,
            onclick: async () => {
              deleted = true;
              dialog.close();
              await remove(card);
              onClose?.();
            },
          },
          icon("trash"),
          "Delete card",
        ),
      ),
    ),
  );
  let deleted = false;
  dialog.addEventListener("close", () => {
    dialog.remove();
    if (deleted) return;
    if (onClose) return onClose();
    focusId = card.id;
    render();
  });
  // A click and a quick Enter can both ask to open a card; one sheet is enough.
  if (document.querySelector("dialog[open]")) return;
  document.body.append(dialog);
  dialog.showModal();
}

function startRename(card: Card, span: HTMLElement) {
  const input = h("input", {
    class: "min-w-0 flex-1 rounded border border-accent bg-card px-1 -mx-1 focus:outline-none",
    value: card.title,
    maxlength: 500,
    "aria-label": "Card title",
  });
  let done = false;
  const finish = async (save: boolean) => {
    if (done) return;
    done = true;
    const title = input.value.trim();
    try {
      if (save && title && title !== card.title) {
        await invoke("rename_card", { id: card.id, title });
        pushShared(card);
      }
    } finally {
      // Even when saving fails (the error shows as a toast), the input goes back to a title.
      focusId = card.id;
      await render();
    }
  };
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") finish(true);
    else if (e.key === "Escape") finish(false);
  });
  input.addEventListener("blur", () => finish(true));
  span.replaceWith(input);
  input.focus();
  input.select();
}

async function remove(card: Card) {
  // Keep keyboard focus on the board: the next card in the column, or the one before.
  const li = root.querySelector<HTMLElement>(`[data-id="${card.id}"]`);
  const near = (li?.nextElementSibling ?? li?.previousElementSibling) as HTMLElement | null;
  if (li?.contains(document.activeElement) && near?.dataset.id) focusId = Number(near.dataset.id);
  // The backend returns the row as it was, so Undo restores what was deleted, not a stale copy.
  const saved = await invoke<Card | null>("delete_card", { id: card.id });
  await render();
  if (!saved) return;
  toast(`Deleted "${saved.title}".`, {
    action: {
      label: "Undo",
      run: async () => {
        focusId = await invoke<number>("restore_card", { card: saved });
        await render();
      },
    },
  });
}

// A shared card's event, and a given task, follow the card's title, date, and (for a task) Done.
function pushShared(card: Card) {
  if (card.shared_event_id || card.task_id) invoke("calendar_push_card", { id: card.id }).catch((e) => toast(String(e), { danger: true }));
}

// ---- Moving cards ----

let dragId: number | null = null;
const marker = h("li", { class: "pointer-events-none -my-1 h-0.5 rounded-full bg-accent", "aria-hidden": "true" });

// Cards in a column other than the one being dragged.
function others(section: HTMLElement) {
  return [...section.querySelectorAll<HTMLElement>("li[data-id]")].filter((li) => Number(li.dataset.id) !== dragId);
}

function dropIndex(section: HTMLElement, y: number) {
  return others(section).filter((li) => {
    const r = li.getBoundingClientRect();
    return r.top + r.height / 2 < y;
  }).length;
}

function showMarker(section: HTMLElement, y: number) {
  // A folded day has no list; the marker then shows under its header.
  const list = section.querySelector("ol") ?? section;
  const before = others(section)[dropIndex(section, y)];
  if (before) list.insertBefore(marker, before);
  else list.append(marker);
}

// Dragging a card with the pointer. Built on pointer events, not the browser's drag and drop,
// so Tauri's own file drop can stay on.
let justDragged = false; // the click that ends a drag must not open the card

function startDrag(e: PointerEvent, card: Card) {
  if (e.button !== 0 || (e.target as Element).closest("button, input")) return;
  const li = e.currentTarget as HTMLElement;
  const [sx, sy] = [e.clientX, e.clientY];
  let ghost: HTMLElement | null = null;
  let lastArrow: Element | null = null;

  const under = (ev: PointerEvent) => document.elementFromPoint(ev.clientX, ev.clientY);
  const end = () => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("keydown", onKey);
    ghost?.remove();
    marker.remove();
    li.classList.remove("opacity-40");
    document.body.classList.remove("select-none", "cursor-grabbing");
    if (dragId != null) {
      justDragged = true;
      setTimeout(() => (justDragged = false));
    }
    dragId = null;
  };
  const onMove = (ev: PointerEvent) => {
    if (dragId == null) {
      if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 5) return; // a click, not a drag yet
      dragId = card.id;
      const r = li.getBoundingClientRect();
      ghost = li.cloneNode(true) as HTMLElement;
      Object.assign(ghost.style, { position: "fixed", left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, pointerEvents: "none", zIndex: "50" });
      ghost.classList.add("shadow-raised", "rotate-1");
      document.body.append(ghost);
      li.classList.add("opacity-40");
      document.body.classList.add("select-none", "cursor-grabbing");
    }
    ghost!.style.transform = `translate(${ev.clientX - sx}px, ${ev.clientY - sy}px)`;
    const target = under(ev);
    // Onto a week arrow: slide once per visit, so a card can reach a hidden week.
    const arrow = target?.closest("button[data-dir]") ?? null;
    if (arrow && arrow !== lastArrow) (arrow as HTMLElement).click();
    lastArrow = arrow;
    const zone = target?.closest<HTMLElement>("[data-slot]");
    if (!zone) return marker.remove();
    showMarker(zone, ev.clientY);
    // Near the top or bottom of a list, scroll it. ponytail: scrolls only while the pointer moves.
    const list = zone.querySelector("ol");
    if (list) {
      const r = list.getBoundingClientRect();
      if (ev.clientY < r.top + 40) list.scrollTop -= 12;
      else if (ev.clientY > r.bottom - 40) list.scrollTop += 12;
    }
  };
  const onUp = async (ev: PointerEvent) => {
    const id = dragId;
    const zone = id == null ? null : under(ev)?.closest<HTMLElement>("[data-slot]");
    const index = zone ? dropIndex(zone, ev.clientY) : 0; // before end(): it counts the other cards
    end();
    if (id != null && zone) await move(id, zone.dataset.slot!, index);
  };
  const onKey = (ev: KeyboardEvent) => {
    if (ev.key === "Escape") end();
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  window.addEventListener("keydown", onKey);
}

// Screen readers hear where a moved card went. Lives outside the board so re-renders keep it.
const status = document.body.appendChild(h("p", { class: "sr-only", "aria-live": "polite" }));

// Moving a card to a day sets its due date; to Inbox, clears a date in the next 4 weeks, which
// would otherwise pull it straight back to its day.
async function move(id: number, slot: string, index: number) {
  const card = cards.find((c) => c.id === id)!;
  const ds = days();
  const from = slotOf(card, ds);
  if (isDay(slot)) {
    // An overdue card already shows in today; dropping it there keeps its real due date.
    if (slot !== from || !card.due) {
      await invoke("set_due", { id, due: slot });
      if (!card.task_id) pushShared(card); // a task pushes once, below, after the column is set too
    }
    if (card.col !== "inbox") await invoke("move_card", { id, col: "inbox", index: 0 });
  } else {
    if (slot === "inbox" && card.due && card.due <= ds[ds.length - 1]) await invoke("set_due", { id, due: null });
    await invoke("move_card", { id, col: slot, index });
  }
  // A task's new deadline, or its Done, goes back to whoever gave it.
  if (card.task_id && (isDay(slot) || (slot === "done") !== (from === "done"))) pushShared(card);
  if (slot === "done" && from !== "done") swipeId = id;
  status.textContent = isDay(slot) ? `Moved to ${slotLabel(slot)}.` : `Moved to ${slotLabel(slot)}, position ${index + 1}.`;
  focusId = id;
  await render();
}

// Arrow keys move focus between cards. Alt+arrow keys move the focused card: in the weeks,
// up and down is a day and left and right is a week; elsewhere up and down reorders.
function cardKeys(e: KeyboardEvent, card: Card, slot: string, title: HTMLElement) {
  if (e.target !== e.currentTarget) return;
  const li = e.currentTarget as HTMLElement;
  const step = { ArrowUp: [0, -1], ArrowDown: [0, 1], ArrowLeft: [-1, 0], ArrowRight: [1, 0] }[e.key];

  if (e.key === "Enter") openCard(card);
  else if (e.key === "F2") startRename(card, title);
  else if (e.key === "Delete") remove(card);
  else if (step && e.altKey) {
    e.preventDefault();
    const [dc, di] = step;
    const all = slots();
    const s = all.indexOf(slot);
    const lastDay = all.length - 3;
    if (isDay(slot)) {
      const t = s + (dc ? dc * 7 : di);
      move(card.id, t < 1 ? "inbox" : t > lastDay ? "doing" : all[t], 0);
    } else if (dc && all[s + dc]) move(card.id, all[s + dc], 0);
    else if (di) {
      const siblings = [...li.parentElement!.querySelectorAll<HTMLElement>("li[data-id]")];
      const i = siblings.indexOf(li);
      if (siblings[i + di]) move(card.id, slot, i + di);
    }
  } else if (step) {
    e.preventDefault();
    const [dc, di] = step;
    const section = li.closest("section")!;
    const sections = [...root.querySelectorAll("section")];
    const inSection = [...section.querySelectorAll<HTMLElement>("li[data-id]")];
    const target = dc
      ? sections[sections.indexOf(section) + dc]?.querySelector<HTMLElement>("li[data-id]")
      : inSection[inSection.indexOf(li) + di];
    target?.focus();
  }
}
