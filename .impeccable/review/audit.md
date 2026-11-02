# impeccable audit: board, card sheet, and notes

Date: 2026-10-18. Scope: `src/board.ts`, `src/notes.ts`, `src/dom.ts`, `src/main.ts`, `src/styles.css`.

How it ran: the checks below were done by hand in the running app (WebView2, through the Chrome DevTools Protocol), with contrast measured from the live CSS tokens in both themes. Screenshots are in this folder (not committed). The launcher waited for the owner's approval, given on 2026-10-18; the detector results are in the next section.

## Detector results (engine 0.1.5, 2026-10-18)

- **Source scan** (`impeccable detect --json src index.html`): 0 findings.
- **Rendered scan**: this machine has no Chrome or Edge for a URL scan, so each view's rendered HTML (board, notes, sessions, settings) was saved from the running app with the compiled CSS inlined, then scanned. 12 findings, all `warning`, none high severity:
  - **cramped-padding (10): false positives.** The flagged elements are the board columns (children inset 8 to 12px), the Settings sections (`p-5`, 20px), and the Settings status rows (`pt-4`, 16px). Tailwind v4 writes padding as `calc(var(--spacing) * N)`, which the static scan doesn't resolve.
  - **flat-type-hierarchy (2): intentional.** DESIGN.md sets one family at 12, 14, 15, and 18px, with weight carrying emphasis. Change DESIGN.md first if you want larger size steps.

## Audit health score

| # | Dimension | Score | Key finding |
|---|---|---|---|
| 1 | Accessibility | 3 | Every card is its own Tab stop, so Tab walks the whole board (P3) |
| 2 | Performance | 4 | -- |
| 3 | Responsive design | 3 | Drag and drop needs a mouse; touch has no way to move a card (P3) |
| 4 | Theming | 4 | -- |
| 5 | Implementation integrity | 4 | -- |
| **Total** | | **18/20** | **Excellent** |

## Implementation integrity verdict

Pass. One token set (`styles.css`) drives both themes, one element helper builds all markup with text nodes only, and one button style map (`btn` in `dom.ts`) serves every view. The highlighter color appears only on "Today" and "Due today", as DESIGN.md requires.

## Findings

No open P0 or P1 issues. The Phase 1 verifier (2026-10-18) found two P1 data-loss bugs in Undo that this audit missed; both are fixed:
- **[P1] Undo failed when a new card had taken the deleted card's id.** Fixed: Undo inserts the card with a fresh id and relinks its note, in one transaction.
- **[P1] Undo from the card sheet restored a stale copy** (no note link, no due date). Fixed: `delete_card` returns the row as it was just before deleting, and Undo restores that.
- **[P2] Note text typed at machine speed could be lost on close.** Fixed: saves coalesce to the newest text, and closing the window hides it at once and ends it 0.5 seconds later (in Rust, so a broken page can't block closing), which lets the last save land. 121 characters typed at full speed and an immediate close all survived.
- **[P2] Input borders were 1.3:1 against their background** (WCAG 1.4.11 asks for 3:1). Fixed: a `field` token; borders now measure 3.19:1 or more in both themes.
- **[P3] The pressed Delete button dropped to 4.06:1.** Fixed: pressing keeps the hover tint.
- **[P3] The Delete key left focus on the page body.** Fixed: focus moves to the next card.

Fixed during this pass:
- **[P2] Instant delete with no way back.** The Delete key removed a card at once. Fixed: an Undo toast for 8 seconds, also on Ctrl+Z, which restores the card and its note link (`restore_card`, covered by `cargo test`).
- **[P2] Keyboard moves were silent for screen readers.** Fixed: a polite live region says "Moved to Doing, position 1."
- **[P2] Titles wrapped early.** The hidden delete button reserved space. Fixed: the button floats over the card corner.
- **[P2] Cramped at the 800px minimum width.** Fixed: the sidebar collapses to an icon rail below 1024px.
- **[P3] No h1.** Fixed: the app name is the page's h1.

Open:
- **[P3] Many Tab stops.** Location: `board.ts`, `cardEl`. Every card has `tabindex="0"`. Arrow keys already move focus between cards, so a roving tabindex (one Tab stop per column) would shorten Tab paths. Suggested command: `$impeccable harden`.
- **[P3] No touch path for moving cards.** Location: `board.ts`. HTML5 drag and drop does not start from touch in WebView2. A "Move to" control in the card sheet would cover touch screens. Suggested command: `$impeccable adapt`.

## Measured contrast (WCAG AA needs 4.5:1)

| Pair | Light | Dark |
|---|---|---|
| ink on card | 16.88 | 12.39 |
| ink-2 on panel | 5.87 | 7.41 |
| accent on panel (focus ring, marker) | 5.30 | 7.67 |
| overdue chip (danger on its 12% tint) | 4.97 | 4.63 |
| field edge on card (input borders, 3:1 needed) | 3.96 | 3.19 |
| accent-ink on accent | 6.01 | 8.12 |
| mark-ink on mark | 10.89 | 9.98 |
| danger on card | 6.08 | 5.59 |
| danger-ink on danger | 5.93 | 6.90 |

## Checked in the running app

- Keyboard only, from a fresh page: Tab reaches the add field, Enter adds a card, Tab focuses it, Alt+Right moves it to Today, Enter opens its sheet.
- Dark mode renders because Windows is set to dark; light mode renders under an emulated `prefers-color-scheme: light`.
- Reduced motion turns off the Done swipe (`@media (prefers-reduced-motion: reduce)` in `styles.css`).

## Positive findings

- Empty columns explain what to do with them.
- Due dates read as words ("Due today", "Due tomorrow", "Overdue, Fri 16 Oct"), not ISO strings.
- Browser surfaces are themed: selection, caret, scrollbars, placeholder, focus ring, and the dialog backdrop.
