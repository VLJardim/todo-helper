# Product

<!-- impeccable:product-schema 1 -->

Source: the original author's requirements (2026-10-18). Lines marked *(inferred)* come from the plan's context, not from the author's own words.

## Platform

web

A Tauri 2 desktop app on Windows. The interface is a web page in WebView2, so web conventions apply, inside a resizable desktop window (minimum 800 x 560).

## Users

One person, the owner. A student who uses itslearning for school, Outlook (school, personal, and work mail), and one Discord server, and who runs Claude Code sessions on the same computer. They open the board during the school day and in the evening to see what to do next. *(inferred: student, from itslearning and the essay examples)*

## Product Purpose

Put every todo in one place without copying it by hand. The app collects todos from Outlook, one Discord server, and the itslearning calendar, lets the owner sort them on a Kanban board, keeps notes, and shows which Claude Code sessions and agents are running. Success: the owner checks one window instead of four, and nothing due slips past.

## Positioning

A personal board that fills itself. Sources become cards automatically, through one extraction step that turns new messages into card suggestions. Everything stays on the owner's computer in one SQLite file.

## Operating Context

- Board columns: Inbox, Today, Doing, Done. Cards from sources land in Inbox; cards due today land in Today.
- Sources check on a schedule: Outlook and Discord every 5 minutes, itslearning every 30 minutes, and each catches up at the next start.
- The window follows the Windows light or dark setting.
- Used with mouse and keyboard on a laptop or desktop screen. *(inferred)*

## Capabilities and Constraints

- Cards: title, column, position, optional due date, source (`manual`, `email`, `discord`, `itslearning`), and at most one linked note.
- Notes are plain text and save on every keystroke.
- Secrets live in Windows Credential Manager, never in SQLite or the repository.
- Out of scope: phone access, sync between devices, running in the tray while the window is closed, and Claude sessions that don't run on this computer.

## Evidence on Hand

None. There are no real cards, emails, or messages in the repository. Example content in screenshots is test data; never present it as the owner's.

## Product Principles

1. The next action is obvious. Today is the column that matters, and it is the easiest to read.
2. Starting costs nothing. Adding a card is typing and pressing Enter; nothing asks for setup first.
3. Progress is visible. Finished work stays on screen in Done.
4. Quiet by default. The board shows what needs doing, not decoration. *(inferred from the owner's request for "a standard look")*
5. Nothing is lost. Typing saves immediately, and errors say what happened and how to fix it.

## Accessibility & Inclusion

- Every action works from the keyboard: add, move, open, rename, and delete a card.
- Text meets WCAG AA contrast in both light and dark mode.
- Motion stays short and never hides state; reduced-motion settings remove it.
