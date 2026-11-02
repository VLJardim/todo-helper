type Child = Node | string | number | null | undefined | false;

// Creates an element. Strings become text nodes, so outside text is never parsed as HTML.
// Props starting with "on" become event listeners; known DOM properties are set directly;
// everything else becomes an attribute.
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Record<string, any> = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v == null || v === false) continue;
    if (k.startsWith("on")) el.addEventListener(k.slice(2).toLowerCase(), v);
    else if (k in el && !k.includes("-")) (el as any)[k] = v;
    else el.setAttribute(k, v === true ? "" : String(v));
  }
  for (const c of children) if (c != null && c !== false) el.append(typeof c === "number" ? String(c) : c);
  return el;
}

// Replaces a view's contents but keeps each scrolling area where it was, down and sideways. Every
// redraw of a view builds the same areas in the same order, so they match up by position.
const SCROLLS = ".overflow-auto, .overflow-y-auto, .overflow-x-auto";
export function redraw(root: HTMLElement, ...nodes: Node[]) {
  const at = [...root.querySelectorAll(SCROLLS)].map((e) => [e.scrollTop, e.scrollLeft]);
  root.replaceChildren(...nodes);
  root.querySelectorAll(SCROLLS).forEach((e, i) => {
    e.scrollTop = at[i]?.[0] ?? 0;
    e.scrollLeft = at[i]?.[1] ?? 0;
  });
}

// Shared button styles, so the same action looks the same everywhere.
const base =
  "inline-flex items-center justify-center gap-1.5 rounded-lg px-3 py-1.5 text-sm font-medium transition-colors disabled:opacity-50";
export const btn = {
  primary: `${base} bg-accent text-accent-ink hover:brightness-110 active:brightness-95`,
  secondary: `${base} border border-line bg-card text-ink hover:bg-panel active:bg-line`,
  danger: `${base} text-danger hover:bg-danger/12 active:bg-danger/12`,
  icon: "rounded-md p-1.5 text-ink-2 transition-colors hover:bg-panel hover:text-ink",
};

// One message line at the bottom of the window. An action (such as Undo) also runs on Ctrl+Z.
let toastEl: HTMLElement | undefined;
let toastTimer: number | undefined;
let toastAction: (() => unknown) | undefined;

export function toast(text: string, opts: { danger?: boolean; action?: { label: string; run: () => unknown } } = {}) {
  toastEl ??= document.body.appendChild(h("div", { class: "fixed bottom-4 left-1/2 z-50 -translate-x-1/2" }));
  toastAction = opts.action?.run;
  toastEl.setAttribute("aria-live", opts.danger ? "assertive" : "polite");
  const run = () => {
    const action = toastAction;
    hide();
    action?.();
  };
  const hide = () => {
    toastAction = undefined;
    toastEl!.replaceChildren();
  };
  toastEl.replaceChildren(
    h(
      "div",
      {
        class: `flex max-w-[60ch] items-center gap-4 rounded-lg px-4 py-2 text-sm shadow-raised ${
          opts.danger ? "bg-danger text-danger-ink" : "bg-ink text-bg"
        }`,
      },
      h("span", {}, text),
      opts.action
        ? h("button", { class: "rounded font-semibold underline underline-offset-2 hover:no-underline", onclick: run }, opts.action.label, h("span", { class: "sr-only" }, " (Ctrl+Z)"))
        : null,
    ),
  );
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(hide, opts.action ? 8000 : 6000);
}

document.addEventListener("keydown", (e) => {
  const typing = (e.target as Element).closest?.("input, textarea");
  if (toastAction && e.ctrlKey && e.key.toLowerCase() === "z" && !typing) {
    e.preventDefault();
    toastEl!.querySelector("button")!.click(); // same path as clicking Undo
  }
});

// Asks before a destructive action. Resolves true only if the person confirms.
export function confirmDialog(message: string, confirmLabel: string): Promise<boolean> {
  return new Promise((resolve) => {
    const dialog = h(
      "dialog",
      { class: "m-auto w-80 rounded-xl bg-card p-5 text-ink shadow-raised" },
      h("p", { class: "text-sm" }, message),
      h(
        "form",
        { method: "dialog", class: "mt-5 flex justify-end gap-2" },
        h("button", { value: "cancel", class: btn.secondary, autofocus: true }, "Cancel"),
        h("button", { value: "ok", class: `${base} bg-danger text-danger-ink hover:brightness-110` }, confirmLabel),
      ),
    );
    dialog.addEventListener("close", () => {
      resolve(dialog.returnValue === "ok");
      dialog.remove();
    });
    document.body.append(dialog);
    dialog.showModal();
  });
}
