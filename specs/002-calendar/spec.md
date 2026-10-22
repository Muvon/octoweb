# 002 — Calendar

Status: draft · 2026-10-22

## Why

Calendars fail people in a few, well-documented ways: they **miss meetings or
join late** because the reminder was easy to dismiss and the video link was
buried in notes; they **juggle several accounts** in several apps; they
**go back and forth over email to find a time**; and they **leave what they are
doing** to check what is next. A browser is where meetings are joined (Meet,
Zoom and Teams links all open in it) and where the work around them happens.

Octoweb Calendar is **the next meeting, one key from joining, and today's
plan beside the page**. It is not a full calendar app: it does the daily
things well and leaves rarely used ones (room booking, booking pages, task
time-blocking, month grids) to the provider's web calendar, one click away.

Reference: MeetingBar and Itsycal for the next meeting and one-key join;
Notion Calendar (Cron) and Fantastical for natural-language quick add and
shared availability; Arc's calendar preview for meetings in the browser.

## Principles

1. **Never miss a meeting.** The next meeting is always visible with a Join
   button, an alert comes before it starts, and ⌘J joins it from anywhere.
2. **Your accounts, already set up.** Calendars come from the mail accounts
   in Settings → Mail; there is no second account list.
3. **The provider is the truth.** Edits, answers and invitations go to Google,
   Microsoft or the CalDAV server, so phone and web agree. Locally there is
   only a cache.
4. **Keyboard first, plain English for input.** "Lunch with anna@x.com
   tomorrow 1pm" is an event.
5. **The AI drafts, you decide.** Agents read the calendar and can open a
   filled-in event; only the user saves it or sends invitations.
6. **Invitation text is untrusted.** Event notes reach agents inside the
   untrusted wrapper.

## Scope

**In:** Google Calendar (Gmail, Workspace), Outlook / Microsoft 365
calendars, iCloud and any CalDAV server · unified, colour-coded calendars per
workspace · next-meeting card, address-bar chip and ⌘J join · meeting alerts
· list, day and week views · event details with guests and RSVP · create,
edit and delete, with guests, video link and simple repeats · quick add in
plain English · copy free times · meeting notes into Notes · MCP tools ·
ICS subscriptions (`webcal://`) · `.ics` import · invitations in mail.

**Out:** month and year grids, drag to move or resize, booking pages, room
booking, task time-blocking, AI auto-scheduling, secondary time zones (v2),
editing repeating CalDAV events, answering CalDAV invitations (done in mail).

## Accounts

| Provider | Calendar API | Sign-in |
|---|---|---|
| Gmail, Google Workspace | Google Calendar API v3 | OAuth, calendar scope, separate from mail |
| Outlook / Microsoft 365 | Microsoft Graph | OAuth, `Calendars.ReadWrite`, separate from mail |
| iCloud | CalDAV, `caldav.icloud.com` | The account's app-specific password |
| Namecheap Private Email | CalDAV, `dav.privateemail.com` | The mailbox password |
| Any other | CalDAV, found via `/.well-known/caldav`, or a URL entered in Settings | The account's password |

- **ACC-1** Each mail account shall have a **Calendar** switch in Settings →
  Mail, on by default; its calendars follow the account's workspace scope.
- **ACC-2** Google and Microsoft calendar access shall be granted with its own
  OAuth sign-in ("Connect calendar"), whose refresh token is kept in the
  Keychain beside the mail password. Mail's sign-in and scopes do not change.
- **ACC-3** Until it is connected, the pane shall show a **Connect** banner
  for the account; an expired grant shall ask to connect again.
- **ACC-4** A Google project without the Calendar API turned on shall be
  reported with the link to turn it on.
- **ACC-5** CalDAV accounts shall find their calendars on their own; when they
  cannot, the account shows as off with how to set the server, and no error
  banner nags.
- **ACC-6** Removing an account shall forget its calendar token and cache.

## Pane

- **PANE-1** Calendar shall be the third tab of the right-hand pane, after
  Notes and Mail, sharing its width, resize and fullscreen. ⌘⇧D (remappable
  as **Toggle calendar**) opens, switches to or hides it; Tab and ⇧Tab move
  between the pane's tabs.
- **PANE-2** Docked, the pane shows the list view by default; fullscreen, the
  week view. The user's choice of List, Day or Week holds for the session.
- **PANE-3** Calendars shall be shown or hidden per calendar from the pane's
  Calendars menu; hidden calendars do not alert.

## Next meeting

- **NXT-1** The top of the pane shall show the next meeting — one under way,
  or the next to start within 12 hours — with its countdown and a **Join
  <service>** button when it has a video link.
- **NXT-2** All-day events, declined meetings and out-of-office blocks are
  not meetings.
- **NXT-3** From 30 minutes before a meeting until it ends, the address bar
  shall show it with its countdown; a click joins (or opens it without a
  link).
- **NXT-4** **Join meeting** (⌘J) shall open the video link of the meeting
  under way or starting within 15 minutes. With nothing to join, ⌘J is the
  focused page's key as before.
- **NXT-5** Video links shall be found in conference data first (Meet,
  Teams), then the URL, location and notes; Outlook SafeLinks and Google
  redirects are unwrapped; of several links to the same service the longest
  wins (it keeps Zoom's password). Recognised: Zoom, Meet, Teams, Webex,
  Slack huddles, Around, Whereby, Jitsi, GoTo, Chime, FaceTime, Tuple,
  Gather, RingCentral, Discord.

## Alerts

- **ALR-1** Before each meeting, by the lead time in Settings → Mail →
  Calendar (off, at start, 1, 2, 5 or 10 minutes; default 1), the app shall
  show an alert with **Join** that stays a minute, with a short sound unless
  sounds are off.
- **ALR-2** When Octoweb is not the active app, the alert shall also bounce
  the Dock icon and post a macOS notification.
- **ALR-3** Each occurrence alerts once.

## Views and events

- **VIEW-1** The list shows two weeks from the chosen day, grouped by day,
  with all-day events first, a now line on today, past events dimmed, declined
  ones struck through, and "Invited" on unanswered invitations.
- **VIEW-2** Day and week grids lay overlapping events side by side, with
  all-day events in a row that stays on top; clicking an empty slot starts an
  event there.
- **VIEW-3** A week strip shows the days with events; ←/→ move by day (by
  week in the week view), t returns to today.
- **EVT-1** An event's details show when, its calendar, Join, place, guests
  with their answers, and notes with links; **Yes / Maybe / No** answer an
  invitation (Google, Microsoft).
- **EVT-2** The user shall create events with title, all-day or times,
  repeat (none, daily, weekdays, weekly, monthly, yearly), calendar, guests,
  a Meet or Teams link, place and notes. Invitations go to guests when saved.
- **EVT-3** Events the user organises shall be editable and deletable;
  deleting a repeating event asks: only this one, or all. Edits of an
  occurrence change that occurrence.
- **EVT-4** Meeting notes shall add a heading with the time, title and
  guests to the workspace notes under the meeting's day, and show Notes.

## Quick add

- **QCK-1** One line of plain English shall become an event: dates (today,
  tomorrow, weekday names, next Monday, Oct 24, 24/10, 2026-10-24, in 3
  days), times (3pm, 15:00, at 3, noon, 3-4pm, from 2 to 4), lengths (for
  45m), all day, ranges of days, repeats (every weekday, every Tuesday),
  guests (email addresses), a place (`@Place`) and `+meet` for a video link.
- **QCK-2** A preview under the field shall say what will be created; Enter
  creates it in the last-used calendar, ⌘Enter opens the full form.
- **QCK-3** A time already past today, with no day given, means tomorrow;
  without a time the event is all-day.

## Subscriptions, files and invitations

The interchange every calendar app supports: iCalendar (`.ics`) files,
`webcal://` feeds and invitations sent by mail.

- **SUB-1** The user shall subscribe to an ICS feed (`webcal://`, `webcals://`
  or `https://`) from the Calendars menu; it shows as a read-only calendar of
  the workspace it was added in, refreshed every 30 minutes, named by the
  feed (`X-WR-CALNAME`) unless renamed.
- **SUB-2** Settings → Mail → Calendar shall list subscriptions to rename,
  scope to workspaces or remove; the Calendars menu can unsubscribe too.
- **SUB-3** Following a `webcal://` link in a page shall open the Calendars
  menu with the link filled in, rather than handing it to another app.
- **IMP-1** The user shall import an `.ics` file by choosing it or dropping it
  on the pane; a sheet shows how many events and the first few, and the
  calendar to add them to.
- **IMP-2** Import shall keep each event's UID, time zone and repeat rule,
  invite nobody, and skip events the calendar already has, so importing
  twice adds nothing. Google uses `events.import`; Outlook gets each event
  with its repeat pattern when Graph can express it; CalDAV stores the
  objects as they are.
- **INV-1** A mail message carrying an invitation (`text/calendar`, or an
  `.ics` attachment) shall show a card with the event's title, time, place
  and organizer instead of an attachment chip.
- **INV-2** When the workspace's calendar already holds the event (same UID,
  as Google and Outlook add invitations themselves), the card shall answer it
  with Yes / Maybe / No — the whole series for a repeating one — and open it
  in the Calendar tab; otherwise it offers **Add to calendar**, which imports
  it into the default calendar.
- **INV-3** A cancellation (`METHOD:CANCEL`) shows as cancelled; a video link
  offers Join.

## Availability

- **AVL-1** **Copy my free times** shall put the next five working days'
  free stretches of at least 30 minutes, within working hours (Settings →
  Mail → Calendar, default 9:00–18:00), on the clipboard as text with the
  time zone, ready to paste into a message.

## Agents

- **AGT-1** MCP shall expose `calendar_list_events`, `calendar_get_event`,
  `calendar_find_free_time` and `calendar_draft_event` to agents of the
  workspace whose calendars they read.
- **AGT-2** `calendar_draft_event` shall only open a filled-in form in the
  pane; nothing is created and no invitation sent until the user saves.
  There shall be no MCP tool that creates, changes, deletes or answers.
- **AGT-3** Event notes reach agents inside the untrusted wrapper.

## Sync and storage

- **SYN-1** Each account syncs two weeks back and two months ahead every 5
  minutes, when the pane opens or Sync is pressed, and after every change;
  browsing further syncs that range too.
- **SYN-2** The cache is a JSON file under the config dir, so the pane and
  alerts work at once on launch and offline; a failed sync keeps it.
- **SYN-3** Writes go to the provider first; a deleted event leaves the view
  at once and the next sync confirms everything.

## Design notes

- **Engine.** `calendar.rs`: one thread with its own runtime, a task per
  account, the store shared with the main thread for reads. Providers in
  `calendar_google.rs`, `calendar_graph.rs`, `calendar_dav.rs`; HTTP through
  `/usr/bin/curl` with the whole request on stdin (`http.rs`), as mail OAuth
  does, so no token appears in process arguments.
- **Recurrence.** Google and Graph expand series server-side. CalDAV objects
  are expanded here (`RRULE` with `BYDAY`, `BYMONTHDAY`, `BYMONTH`,
  `BYSETPOS`, `COUNT`, `UNTIL`, `EXDATE`, `RDATE`, overrides by
  `RECURRENCE-ID`), with time zones through the system's tz database
  (`NSTimeZone`), so behaviour does not depend on server `expand` support.
- **Why not EventKit.** It cannot add guests or answer invitations, does not
  expose Meet/Teams conference data, depends on macOS account sync, and
  Exchange through it is being retired with EWS (2026–27).
- **Verified.** Unit tests for each provider's parsing and writing, the
  RRULE engine, quick add and link detection; the CalDAV path end to end
  against Radicale (`dav_round_trip_against_a_server`). Real Google,
  Microsoft and iCloud accounts need a manual check.
