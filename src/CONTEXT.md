# src — the window's user interface

One job: draw the views and send every read and write to the Rust backend through Tauri commands.

## What's here
- `main.ts`: draws the side navigation and switches views (also on a `show-view` event, so a view can send you to Settings). Each visit mounts the view into a fresh container, so a view's `root.isConnected` turns false when you leave it; background refreshes check that before drawing. Failed backend calls show as an error toast.
- `board.ts`: the Kanban board. A card marked as a deadline (card sheet, or set by extraction for an assignment email) turns violet from the Monday of its week, orange the day before, and red on the day. Inbox, 4 week columns of 7 days starting today, Doing, Done. A card not in Doing or Done shows on its due date (overdue in today, later than 28 days or undated in Inbox); dropping it on a day sets that date, on Inbox clears it. Add, rename (double-click or F2), and delete (Delete key or the x button) cards. Drag a card, or press Alt+arrow keys, to move it; plain arrow keys move focus. Click a card or press Enter to open its sheet: title, due date, source, the shared calendar, and its note. Shared calendar events show in their day above the cards and open the event sheet; they can't be dragged. A card shared to the calendar pushes its title and date to its event when either changes. `openCard()` is exported for the Calendar view.
- `calendar.ts`: the Calendar view: one week (Monday first) of everyone's shared events in an hour grid with a column per shown member per day (you first; the grid scrolls sideways inside its panel, with the days on top and the hours on the left pinned), all-day events and your own cards with a date above it (a card with times, such as an itslearning lesson, sits in the hour grid as an outline while it is on that day), a legend that hides a member's events, and the Activity list (the newest 50 changes, deletes included). `openEvent()` is the event sheet: anyone can change or delete any event, and a save carries the version it opened, so someone else's newer change comes back instead of being overwritten. Events are placed by clock time, so the days the clocks change line up. Members are told apart by name, never by color.
- `tasks.ts`: giving tasks: `giveTask()` (title, member, deadline, note; the backend checks them) and `showGiven()` (tasks you gave, with who has them, the deadline, Done or Open, and who changed them). The board opens both from the buttons over Inbox, and "Give to…" in a card's sheet fills in its title and date.
- `notes.ts`: the Notes view and `noteEditor()`, the textarea that saves on every keystroke. The card sheet uses it too.
- `sessions.ts`: the Sessions view. Re-renders on the backend's `sessions-changed` event and every 30 seconds.
- `settings.ts`: the Settings view. The Shared calendar section has the Project URL, the publishable key, and your account: sign in, your name, change password, sign out. Plain fields save when they change; secrets have their own Save, Replace, and Remove, and the window only ever learns whether a secret is set.
- `dom.ts`: `h()`, the one helper that builds elements (strings become text nodes), the shared button styles `btn`, `toast()` (with an optional Undo that also runs on Ctrl+Z), and `confirmDialog()`.
- `icons.ts`: the Lucide icon paths the app uses.
- `styles.css`: Tailwind entry point and the color tokens for light and dark mode.

## Rules
- Vanilla TypeScript, no framework. Tailwind classes for styling, using the color tokens from `styles.css`. `DESIGN.md` at the root sets the visual rules.
- Text from outside the app (emails, Discord messages, calendar events) goes into the page as `textContent`, never `innerHTML`.
- The UI keeps no state of its own that must survive a restart. SQLite holds it, through `invoke`.

## Check
- `npm run build` passes (it runs `tsc` first).
