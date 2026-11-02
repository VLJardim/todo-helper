import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { btn, h } from "./dom";
import { icon } from "./icons";

type Status = { ok: boolean; message: string; at: string };
type View = {
  plain: Record<string, string>;
  secrets: Record<string, boolean>;
  status: Record<string, Status | null>;
  store: string; // "Windows Credential Manager" or "Keychain"
  outlook_works: boolean;
};

let root: HTMLElement;
let view: View;
let pressed: string | undefined; // the source whose Check now was just pressed

export function mountSettings(el: HTMLElement) {
  root = el;
  return render();
}

// Sources whose Check now is still running; their button stays "Checking" through other redraws.
const running = new Set<string>();

// A finished check updates only the status lines, so half-typed fields are kept.
listen<string | null>("settings-changed", async (e) => {
  if (e.payload) running.delete(e.payload);
  if (!root?.isConnected) return;
  view = await invoke<View>("get_settings");
  // Disabling the pressed button moved focus to the page, so return it to the new button,
  // unless you have moved on to another field since.
  const active = document.activeElement;
  const lost = !active || active === document.body;
  const focused = (lost ? pressed : undefined) ?? active?.closest<HTMLElement>("[data-status]")?.dataset.status;
  pressed = undefined;
  for (const el of root.querySelectorAll<HTMLElement>("[data-status]")) el.replaceWith(statusLine(el.dataset.status!));
  // Keep keyboard focus on the Check now button that was pressed.
  if (focused) root.querySelector<HTMLElement>(`[data-status="${focused}"] button`)?.focus();
});

async function render() {
  view = await invoke<View>("get_settings");
  root.replaceChildren(
    h(
      "div",
      { class: "h-full overflow-y-auto" },
      h(
        "div",
        { class: "mx-auto flex max-w-2xl flex-col gap-8 p-6" },
        h(
          "header",
          {},
          h("h2", { class: "text-lg font-semibold" }, "Settings"),
          h(
            "p",
            { class: "text-sm text-ink-2" },
            `Passwords, tokens, and feed links are kept in ${view.store}, never in the app's database.`,
          ),
        ),
        view.outlook_works
          ? section(
              "outlook",
              "Outlook",
              "New mail in every account in classic Outlook for Windows becomes cards. Outlook must be running; no password needed.",
              plainField("email_interval_min", "Check every (minutes)", { type: "number", min: 1, max: 1440 }),
            )
          : h(
              "section",
              { class: "flex flex-col rounded-xl bg-panel p-5", "aria-labelledby": "h-outlook" },
              h("h3", { id: "h-outlook", class: "font-semibold" }, "Outlook"),
              h("p", { class: "text-sm text-ink-2" }, "Outlook only works on Windows, with classic Outlook, so it's off on this computer."),
            ),
        section(
          "discord",
          "Discord",
          "Messages in the channels you pick become cards. Needs a bot in the server.",
          secretField("discord_bot_token", "Bot token", "From the Bot page of your app at discord.com/developers"),
          plainField("discord_channel_ids", "Channel IDs", { placeholder: "123456789012345678, 234567890123456789" }),
          plainField("discord_interval_min", "Check every (minutes)", { type: "number", min: 1, max: 1440 }),
        ),
        section(
          "itslearning",
          "itslearning",
          "Your timetable for the next 26 weeks shows in the Calendar view. Only hand-in deadlines show on the board.",
          secretField("itslearning_feed_url", "Calendar feed link", "From Calendar, then Subscribe, in itslearning"),
          plainField("itslearning_interval_min", "Check every (minutes)", { type: "number", min: 1, max: 1440 }),
        ),
        section(
          "calendar",
          "Shared calendar",
          "Everyone's events in one calendar, in your own Supabase project. Anyone can change any event, and every change is logged with who made it.",
          plainField("calendar_url", "Project URL", { placeholder: "https://abcdefgh.supabase.co", spellcheck: false }),
          plainField("calendar_key", "Publishable key", { placeholder: "sb_publishable_...", spellcheck: false, autocomplete: "off" }),
          await accountField(),
        ),
        section(
          "gcal",
          "Google Calendar",
          "Copies your Google calendars into the shared calendar for the next 26 weeks, as you, with private events left out. It only ever changes its own copies, never events added in the app. Uses the shared calendar's sign-in above.",
          await calendarsField(),
          plainField("gcal_interval_min", "Check every (minutes)", { type: "number", min: 1, max: 1440 }),
        ),
      ),
    ),
  );
}

function section(source: string, title: string, about: string, ...fields: HTMLElement[]) {
  return h(
    "section",
    { class: "flex flex-col gap-4 rounded-xl bg-panel p-5", "aria-labelledby": `h-${source}` },
    h(
      "div",
      {},
      h("h3", { id: `h-${source}`, class: "font-semibold" }, title),
      h("p", { class: "text-sm text-ink-2" }, about),
    ),
    ...fields,
    statusLine(source),
  );
}

function statusLine(source: string) {
  const status = view.status[source];
  return h(
    "div",
    { class: "flex items-start gap-3 border-t border-line pt-4", "data-status": source },
    h(
      "p",
      { class: "flex flex-1 items-start gap-2 text-sm", role: "status" },
      h("span", {
        class: `mt-1.5 size-2 shrink-0 rounded-full ${!status ? "bg-line" : status.ok ? "bg-accent" : "bg-danger"}`,
        "aria-hidden": "true",
      }),
      h(
        "span",
        { class: status && !status.ok ? "text-danger" : "text-ink-2" },
        status
          ? `${status.message} Checked ${new Date(status.at).toLocaleString("en-GB", { dateStyle: "medium", timeStyle: "short" })}.`
          : "Not checked yet. Fill in the fields above, then press Check now.",
      ),
    ),
    h(
      "button",
      {
        class: btn.secondary,
        disabled: running.has(source),
        onclick: async (e: Event) => {
          const b = e.currentTarget as HTMLButtonElement;
          pressed = source;
          running.add(source);
          b.disabled = true;
          b.textContent = "Checking";
          await invoke("check_now", { source });
        },
      },
      running.has(source) ? "Checking" : "Check now",
    ),
  );
}

const inputCls =
  "w-full rounded-lg border border-field bg-card px-3 py-1.5 text-sm transition-colors hover:border-ink-2 focus:border-accent focus:outline-none";

function fieldShell(id: string, label: string, control: HTMLElement, help: HTMLElement) {
  return h(
    "div",
    { class: "grid grid-cols-[11rem_1fr] items-start gap-x-4 gap-y-1" },
    h("label", { for: id, class: "pt-1.5 text-sm" }, label),
    h("div", { class: "flex flex-col gap-1" }, control, help),
  );
}

// The Google calendars you track: saved ones greyed out with a trash can, and new fields added
// with + and taken away with -. Save stores the new ones. Only labels come back, never an address.
async function calendarsField() {
  const id = "f-gcal";
  const help = h("p", { id: `${id}-help`, class: "min-h-4 text-xs text-ink-2", "aria-live": "polite" }, "In Google Calendar: Settings, pick a calendar, then Secret address in iCal format.");
  const saved = h("ul", { class: "flex flex-col gap-1.5", "aria-label": "Calendars you track" });
  const fresh = h("div", { class: "flex flex-col gap-1.5" });
  const save = h("button", { class: btn.primary }, "Save");

  const showSaved = (labels: string[]) =>
    saved.replaceChildren(
      ...labels.map((label, index) =>
        h(
          "li",
          { class: "flex items-center gap-2 rounded-lg border border-line bg-panel px-3 py-1.5 text-sm text-ink-2" },
          h("span", { class: "min-w-0 flex-1 break-all" }, label),
          h(
            "button",
            {
              class: `${btn.icon} hover:text-danger`,
              title: "Stop tracking this calendar",
              "aria-label": `Stop tracking ${label}`,
              onclick: async () => {
                try {
                  const left = await invoke<string[]>("gcal_remove", { index });
                  showSaved(left);
                  // With none left nothing is checked, so the copies stay until deleted in the Calendar view.
                  say(help, left.length ? `Stopped tracking ${label}. Its copies leave the shared calendar at the next check.` : `Stopped tracking ${label}. Its copies stay in the shared calendar; delete them there if you don't want them.`);
                } catch (e) {
                  say(help, String(e), true);
                }
              },
            },
            icon("trash"),
          ),
        ),
      ),
    );

  const addRow = (focus = true) => {
    const input = h("input", { type: "password", autocomplete: "off", spellcheck: false, class: inputCls, placeholder: "Secret address in iCal format", "aria-label": "Calendar address" });
    const row = h(
      "div",
      { class: "flex items-center gap-2" },
      input,
      h("button", { class: btn.icon, title: "Remove this field", "aria-label": "Remove this field", onclick: () => row.remove() }, icon("minus")),
    );
    fresh.append(row);
    if (focus) input.focus();
  };

  save.onclick = async () => {
    const urls = [...fresh.querySelectorAll("input")].map((i) => i.value).filter((v) => v.trim());
    if (!urls.length) return say(help, "Paste an address first, or add a field with +.", true);
    try {
      showSaved(await invoke<string[]>("gcal_add", { urls }));
      fresh.replaceChildren();
      say(help, `Saved in ${view.store}. Press Check now to copy the events.`);
    } catch (e) {
      say(help, String(e), true);
    }
  };

  const labels = await invoke<string[]>("gcal_feeds");
  showSaved(labels);
  if (!labels.length) addRow(false);
  const control = h(
    "div",
    { id, class: "flex flex-col gap-2" },
    saved,
    fresh,
    h("div", { class: "flex gap-2" }, h("button", { class: btn.secondary, onclick: () => addRow() }, icon("plus"), "Add calendar"), save),
  );
  return fieldShell(id, "Calendars", control, help);
}

function say(help: HTMLElement, text: string, danger = false) {
  help.className = `min-h-4 text-xs ${danger ? "text-danger" : "text-ink-2"}`;
  help.textContent = text;
}

function plainField(key: string, label: string, attrs: Record<string, unknown> = {}) {
  const id = `f-${key}`;
  const help = h("p", { id: `${id}-help`, class: "min-h-4 text-xs text-ink-2", "aria-live": "polite" });
  const input = h("input", {
    id,
    class: inputCls,
    value: view.plain[key] ?? "",
    "aria-describedby": `${id}-help`,
    ...attrs,
    onchange: async () => {
      try {
        input.value = await invoke<string>("save_setting", { key, value: input.value });
        say(help, "Saved.");
      } catch (e) {
        say(help, String(e), true);
      }
    },
  });
  return fieldShell(id, label, input, help);
}

type Account = { status: "not_set_up" | "signed_out" | "needs_name" | "ready"; me: string | null; email: string | null; members: { id: string; name: string }[] };

// Signing in to the shared calendar, your name, your password, and signing out. The password goes
// straight to Supabase; only the sign-in it gives back is kept, in the system's password store.
async function accountField() {
  const id = "f-calendar-account";
  const help = h("p", { id: `${id}-help`, class: "min-h-4 text-xs text-ink-2", "aria-live": "polite" });
  const slot = h("div", { class: "flex flex-col gap-2" });

  // Runs one action from a button, and shows the account again when it worked or the reason when not.
  const run = async (b: HTMLButtonElement, busy: string, work: () => Promise<unknown>, done?: string) => {
    const label = b.textContent;
    b.disabled = true;
    b.textContent = busy;
    try {
      await work();
      await show(true);
      if (done) say(help, done);
    } catch (e) {
      say(help, String(e), true);
      b.disabled = false;
      b.textContent = label;
    }
  };
  const form = (onsubmit: () => void, ...children: HTMLElement[]) =>
    h("form", { class: "flex flex-wrap gap-2", onsubmit: (e: Event) => (e.preventDefault(), onsubmit()) }, ...children);
  const signOut = () => {
    const b = h("button", { type: "button", class: btn.danger }, "Sign out");
    b.onclick = () => run(b, "Signing out", () => invoke("calendar_sign_out"), "Signed out. The calendar's copy on this computer is removed.");
    return b;
  };

  const show = async (focus = false) => {
    const a = await invoke<Account>("calendar_state");
    say(help, "");
    if (a.status === "not_set_up" || a.status === "signed_out") {
      const email = h("input", { id, type: "email", class: inputCls, placeholder: "Email", autocomplete: "username", "aria-describedby": `${id}-help` });
      const password = h("input", { type: "password", class: inputCls, placeholder: "Password", autocomplete: "current-password", "aria-label": "Password" });
      const go = h("button", { type: "submit", class: btn.primary }, "Sign in");
      slot.replaceChildren(form(() => run(go, "Signing in", () => invoke("calendar_sign_in", { email: email.value, password: password.value })), email, password, go));
      say(help, a.status === "not_set_up" ? "Add the Project URL and the publishable key above first." : "Use your email and the password you got from the calendar owner.");
      if (focus) email.focus();
    } else if (a.status === "needs_name") {
      const name = h("input", { id, class: `${inputCls} flex-1`, placeholder: "Your name", maxlength: 40, "aria-describedby": `${id}-help` });
      const go = h("button", { type: "submit", class: btn.primary }, "Save");
      slot.replaceChildren(
        h("p", { class: "text-sm" }, `Signed in as ${a.email}.`),
        form(() => run(go, "Saving", () => invoke("calendar_set_name", { name: name.value })), name, go),
        h("div", {}, signOut()),
      );
      say(help, "Type your name the way the others should see it.");
      if (focus) name.focus();
    } else {
      const me = a.members.find((m) => m.id === a.me)?.name ?? a.email;
      const change = h("button", { type: "button", class: btn.secondary, onclick: () => changePassword() }, "Change password");
      slot.replaceChildren(h("p", { id, class: "pt-1.5 text-sm", tabindex: -1 }, `Signed in as ${me} (${a.email}).`), h("div", { class: "flex gap-2" }, change, signOut()));
      if (focus) change.focus();
    }
  };

  const changePassword = () => {
    const password = h("input", {
      id,
      type: "password",
      class: `${inputCls} flex-1`,
      placeholder: "New password, at least 8 characters",
      autocomplete: "new-password",
      "aria-describedby": `${id}-help`,
    });
    const go = h("button", { type: "submit", class: btn.primary }, "Save");
    slot.replaceChildren(
      form(
        () => run(go, "Saving", () => invoke("calendar_change_password", { password: password.value }), "Password changed."),
        password,
        go,
        h("button", { type: "button", class: btn.secondary, onclick: () => show(true) }, "Cancel"),
      ),
    );
    password.focus();
  };

  await show();
  return fieldShell(id, "Your account", slot, help);
}

function secretField(name: string, label: string, hint: string) {
  const id = `f-${name}`;
  const help = h("p", { id: `${id}-help`, class: "min-h-4 text-xs text-ink-2", "aria-live": "polite" }, hint);
  const slot = h("div", { class: "flex items-center gap-2" });

  const showSet = (focus = false) => {
    slot.replaceChildren(
      h("span", { id, class: "flex-1 text-sm", tabindex: -1 }, `Saved in ${view.store}`),
      h("button", { class: btn.secondary, "data-replace": "", onclick: () => showEdit(true) }, "Replace"),
      h(
        "button",
        {
          class: btn.danger,
          onclick: async () => {
            try {
              await invoke("clear_secret", { name });
              view.secrets[name] = false;
              showEdit(false, true);
              say(help, "Removed.");
            } catch (e) {
              say(help, String(e), true);
            }
          },
        },
        "Remove",
      ),
    );
    if (focus) slot.querySelector<HTMLElement>("[data-replace]")?.focus();
  };

  const showEdit = (replacing: boolean, focus = replacing) => {
    const input = h("input", {
      id,
      type: "password",
      autocomplete: "off",
      spellcheck: false,
      class: inputCls,
      placeholder: replacing ? "New value" : "Not set",
      "aria-describedby": `${id}-help`,
    });
    const save = async () => {
      try {
        await invoke("set_secret", { name, value: input.value });
        view.secrets[name] = true;
        showSet(true);
        say(help, "Saved.");
      } catch (e) {
        say(help, String(e), true);
      }
    };
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") save();
      if (e.key === "Escape" && replacing) {
        say(help, hint);
        showSet(true);
      }
    });
    slot.replaceChildren(input, h("button", { class: btn.primary, onclick: save }, "Save"));
    if (replacing) slot.append(h("button", { class: btn.secondary, onclick: () => showSet(true) }, "Cancel"));
    if (focus) input.focus();
  };

  if (view.secrets[name]) showSet();
  else showEdit(false);
  return fieldShell(id, label, slot, help);
}
