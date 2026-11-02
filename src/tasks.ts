import { invoke } from "@tauri-apps/api/core";
import { btn, h, toast } from "./dom";
import { icon } from "./icons";
import { nameOf, type Calendar } from "./calendar";

// A task one member gives another (src-tauri/src/tasks.rs). It lands as a card in their Inbox.
export type Task = {
  id: string;
  title: string;
  note: string;
  assignee: string;
  due: string; // YYYY-MM-DD
  done: boolean;
  created_by: string | null;
  created_at: string;
  updated_by: string | null;
  updated_at: string;
};

const dayLabel = (day: string) => new Date(`${day}T00:00:00`).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
const today = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};

function sheet(label: string, ...children: (HTMLElement | null)[]) {
  const dialog = h(
    "dialog",
    { class: "fixed inset-y-0 right-0 left-auto m-0 h-dvh max-h-dvh w-[28rem] max-w-full border-l border-line bg-card text-ink shadow-raised", "aria-label": label },
    ...children,
  );
  dialog.addEventListener("close", () => dialog.remove());
  document.body.append(dialog);
  dialog.showModal();
  return dialog;
}

// The form for giving a task: title, note, member, deadline. A card's "Give to…" fills in its title and date.
export function giveTask(cal: Calendar, preset?: { title?: string; due?: string | null }) {
  const field = "rounded-md border border-field bg-card px-2 py-1 focus:border-accent focus:outline-none";
  const title = h("input", { id: "task-title", class: field, maxlength: 200, value: preset?.title ?? "" });
  const member = h(
    "select",
    { id: "task-member", class: field },
    h("option", { value: "" }, "Pick a member"),
    ...cal.members.filter((m) => m.id !== cal.me).map((m) => h("option", { value: m.id }, m.name)),
  );
  const due = h("input", { id: "task-due", type: "date", class: field, min: today(), value: preset?.due && preset.due >= today() ? preset.due : "" });
  const note = h("textarea", { id: "task-note", class: `${field} min-h-24 resize-y py-1.5`, maxlength: 4000 });
  const message = h("p", { class: "min-h-5 text-sm text-danger", role: "alert" });
  const give = h("button", { type: "submit", class: btn.primary }, "Give task");

  const submit = async (e: SubmitEvent) => {
    e.preventDefault();
    message.textContent = "";
    give.disabled = true;
    try {
      // The backend checks the title, member, and deadline, and says what's missing.
      await invoke("task_give", { draft: { title: title.value, note: note.value, assignee: member.value, due: due.value } });
      dialog.close();
      toast(`Gave "${title.value.trim()}" to ${nameOf(cal, member.value)}.`);
    } catch (err) {
      message.textContent = String(err); // the form keeps what you typed
    } finally {
      give.disabled = false;
    }
  };

  const row = (id: string, label: string, control: HTMLElement) => [h("label", { for: id, class: "text-ink-2" }, label), control];
  const dialog = sheet(
    "Give a task",
    h(
      "form",
      { class: "flex h-full flex-col gap-4 p-5", onsubmit: submit },
      h(
        "div",
        { class: "flex items-start gap-2" },
        h("h2", { class: "flex-1 text-lg font-semibold" }, "Give a task"),
        h("button", { type: "button", class: btn.icon, title: "Close", "aria-label": "Close", onclick: () => dialog.close() }, icon("x")),
      ),
      h("p", { class: "text-xs text-ink-2" }, "It lands in their Inbox on its deadline. Anyone can change it later, and every change is logged."),
      h(
        "div",
        { class: "grid grid-cols-[5rem_1fr] items-center gap-x-3 gap-y-3 text-sm" },
        ...row("task-title", "Task", title),
        ...row("task-member", "For", member),
        ...row("task-due", "Deadline", due),
        ...row("task-note", "Note", note),
      ),
      message,
      h(
        "div",
        { class: "mt-auto flex items-center gap-2 border-t border-line pt-4" },
        h("button", { type: "button", class: `${btn.secondary} ml-auto`, onclick: () => dialog.close() }, "Cancel"),
        give,
      ),
    ),
  );
  (preset?.title ? member : title).focus();
}

// Tasks you gave, with who has them, their deadline, and whether they're done.
export function showGiven(cal: Calendar) {
  const mine = cal.tasks.filter((t) => t.created_by === cal.me);
  const dialog = sheet(
    "Tasks you gave",
    h(
      "div",
      { class: "flex h-full flex-col gap-4 p-5" },
      h(
        "div",
        { class: "flex items-start gap-2" },
        h("h2", { class: "flex-1 text-lg font-semibold" }, "Tasks you gave"),
        h("button", { type: "button", class: btn.icon, title: "Close", "aria-label": "Close", onclick: () => dialog.close() }, icon("x")),
      ),
      mine.length
        ? h(
            "ul",
            { class: "flex min-h-0 flex-col gap-2 overflow-y-auto" },
            ...mine.map((t) =>
              h(
                "li",
                { class: "rounded-lg bg-panel px-3 py-2 text-sm" },
                h("div", { class: "flex items-baseline gap-2" }, h("span", { class: `flex-1 break-words ${t.done ? "text-ink-2 line-through" : ""}` }, t.title), h("span", { class: "text-xs text-ink-2" }, t.done ? "Done" : "Open")),
                h(
                  "p",
                  { class: "mt-1 text-xs text-ink-2" },
                  [`For ${nameOf(cal, t.assignee)}`, `deadline ${dayLabel(t.due)}`, t.updated_by && t.updated_by !== t.created_by ? `changed by ${nameOf(cal, t.updated_by)}` : null]
                    .filter(Boolean)
                    .join(", ") + ".",
                ),
              ),
            ),
          )
        : h("p", { class: "text-sm text-ink-2" }, "You haven't given any tasks yet. Use Give a task above the board."),
    ),
  );
}
