---
name: todo-helper
description: A personal Kanban board that fills itself from Outlook, Discord, and itslearning.
colors:
  bg: "oklch(0.975 0.004 250)"
  panel: "oklch(0.948 0.006 250)"
  card: "oklch(1 0 0)"
  line: "oklch(0.89 0.007 250)"
  field: "oklch(0.6 0.012 260)"
  ink: "oklch(0.23 0.012 260)"
  ink-2: "oklch(0.47 0.014 260)"
  accent: "oklch(0.5 0.16 262)"
  accent-ink: "oklch(0.99 0 0)"
  mark: "oklch(0.92 0.15 103)"
  mark-ink: "oklch(0.3 0.06 100)"
  danger: "oklch(0.52 0.19 27)"
  danger-ink: "oklch(0.99 0 0)"
  bg-dark: "oklch(0.185 0.008 260)"
  panel-dark: "oklch(0.225 0.009 260)"
  card-dark: "oklch(0.275 0.01 260)"
  line-dark: "oklch(0.345 0.011 260)"
  field-dark: "oklch(0.56 0.012 260)"
  ink-dark: "oklch(0.94 0.005 260)"
  ink-2-dark: "oklch(0.74 0.012 260)"
  accent-dark: "oklch(0.75 0.12 262)"
  accent-ink-dark: "oklch(0.2 0.03 262)"
  mark-dark: "oklch(0.82 0.14 100)"
  mark-ink-dark: "oklch(0.22 0.04 100)"
  danger-dark: "oklch(0.72 0.15 25)"
  danger-ink-dark: "oklch(0.2 0.03 25)"
typography:
  title:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "18px"
    fontWeight: 600
    lineHeight: 1.55
  body:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.375
  note:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.625
  label:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.33
rounded:
  sm: "4px"
  md: "6px"
  lg: "8px"
  xl: "12px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "20px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.accent-ink}"
    rounded: "{rounded.lg}"
    padding: "6px 12px"
  button-secondary:
    backgroundColor: "{colors.card}"
    textColor: "{colors.ink}"
    rounded: "{rounded.lg}"
    padding: "6px 12px"
  button-danger:
    textColor: "{colors.danger}"
    rounded: "{rounded.lg}"
    padding: "6px 12px"
  card:
    backgroundColor: "{colors.card}"
    textColor: "{colors.ink}"
    rounded: "{rounded.lg}"
    padding: "10px 12px"
  column:
    backgroundColor: "{colors.panel}"
    rounded: "{rounded.xl}"
  input:
    backgroundColor: "{colors.card}"
    textColor: "{colors.ink}"
    rounded: "{rounded.lg}"
    padding: "8px 12px"
  chip-due-today:
    backgroundColor: "{colors.mark}"
    textColor: "{colors.mark-ink}"
    rounded: "{rounded.sm}"
    padding: "0 4px"
  nav-item-current:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.ink}"
    rounded: "{rounded.lg}"
    padding: "8px 12px"
---

# Design System: todo-helper

## Overview

**Creative North Star: "The Highlighted Planner"**

A student's paper planner: quiet ink on cool paper, where the only loud thing on the page is the highlighter stroke across what is due today. The board stays neutral so the owner's own words carry the screen; color appears only where it changes what to do next.

Density is moderate. Seven equal columns hold cards: Inbox, four week columns of seven stacked days (each day with its date in the corner, the first day being today), Doing, and Done. A card with a due date in the next 28 days sits in its day, and overdue cards sit in today; when the window is too narrow for all four weeks, the weeks become a carousel with arrow buttons, one week per click. Cards have one or two lines of text and a single line of quiet metadata. The interface uses the Windows system face and follows the Windows light or dark setting, so it reads as a native tool, not a website. Motion exists once: a card that reaches Done gets a short highlighter swipe, so finishing something registers.

**Key Characteristics:**
- Restrained palette: cool neutrals, one ink-blue accent for actions and focus, one highlighter yellow for "today".
- Tonal layering instead of borders: background, then column panel, then white card.
- One type family at four sizes; weight, not size, carries emphasis.
- Every state has a visible form: hover lift, focus ring, drag marker, empty-state hint.

## Colors

Cool graphite neutrals with an ink-blue pen and a highlighter.

### Primary
- **Ink Blue** (`accent`): primary buttons, the focus ring, the drop marker while dragging, the Done check mark, and text selection. It marks what you can act on.

### Secondary
- **Highlighter Yellow** (`mark`, text in `mark-ink`): the "Today" label on the first day and the "Due today" chip. It marks what is due now, and nothing else.

### Tertiary
- **Correction Red** (`danger`, text on fills in `danger-ink`): overdue chips, delete actions, and error messages.

### Deadlines
- **Deadline Violet** (`deadline`, text on fills in `deadline-ink`): a card marked as a hand-in deadline gets a violet left edge, a light violet tint, and a filled label from the Monday of its deadline's week. Before that week it looks like any other card.
- **Deadline Orange** (`soon`, text on fills in `soon-ink`): the same card the day before its deadline.
- On the day itself, and after it, a deadline card uses Correction Red. Only cards marked as deadlines get these colors, so they stay rare.

### Neutral
- **Cool Paper** (`bg`): the window background and the sidebar.
- **Ruled Panel** (`panel`): column backgrounds, the current navigation item, and hover fills.
- **Card White** (`card`): cards, inputs, sheets, and dialogs.
- **Pencil Line** (`line`): dividers, the sidebar edge, secondary button outlines, and scrollbars.
- **Field Edge** (`field`): input and text area borders, dark enough (3:1 or more) to show where you can type.
- **Ink** (`ink`) and **Faded Ink** (`ink-2`): primary text, and secondary text such as counts, dates, and empty-state hints.

Each color has a `-dark` twin used when Windows is in dark mode. In dark mode the layers step lighter toward the front: background, panel, card.

### Named Rules
**The One Highlighter Rule.** Highlighter yellow marks two things only: what is due today, and, for 700ms, a card that has just reached Done. A third use dilutes the signal the owner scans for.

**The Measured Contrast Rule.** Every text and fill pair meets WCAG AA (4.5:1) in both themes, and input borders meet 3:1. The lowest text pair is the overdue chip in dark mode (4.63:1).

## Typography

**Body Font:** Segoe UI Variable Text (with Segoe UI, system-ui)

**Character:** The Windows system face, so the board feels native. Hierarchy comes from weight (600 for headings and the current view) and from the secondary ink color, not from large size jumps.

### Hierarchy
- **Title** (600, 18px, 1.55): the card title in the card sheet.
- **Body** (400, 14px, 1.375): card titles, buttons, inputs, and navigation. Column names use 600 at the same size.
- **Note** (400, 15px, 1.625): the note editor, at most 72 characters wide.
- **Label** (400, 12px): counts, due chips, metadata, empty-state hints. Counts use tabular numbers.

## Layout

A fixed 192px sidebar on the left and the active view on the right. Below 1024px the sidebar collapses to an icon rail about 53px wide, with labels kept for screen readers. The board is a four-column grid with 12px gaps and 16px outer padding; each column scrolls on its own. Cards stack with 8px gaps. Spacing steps are 4, 8, 12, 16, and 20px.

The card sheet opens at the right edge, 448px wide, and keeps the board visible behind a scrim. The Notes view is a 256px list beside the editor.

## Elevation & Depth

Depth comes mostly from tonal layers. Cards sit on panels with a soft, offset shadow, and lift on hover. Sheets, dialogs, and toasts use the raised shadow.

### Shadow Vocabulary
- **Card** (`box-shadow: 0 1px 2px oklch(0.2 0.01 260 / 0.08), 0 1px 1px oklch(0.2 0.01 260 / 0.04)`): cards at rest.
- **Raised** (`box-shadow: 0 8px 24px oklch(0.2 0.01 260 / 0.14), 0 2px 6px oklch(0.2 0.01 260 / 0.08)`): a hovered card, the card sheet, dialogs, and toasts.

### Named Rules
**The Lift Means Movable Rule.** Only things you can drag or that float above the board get the raised shadow.

## Shapes

Soft rectangles. Columns use 12px corners, cards, inputs, and buttons 8px, small icon buttons 6px, and inline chips 4px. Borders are 1px and rare; tonal steps separate most surfaces.

## Components

### Buttons
- **Shape:** gently rounded (8px).
- **Primary:** ink-blue fill with near-white text, 6px by 12px padding, 14px medium weight. Used once per view at most ("New note").
- **Secondary:** white fill with a 1px pencil-line border. Hover fills with the panel color.
- **Danger:** red text with no fill; hover adds a 12% red tint. The confirm button in a delete dialog is a solid red fill.
- **Hover / Focus:** 150ms color transitions; a 2px ink-blue focus ring offset by 2px.

### Chips
- **Due today:** highlighter fill, dark olive text, 4px corners.
- **Overdue:** 12% red tint with red text, labeled "Overdue, Fri 16 Oct".
- **Other dates:** quiet faded ink with a calendar icon ("Due tomorrow", "Due Tue 27 Oct").

### Cards / Containers
- **Corner Style:** 8px.
- **Background:** card white on the ruled panel.
- **Shadow Strategy:** the Card shadow at rest, Raised on hover.
- **Internal Padding:** 10px by 12px.
- **Done cards:** an ink-blue check before the title, and the title in faded ink.

### Inputs / Fields
- **Style:** card white, 1px field-edge border, 8px corners.
- **Focus:** the border turns ink blue.

### Navigation
- **Style:** 14px items with an icon; the current view gets the panel fill and 600 weight. Below 1024px only the icons show.

### Toast
A dark ink pill centered at the bottom: "Deleted "Read chapter 4". Undo". The Undo action also runs on Ctrl+Z. Errors use the red fill and stay for 6 seconds.

### Done Swipe (signature)
When a card arrives in Done, its background starts highlighter yellow and fades to card white over 700ms (`cubic-bezier(0.16, 1, 0.3, 1)`). Reduced-motion settings turn it off; the check mark still shows the state.

## Do's and Don'ts

### Do:
- **Do** keep highlighter yellow for "due today" and the Done swipe only.
- **Do** use `ink-2` for secondary text instead of lowering opacity.
- **Do** give every new interactive element a hover state, the ink-blue focus ring, and a keyboard path.
- **Do** write empty states that say what to do ("Drag here what you'll do today.").

### Don't:
- **Don't** add a second accent color or color-code sources; source icons stay in faded ink.
- **Don't** put borders around cards; the tonal step and the Card shadow separate them.
- **Don't** add motion other than the Done swipe and short state transitions.
- **Don't** insert outside text (emails, messages, events) with `innerHTML`.
