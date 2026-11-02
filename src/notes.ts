import { invoke } from "@tauri-apps/api/core";
import { btn, confirmDialog, h, redraw, toast } from "./dom";
import { icon } from "./icons";

export type Note = {
  id: number;
  body: string;
  card_title: string | null;
  updated_at: string;
};

// Every keystroke asks for a save. While one save runs, later keystrokes only replace the text
// waiting to go next, so a fast typist never builds a queue and the newest text always lands last.
const waiting = new Map<number, string>();
let draining: Promise<void> | null = null;

function saveNote(id: number, body: string) {
  waiting.set(id, body);
  draining ??= drain();
}

async function drain() {
  for (const [id, body] of waiting) {
    waiting.delete(id);
    try {
      await invoke("save_note", { id, body });
    } catch (e) {
      toast(`The note didn't save: ${e}`, { danger: true });
    }
  }
  draining = null;
}

export function noteTitle(body: string) {
  return body.split("\n", 1)[0].trim() || "Untitled note";
}

export function noteEditor(note: Note, cls = "", onInput?: (body: string) => void) {
  return h("textarea", {
    class: `min-h-40 w-full resize-none rounded-lg border border-field bg-card p-3 text-sm leading-relaxed focus:border-accent focus:outline-none ${cls}`,
    value: note.body,
    placeholder: "Write here. It saves as you type.",
    "aria-label": "Note text",
    spellcheck: true,
    oninput: (e: Event) => {
      note.body = (e.currentTarget as HTMLTextAreaElement).value;
      saveNote(note.id, note.body);
      onInput?.(note.body);
    },
  });
}

function when(updatedAt: string) {
  return new Date(updatedAt.replace(" ", "T") + "Z").toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

let root: HTMLElement;
let selectedId: number | null = null;

export function mountNotes(el: HTMLElement) {
  root = el;
  return render();
}

async function render(focusEditor = false) {
  const notes = await invoke<Note[]>("list_notes");
  if (!notes.some((n) => n.id === selectedId)) selectedId = notes[0]?.id ?? null;
  const selected = notes.find((n) => n.id === selectedId);

  const items = notes.map((n) => {
    const title = h("span", { class: "block truncate font-medium" }, noteTitle(n.body));
    const button = h(
      "button",
      {
        class:
          "w-full rounded-lg px-3 py-2 text-left text-sm transition-colors hover:bg-panel aria-[current=true]:bg-panel",
        "aria-current": String(n.id === selectedId),
        onclick: async () => {
          selectedId = n.id;
          await render();
          // The list was rebuilt, so put focus back on the note you chose.
          root.querySelector<HTMLElement>('[aria-current="true"]')?.focus();
        },
      },
      title,
      h("span", { class: "block truncate text-xs text-ink-2" }, n.card_title ? `On card: ${n.card_title}` : when(n.updated_at)),
    );
    return { note: n, title, li: h("li", {}, button) };
  });

  const list = h(
    "aside",
    { class: "flex w-64 shrink-0 flex-col gap-3 border-r border-line p-3" },
    h(
      "button",
      {
        class: btn.primary,
        onclick: async () => {
          selectedId = (await invoke<Note>("create_note", { cardId: null })).id;
          render(true);
        },
      },
      icon("plus"),
      "New note",
    ),
    h("ul", { class: "flex min-h-0 flex-col gap-0.5 overflow-y-auto", "aria-label": "Notes" }, ...items.map((i) => i.li)),
  );

  const pane = selected
    ? h(
        "section",
        { class: "flex min-w-0 flex-1 flex-col gap-3 p-5", "aria-label": "Note editor" },
        h(
          "div",
          { class: "flex items-center justify-between gap-3 text-xs text-ink-2" },
          h("span", {}, selected.card_title ? `Linked to the card "${selected.card_title}"` : "Not linked to a card"),
          h(
            "button",
            {
              class: btn.danger,
              onclick: async () => {
                if (!(await confirmDialog("Delete this note? You can't undo this.", "Delete note"))) return;
                await invoke("delete_note", { id: selected.id });
                selectedId = null;
                render();
              },
            },
            icon("trash"),
            "Delete note",
          ),
        ),
        noteEditor(selected, "flex-1 max-w-[72ch] text-[15px]", (body) => {
          items.find((i) => i.note.id === selected.id)!.title.textContent = noteTitle(body);
        }),
      )
    : h(
        "section",
        { class: "flex flex-1 items-center justify-center p-5 text-sm text-ink-2" },
        h("p", {}, "No notes yet. Press New note to write one, or add a note from a card."),
      );

  redraw(root, h("div", { class: "flex h-full" }, list, pane));
  if (focusEditor) root.querySelector("textarea")?.focus();
}
