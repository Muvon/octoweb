# 003 — Tasks

Status: draft · 2026-10-24

## Why

Tasks already land in the workspace notes: **To notes** on a conversation
(mail TRI-8), **Extract tasks** (mail AI-5), the capture chord on a page and
meeting notes (calendar EVT-4) all write `- [ ]` lines. In one long file they
end up buried under day headings; nothing says what is due today, and the
calendar does not know about them.

A separate to-do store would split the truth in two, and syncing with Google
Tasks, Microsoft To Do or Reminders would add three providers for a list whose
team work already lives elsewhere (Asana, GitHub Projects). So tasks stay
lines in the notes, and the Notes tab shows the open ones of the whole file
above its text, by when they are due.

Reference: the Obsidian Tasks plugin (task lines in notes gathered by a query,
due dates as `📅 YYYY-MM-DD`); the org-mode agenda; Things' Today and Upcoming;
Vivaldi and Outlook's My Day for dated tasks beside the calendar.

## Principles

1. **What people already use, done better.** Markdown checkboxes and Obsidian
   Tasks' due date, so the notes open in Obsidian and its queries read them.
   The better part: dates typed in plain English, tasks that link back to
   their mail, due tasks in the calendar.
2. **The notes file is the truth.** No second store. Any editor can add, tick
   or delete a task; the view follows the file.
3. **One date.** A task has at most one date, the day it is due; after it, it
   is overdue. No start or scheduled dates, priorities or repeats.
4. **Nothing hides.** Tasks due today or overdue show as a count on the Notes
   tab, as unread mail does on the Mail tab.

## Scope

**In:** open tasks above the notes text in the Notes tab · due dates typed in
plain English · quick add of a task · count on the Notes tab · due tasks in the
calendar · MCP tools to list and add tasks.

**Out:** a separate task store · sync with Google Tasks, Microsoft To Do,
Reminders or CalDAV `VTODO` · priorities, repeats, start dates, reminders ·
subtasks as a tree · one list across workspaces (v2) · done dates.

## Task lines

- **TSK-1** A task shall be a line `- [ ] text` or `* [ ] text` at any indent,
  as Notes already highlights them; `[x]` or `[X]` is done.
- **TSK-2** A task's due date shall be the first `📅 YYYY-MM-DD` in its line,
  as Obsidian Tasks writes it. Other Obsidian Tasks fields (`⏳`, `🔁`, `✅`,
  priorities) are kept and shown as text.
- **TSK-3** In a task line, `>` and a date — `>today`, `>tomorrow`, `>fri`,
  `>next mon`, `>oct 28`, `>28/10`, `>2026-10-28`, `>in 3 days` — shall be
  replaced by `📅` and the date it names, as `YYYY-MM-DD`, when the caret
  leaves the line; words after the date stay as the task's text
  (`- [ ] Pay >fri rent` becomes `- [ ] Pay rent 📅 …`). Dates are read as
  quick add reads them (calendar QCK-1); words after `>` that do not start
  with a date stay as typed.
- **TSK-4** A task's day is the `## YYYY-MM-DD` heading above it: the day it
  was captured.

## Tasks in Notes

One flow, not a second view: the Notes tab shows its open tasks above the
text, in the same scroll, as an Obsidian note can open with a Tasks query.

- **VIEW-1** Above the text the Notes tab shall show a **Tasks** block: a head
  line, an add field and the open tasks. The head line folds and unfolds the
  block, says what it holds ("1 overdue · 2 today · 3 without a date"), and
  stays folded or open as the user left it. **Tasks** (⌥⌘T, remappable)
  opens the pane with the keyboard in the add field, unfolding the block, or
  hides the pane when the keyboard is already in it; **Toggle notes** (⌘⇧J)
  puts the keyboard in the text, or hides the pane when it is already there.
- **VIEW-2** The block shall list the open tasks in groups: **Overdue**,
  **Today** and **Upcoming** (each with its day), by date, then file order;
  tasks without a date sit behind one line ("3 tasks without a date") that
  shows them as **No date**, in file order. Empty groups are not shown. Done
  tasks show only in the text.
- **VIEW-3** A row shall show the box, the task's text with links as their
  label (a mail task's in its account's colour, mail TRI-9), and its due
  date as Today, Tomorrow, a weekday within the week ahead, or a date;
  overdue in red.
- **VIEW-4** Clicking the box, or x on the focused row, shall tick the task in
  the text as ⌘↩ does there; ticking a mail task closes its conversation too
  (mail TRI-10). A ticked row stays, struck through, until the pane hides, so
  a second click unticks it.
- **VIEW-5** Clicking a task's text, or ↩ on the focused row, shall put the
  caret on its line in the text below and scroll to it.
- **VIEW-6** ↓ from the add field enters the list; ↑/↓ and j/k move between
  rows, ↑ from the first returns to the field, n goes to the field; Tab and
  ⇧Tab move through the pane's tabs; Esc clears the field, else hides the
  pane, as in the text.
- **VIEW-7** Enter in the add field shall add the task under today's heading,
  as captures do: "Pay rent >fri", ">fri pay rent" and "Pay >fri rent" all
  add `- [ ] Pay rent 📅 2026-10-30`. A date with no task text adds nothing
  and marks the field.

## Count

- **CNT-1** The Notes tab, in every tab of the pane, shall show the number of
  open tasks due today or overdue, with the badge Mail uses for unread; with
  none it shows no number.
- **CNT-2** The count shall follow every save of the notes, a workspace
  switch and the change of day.

## Calendar

- **CAL-1** The calendar's list, day and week views shall show the open tasks
  due each day, with a box, in the all-day row. Ticking one ticks it in the
  notes; clicking its text shows Notes with that task focused in its list.
- **CAL-2** Overdue tasks shall show on today in the list view.
- **CAL-3** Tasks come from the active workspace's notes and never alert.

## Agents

- **AGT-1** MCP shall expose `tasks_list` (the workspace's tasks with text,
  due date, day and done; open ones unless asked) and `tasks_add` (text and an
  optional due date, added under today's heading). `mail_add_tasks` stays for
  tasks that link a conversation.
- **AGT-2** Task text from agents shall have its brackets removed, as
  `mail_add_tasks` does, so no task turns into a link that hides where it goes.
- **AGT-3** There shall be no MCP tool that ticks or deletes a task: ticking a
  mail task closes its conversation, and agents never archive mail.

## Design notes

- **One grammar, two readers.** `notes::tasks(text)` reads task lines (done,
  text, due, day, line) for the count, the calendar and MCP, from the file as
  saved. The block reads the panel's own buffer in JS with the pattern Notes
  already highlights with (`TASK` in `notes_html.rs`), so typing shows at
  once. Both are tested on the same lines.
- **`>` dates.** When the caret leaves a task line with `>…`, or Enter is
  pressed in the add field, the panel sends the words; Rust reads the longest
  run of leading words that is a date with `calendar_quick` and answers it and
  the words left; the panel replaces them only if the line is unchanged. One
  date grammar for quick add and tasks.
- **Who writes.** While the panel shows a workspace's notes it holds the
  newest text, perhaps unsaved, so additions from elsewhere (a capture, mail's
  To notes, `tasks_add`) go into its buffer, which saves. Otherwise the file
  takes them, as ticking from the calendar does: the pane shows one tab at a
  time, and the panel saved its buffer when it was hidden.
- **Cost.** No new WebView, file, store or dependency: a block in the notes
  panel, a function in `notes.rs`, rows in the calendar page.
- **Obsidian.** The notes directory opens as an Obsidian vault and Tasks
  queries read the `📅` dates; `octoweb-mail://` links open only in octoweb.

## Phases

1. **In Notes.** Task lines with `📅` dates, `>` dates, the task block and its
   field, the count.
2. **Around it.** Due tasks in the calendar; `tasks_list` and `tasks_add`.
3. **Today (later).** A pane tab merging today's meetings, tasks due,
   Important mail and returning snoozes (mail spec, open question 3), named
   Today as Things and Outlook name it. Only once Tasks is in daily use.

## Open questions

1. **Done dates.** Obsidian Tasks appends `✅ YYYY-MM-DD` when a task is
   ticked. Doing the same would allow "done today" and a weekly review, at the
   cost of changing the line on every tick.
2. **Across workspaces.** One Tasks list for every workspace, or only the
   active one as in v1?
