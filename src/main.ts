import { h, toast } from "./dom";
import { icon, type IconName } from "./icons";
import { mountBoard } from "./board";
import { mountCalendar } from "./calendar";
import { mountNotes } from "./notes";
import { mountSessions } from "./sessions";
import { mountSettings } from "./settings";

type View = { label: string; icon: IconName; mount: (el: HTMLElement) => unknown };

const views: Record<string, View> = {
  board: { label: "Board", icon: "kanban", mount: mountBoard },
  calendar: { label: "Calendar", icon: "calendar", mount: mountCalendar },
  notes: { label: "Notes", icon: "notebook", mount: mountNotes },
  sessions: { label: "Sessions", icon: "terminal", mount: mountSessions },
  settings: { label: "Settings", icon: "sliders", mount: mountSettings },
};

const main = h("main", { class: "min-w-0 flex-1 overflow-hidden" });
const nav = h("nav", { class: "flex flex-col gap-0.5", "aria-label": "Views" });
const sidebar = h(
  "aside",
  { class: "flex w-48 shrink-0 flex-col gap-4 border-r border-line bg-bg p-3 max-lg:w-auto max-lg:p-2" },
  h("h1", { class: "px-3 pt-1 text-sm font-semibold tracking-tight max-lg:sr-only" }, "todo-helper"),
  nav,
  h("p", { id: "board-hint", class: "mt-auto px-3 text-xs leading-relaxed text-ink-2 max-lg:hidden" }, "Enter opens a card. Alt and an arrow key moves it."),
);

function show(name: string) {
  for (const b of nav.querySelectorAll("button")) {
    if (b.dataset.view === name) b.setAttribute("aria-current", "page");
    else b.removeAttribute("aria-current");
  }
  sidebar.querySelector("#board-hint")!.classList.toggle("invisible", name !== "board");
  // A fresh container per visit: background refreshes check root.isConnected, which turns false
  // as soon as you switch views, so a late refresh can never draw over another view.
  const container = h("div", { class: "h-full" });
  main.replaceChildren(container);
  views[name].mount(container);
}

for (const [name, v] of Object.entries(views)) {
  nav.append(
    h(
      "button",
      {
        "data-view": name,
        class:
          "flex items-center gap-2.5 rounded-lg px-3 py-2 text-left text-sm text-ink-2 transition-colors hover:bg-panel hover:text-ink aria-[current=page]:bg-panel aria-[current=page]:font-semibold aria-[current=page]:text-ink max-lg:p-2.5",
        title: v.label,
        onclick: () => show(name),
      },
      icon(v.icon),
      h("span", { class: "max-lg:sr-only" }, v.label),
    ),
  );
}

// A view can send you to another one, such as the Calendar to Settings to sign in.
window.addEventListener("show-view", (e) => show((e as CustomEvent<string>).detail));

// Every failed backend call ends up here, so errors are never silent.
window.addEventListener("unhandledrejection", (e) => toast(String(e.reason), { danger: true }));

document.getElementById("app")!.replaceChildren(sidebar, main);
show("board");
