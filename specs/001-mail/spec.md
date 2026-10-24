# 001 — Mail

Status: draft · 2026-10-13

## Why

Mail is where most work starts and where most of it gets lost. Webmail in a tab
is fine for reading a message, poor for getting through an inbox, and blind to
everything else octoweb knows: your notes, your tasks, the page you are on, the
assistant beside you.

Octoweb Mail is **one inbox for every account, built for clearing it fast**,
wired into notes and the AI. It is not a full mail client: it does the few
things that decide whether mail eats your day, and does them better than
webmail.

Reference: Spark (Readdle). We take its daily-leverage core — Smart Inbox,
Unified Inbox, Gatekeeper, Done, Snooze, Pin, Mute, Reminders, Send Later — and
leave its team and calendar features out.

## Principles

1. **Triage first.** Every message is one key away from its next state: done,
   later, task, reply. Inbox zero is the default goal, not a mode.
2. **People before robots.** Mail from humans is the inbox. Newsletters and
   notifications are kept, grouped and quiet.
3. **Keyboard first, mouse friendly.** Every action has a key, and the
   command palette lists them.
4. **The AI drafts, you decide.** Nothing is sent, deleted or archived by the
   AI without an explicit user action.
5. **Your server is the truth.** Done means archived on the server, so the
   phone and webmail agree. Only what servers cannot express (snooze, pin,
   mute, reminders, send later) is kept locally.
6. **Mail content is untrusted.** It never runs script, never loads remote
   content unasked, and reaches the AI marked as untrusted input.

## Scope

**In (v1–v3):** unified inbox across accounts · Important / Other split ·
Gatekeeper for new senders · read threads · Done, Snooze, Pin, Mute, Block ·
reply, reply all, forward, compose, undo send, send later, follow-up reminders
· signatures · search · notifications for important mail · mail → notes task ·
AI summaries, drafts, daily brief, task extraction · MCP tools for agents.

**Out:** shared inboxes, team comments, assignments, shared drafts, calendar,
templates library, label/folder management, rules editor, POP3, Exchange EWS,
mobile. Labels and folders are shown read-only.

## Accounts

v1 must work with three providers; every other IMAP provider comes free
through manual setup.

| Provider | Sign-in | IMAP | SMTP | Login name |
|---|---|---|---|---|
| **Gmail** (personal) | App password; needs 2-Step Verification | `imap.gmail.com:993` SSL | `smtp.gmail.com:465` SSL | Full address |
| **iCloud Mail** | App-specific password; needs two-factor authentication | `imap.mail.me.com:993` SSL | `smtp.mail.me.com:587` STARTTLS | IMAP: the name before `@`, else the full address · SMTP: full address |
| **Namecheap Private Email** | Mailbox password | `mail.privateemail.com:993` SSL | `mail.privateemail.com:465` SSL | Full address |
| Any IMAP provider | Password | Entered by hand | Entered by hand | Entered by hand |
| Gmail, Google Workspace (OAuth) | Your own Desktop OAuth client, PKCE, XOAUTH2 | `imap.gmail.com:993` SSL | `smtp.gmail.com:465` SSL | Full address |
| **Outlook / Microsoft 365** | Your own app registration, PKCE, XOAUTH2 | `outlook.office365.com:993` SSL | `smtp.office365.com:587` STARTTLS | Full address |

Workspace (business) Gmail may have app passwords disabled by its admin, and
Microsoft requires OAuth for IMAP: both sign in with a client the user
registers (see *Gmail OAuth*). The refresh token takes the password's place in
the Keychain, and so does a client secret.

- **ACC-1** The user shall be able to add, edit and remove mail accounts in
  Settings → Mail. For Gmail, iCloud and Private Email the user shall give only
  the address and password; servers, ports and login names come from the table
  above. For other providers the user shall also give the IMAP and SMTP
  servers, ports and login name.
- **ACC-1a** When the address ends in `@gmail.com`, `@googlemail.com`,
  `@icloud.com`, `@me.com` or `@mac.com`, the provider shall be chosen
  automatically; Private Email (any domain) shall be a choice in the add form.
- **ACC-1b** For Gmail and iCloud the add form shall say an app password is
  needed and link to the provider's page that creates one.
- **ACC-1c** Where the iCloud IMAP login with the bare name fails, the system
  shall retry with the full address before reporting an error.
- **ACC-2** When an account is saved, the system shall verify the IMAP login
  and the SMTP login, and shall show the server's error verbatim if either
  fails.
- **ACC-3** The system shall store account passwords only in the macOS
  Keychain, never in config files or logs.
- **ACC-4** Each account shall be scoped to workspaces the way extensions are:
  either **all workspaces**, or a chosen list. A new account shall start scoped
  to the workspace it was added in; Settings → Mail shall change the scope
  with the same workspace picker the Extensions tab uses.
- **ACC-4a** A workspace's inbox, search, notifications, notes links and MCP
  tools shall see only the accounts scoped to it. There is no cross-workspace
  inbox: an account wanted everywhere is scoped to all workspaces.
- **ACC-4b** Every account shall keep syncing while its workspaces are not on
  screen, so switching workspace shows an up-to-date inbox at once.
- **ACC-4c** When a workspace is deleted, it shall drop out of every account's
  scope; an account left with no workspace shall stay in Settings, marked
  "not in any workspace", and stop notifying until it is given one.
- **ACC-5** Each account shall have a colour, shown as a dot on every message
  from it, and a signature appended to messages sent from it.

## Inbox

Mail and notes share one right-hand pane: **Notes** and **Mail** are its two
tabs. Mail is where things arrive; notes are what you have committed to. They
are joined by links, not merged into one list.

- **PANE-1** The right-hand pane shall show either Notes or Mail, never both;
  width, resize and fullscreen (⌘⇧Return) shall be shared by the two.
- **PANE-2** The notes chord (⌘⇧J) and the inbox chord (⌘⇧I) shall each open
  the pane on their tab, switch to their tab when the pane shows the other
  one, and hide the pane when it already shows theirs.
- **PANE-3** The pane header shall show both tabs, with the unread Important
  count on Mail; clicking a tab shall switch to it.
- **PANE-4** Mail shall show the list of conversations, opening into the
  thread in place; in fullscreen, list and thread shall sit side by side.

- **INB-1** The inbox shall list conversations (threads), newest activity
  first, from every account of the workspace merged into one list.
- **INB-2** The inbox shall have two sections: **Important** — mail from
  people — and **Other** — newsletters, notifications and other bulk mail,
  grouped by sender.
- **INB-3** A message shall go to Other when it carries `List-Unsubscribe`,
  `List-Id`, `Precedence: bulk|list`, `Auto-Submitted` other than `no`, or comes
  from a no-reply address; otherwise it shall go to Important.
- **INB-4** The user shall be able to move a sender to Important or Other with
  one action, and every later message from that sender shall follow.
- **INB-5** Pinned conversations shall stay at the top of Important until
  unpinned.
- **INB-6** Each row shall show sender, subject, a one-line preview (or the AI
  summary when available, see AI-1), time, account dot, and markers for
  unread, attachment, pinned and reminder.
- **INB-7** When the inbox is empty, the system shall say so plainly and show
  when the next snoozed conversation returns.

### Gatekeeper

- **GK-1** Where Gatekeeper is on for an account, mail from a sender the user
  has never received from or written to shall wait in a **New senders** strip
  at the top of the inbox instead of the list.
- **GK-2** The user shall be able to accept a new sender (their mail joins the
  inbox, now and later) or block them (their mail is archived on arrival, now
  and later).
- **GK-3** Gatekeeper shall be off by default and togglable per account.

## Triage

| Key | Action |
|---|---|
| j / k | Next / previous conversation |
| ↩ / Esc | Open thread / back to list |
| e | **Done** — archive on the server |
| h | **Snooze** — pick a time; returns to the top of the inbox then |
| p | Pin / unpin |
| m | Mute — later replies skip the inbox |
| b | Block sender |
| t | **To notes** — a `- [ ]` task linking the conversation |
| r / a / f | Reply / reply all / forward |
| c | Compose |
| u | Mark unread |
| / | Search |
| x then a verb | Select several, then act on all |

- **TRI-1** Every triage action shall apply to the focused conversation, or to
  all selected conversations, and shall be undoable with ⌘Z until the next
  action.
- **TRI-2** Done shall archive on the server: on Gmail, remove the `Inbox`
  label; elsewhere, move to the folder with the `\Archive` special use, or to
  the archive folder chosen in the account settings.
- **TRI-3** After any action that removes a conversation from the list, the
  next conversation shall take over, so a run of `e e e` clears the inbox: in
  the open thread the next one (below, else above) opens in its place; in the
  list it takes focus. With none left, the list shows. A thread opened from
  notes returns as before. The next conversation shall show at once: the ones
  beside the open one load ahead without being marked read.
- **TRI-4** Snooze shall offer: later today, this evening, tomorrow morning,
  this weekend, next week, and a custom date; times shall be local.
- **TRI-5** A snoozed conversation shall leave the inbox until its time, then
  return to the top of Important, marked as returned; a reply in the meantime
  shall end the snooze.
- **TRI-6** A muted conversation's new messages shall be marked read and kept
  out of the inbox; they stay searchable.
- **TRI-7** A blocked sender's mail shall be archived on arrival and never
  notify.
- **TRI-8** "To notes" shall append `- [ ] [Subject — Sender](octoweb-mail://…)`
  to today's section of the workspace notes and mark the conversation Done.
- **TRI-9** In notes, a task linking a conversation shall show the sender and
  account dot; opening it shall switch the pane to Mail on that conversation,
  and Esc shall return to Notes.
- **TRI-10** Ticking such a task shall mark its conversation Done if it has
  come back to the inbox since; unticking shall return the conversation to
  the inbox. Marking the conversation Done shall not tick the task — the task
  may be more than the email.
- **TRI-11** A two-finger swipe right on a conversation in the list, or on a
  sender group (all its conversations), shall mark it Done. On the open
  thread the same swipe shall go back to the list, like its back button —
  swipe actions belong to list rows, as in Apple Mail.

## Reading

- **READ-1** A thread shall show its messages oldest first, the latest
  expanded and earlier ones collapsed to one line each, with quoted text and
  signatures folded.
- **READ-2** HTML mail shall render in an isolated view with JavaScript
  disabled. Remote images load by default; with "Block remote images" on
  (Settings → Mail), remote content is blocked and the user shall be able to
  load it for this message or always for this sender.
- **READ-3** Links in mail shall open in a new browser tab of the current
  workspace (⌘-click: in the background); the full URL shall show on hover
  before clicking. Right-clicking a link shall offer open in new tab, open in
  background tab, and copy link.
- **READ-4** Attachments shall be listed with name, size and file type;
  opening one shall preview it in the pane (PDF, images, text; ←/→ between a
  message's attachments) with a Download action that saves it to Downloads
  and reveals it, using the existing download flow.
- **READ-5** Opening a thread shall mark it read on the server. Loading it
  ahead or redrawing it shall not, so one marked unread stays unread.

## Writing

- **WRT-1** Reply, reply all and forward shall open an inline composer under
  the thread, with correct `In-Reply-To` and `References`, so the reply threads
  in every client.
- **WRT-2** The composer shall write plain text with light markdown (bold,
  italic, links, lists), sent as `multipart/alternative` text + HTML.
- **WRT-3** The From account shall default to the account that received the
  thread, and be changeable among the workspace's accounts.
- **WRT-4** Sending shall wait 10 seconds with an **Undo** before the message
  leaves; undo shall reopen the draft unchanged.
- **WRT-5** **Send later** shall schedule a message for a chosen time. Where
  octoweb is not running at that time, the message shall be sent at the next
  launch, and the user shall be told it was late.
- **WRT-6** **Remind me** shall mark a sent message to return to the top of the
  inbox after a chosen time if no reply has arrived by then.
- **WRT-7** Drafts shall save locally as the user types and survive a restart;
  an unsent draft shall be shown on its thread.
- **WRT-8** Sent mail shall be stored in the account's Sent folder, unless the
  server does so itself (Gmail).
- **WRT-9** Attachments shall be added by drag and drop or a file picker.

## Sound

- **SND-1** Only four moments shall make a sound, each short and soft:
  pressing **Send** (not Send later), marking a conversation **Done** (any
  way), an **AI draft** ready in the composer, and **new Important mail**
  while Octoweb is the active app. Nothing else — navigation, typing,
  opening mail — makes a sound.
- **SND-2** Settings → General → **Sounds** (on by default) silences them
  all, and so does macOS's "Play user interface sound effects" being off.

## Search

- **SRCH-1** Search shall cover sender, recipients, subject and body of all
  synced mail of the workspace's accounts, including Done (archived) mail,
  ranked newest first. Typing shall also search the server's archive, inbox
  and sent mail for uncached matches, without requiring Enter. Local results
  shall appear first, with progress and any server error shown in the panel.
- **SRCH-2** Search shall understand `from:`, `to:`, `has:attachment`,
  `is:unread`, `before:` and `after:`.
- **SRCH-3** The command palette's `/query` shall include mail results beside
  page text.
- **SRCH-4** Where a query is a question in plain language ("the invoice Anna
  sent in March"), the user shall be able to hand it to the AI, which turns it
  into a search (AI-6).

## Notifications

- **NTF-1** When a new Important message arrives and the mail panel of a
  workspace the account is scoped to is not on screen, the system shall show a
  toast with sender and subject; clicking it shall switch to that workspace
  (the active one first, if the account is in several) and open the
  conversation.
- **NTF-2** Other, muted, blocked and gatekept mail shall never notify.
- **NTF-3** The workspace dot shall show the unread Important count.
- **NTF-4** The new-mail toast shall stay long enough to read (2 s plus 60 ms
  a character, 6–10 s), hold while the pointer or keyboard focus is on it, and
  open its mail from the whole toast or its Open button. Mail arriving for the
  same workspace while it is up shall join it ("3 new messages", latest
  senders and subject); clicking it then opens the conversation if all of it
  landed in one, otherwise the inbox.

## AI

Jobs that read mail are one-shot and run on specialists with no tools — text
in, text out — and the app acts on their answer itself: `octoweb:mail-reader`
summarises and lists tasks, `octoweb:mail-writer` writes replies. Mail comes
from anyone, so nothing in it reaches an agent that can browse, read tabs or
touch the mailbox. Plain-language search uses the reader to translate the
request into a native mail query, then the app runs it in the mail panel.
The brief runs on the reader too: the app hands it the inbox, numbered, and
shows the groups it answers with. Mail features never use the sidebar
assistant; the user can still ask it about mail through its MCP tools. All mail
text is passed inside the untrusted-content wrapper the browser context
already uses, and a message cannot close that wrapper early.

- **AI-1** Where AI summaries are on, each Important conversation shall get a
  one-line summary, shown in the inbox row instead of the preview; summaries
  shall be generated on arrival and cached. The reader writes them for
  numbered conversations, and the app saves each one to the conversation of
  its number.
- **AI-2** Opening a thread shall offer **Summarise** — a short summary with
  what is asked of the user and any dates, streamed in under the toolbar in
  place of the inbox line. It stays with its conversation while the user
  looks elsewhere.
- **AI-3** In a reply composer, a labelled **✦ Write with AI** button beside
  Send (or **⌘J**) shall open the writer right under the text, with one line
  for what to say. The button draws the eye once as the reply opens. With
  words already in the body it reads **✦ Refine with AI** and opens on
  **Improve** (↩), **Shorter**, **Friendlier**, **More formal** and a line
  for any other change, all working on that text.
  The `octoweb:mail-writer` specialist — text in, text out, no tools —
  writes the reply in the user's voice from the thread, the text of its PDF
  and plain-text attachments, the user's past replies to that sender, and
  what the composer holds (rough notes, or nothing for the reply the
  conversation calls for). The reply shall stream into the body's place,
  lighter until kept; under it: **Keep** (↩), **Shorter**, **Friendlier**,
  **More formal**, **↻** and a line to ask for any other change; Esc
  discards or stops. Leaving the thread stops nothing: the reply keeps
  coming, and the composer is back with it when the thread opens again.
  Keep shall put the reply in the body as one edit that ⌘Z undoes.
  Nothing is ever sent by the AI.
- **AI-4** **Daily brief** — on request, or each morning where enabled — shall
  group the inbox into *Needs reply*, *Waiting on others*, *FYI* and *Can
  archive*, each item one line; the user shall be able to apply a group's
  suggested action with one key, and nothing is applied otherwise. The reader
  sorts the numbered inbox from senders, subjects, previews and who wrote
  last; the mail panel says it is sorting, then shows the groups, or why not.
- **AI-5** **Extract tasks** shall turn a thread's requests and deadlines into
  `- [ ]` lines in the workspace notes, each linking the conversation. The
  reader lists them; the app adds them and says how many.
- **AI-6** Ask AI shall translate a plain-language search with the mail reader
  and run the query in the mail panel without opening the sidebar. Progress
  and errors shall appear there. A late answer shall not replace a newer
  search or affect another workspace.
- **AI-7** MCP shall expose `mail_search`, `mail_read_thread`,
  `mail_list_inbox` and `mail_create_draft` to agents of the workspace whose
  accounts they touch; there shall be no MCP tool that sends, archives or
  deletes mail.
- **AI-8** AI features shall be off until the user turns them on in Settings →
  Mail, with a line saying mail text is sent to the configured agent.

## Sync and storage

- **SYN-1** The system shall keep INBOX and Sent of each account in sync, and
  use IMAP IDLE so new mail appears within seconds while octoweb runs.
- **SYN-2** On first sync, the system shall fetch the last 90 days of INBOX and
  Sent (headers first, then bodies), and older mail on demand when opened or
  searched on the server.
- **SYN-3** Mail shall be stored locally in a SQLite database with an FTS5
  index, one per octoweb profile, under the octoweb config dir.
- **SYN-4** Local-only state — snoozes, pins, mutes, sender decisions,
  reminders, scheduled sends, summaries, drafts — shall be kept in the same
  database and never pruned except by the user.
- **SYN-5** When an account is offline or its login fails, the panel shall
  show the account's state and reason; queued actions shall apply when it
  reconnects, in order.
- **SYN-6** Removing an account shall delete its local mail and Keychain entry
  after confirmation; nothing on the server changes.

## Design notes

- **Engine.** `async-imap` + `mail-parser` + `lettre`, TLS through
  `native-tls` (macOS Security.framework, so no second crypto stack). Chosen
  over `email-lib` for the smaller surface; verified end to end against a
  GreenMail server (`src/mail_e2e.rs`). Real Gmail, iCloud and Private Email
  accounts still need a manual check.
- **Store.** `rusqlite` with the bundled SQLite and FTS5. New dependency; the
  existing JSON stores (`page_index`, `later`) do not scale to mail.
- **Threading.** `References` / `In-Reply-To` for every provider, a reply
  synced before its parent adopting the child's thread, then the normalised
  subject within 30 days when a participant is shared. Gmail's `X-GM-THRID`
  is not used: one rule for all providers.
- **Rendering.** Mail HTML in a sandboxed iframe without `allow-scripts`, under
  a CSP that allows remote images unless the user blocks them; inline `cid:`
  images are inlined as data URIs when the message is parsed. WebKit runs
  none of the page's listeners inside such a frame, so links, the link menu,
  keys typed in a message and swipes are handled natively (`mail_patch.rs`).
  `allow-popups` only lets `target=_blank` links reach that handler.
- **Panel.** Mail is the second tab of the notes pane: two WebViews in the
  same column, sharing its width, resize and fullscreen (`pane_bounds!`), only
  one visible at a time. `mail.rs` owns sync on its own thread and talks to the
  event loop through events, as `acp.rs` does.
- **Keys.** Single-letter keys work only while the mail panel has focus.
  Toggling the pane on Mail is the remappable keymap action **Inbox**, default
  ⌘⇧I — the pair to ⌘⇧A for the assistant. DevTools moves from ⌘⇧I to ⌥⌘I,
  the Safari and Chrome chord; a user's own remap of either is kept.

## Phases

1. **Read and clear.** Accounts (app password / password), sync with IDLE,
   unified inbox, Important / Other, threads, Done, Snooze, Pin, Mute, Block,
   To notes, search, notifications. Usable daily for triage; replies still in
   webmail.
2. **Write.** Reply, reply all, forward, compose, signatures, undo send, send
   later, remind me, drafts, attachments. Gatekeeper.
3. **AI.** Summaries, Draft with AI, daily brief, extract tasks, plain-language
   search, MCP tools.
4. **OAuth.** Bring-your-own client for Gmail, Workspace and Microsoft 365
   (see *Gmail OAuth*). JMAP for Fastmail stays out: Fastmail works over IMAP,
   and JMAP was planned only if IMAP fell short.

## Gmail OAuth

Decision: **app passwords in v1; in phase 4, a client the user registers in
their own Google Cloud project; an octoweb-owned verified client only if Mail
is ever offered to many users.**

What Google requires, from its own pages:

- IMAP and SMTP over OAuth (XOAUTH2) take the full `https://mail.google.com/`
  scope. The Gmail API's narrower scopes (`gmail.modify`, `gmail.readonly`,
  `gmail.compose`, `gmail.metadata`) are all *restricted* too; only
  `gmail.send` is merely sensitive and `gmail.labels` non-sensitive. There is
  no cheap scope for reading mail.
- A public app with restricted scopes needs brand verification (2–3 business
  days), restricted-scope review (several weeks) and, if it stores or
  transmits restricted data on servers, a third-party security assessment
  (CASA) renewed every 12 months.
- Unverified apps work, behind an "unverified app" warning, for up to 100
  users; personal use ("the only user of your app or … only a few users")
  is exempt from review.
- In *Testing* status, refresh tokens for Gmail scopes expire after 7 days. In
  *Production* status they do not, verified or not.

What that means for octoweb:

1. **App passwords (v1).** No Google involvement. Works for personal Gmail with
   2-Step Verification. Workspace admins may turn app passwords off, and Google
   can retire them; that is the main risk.
2. **Bring-your-own client (phase 4).** The user creates a Desktop OAuth client
   in their own Cloud project, sets it to *Production* — not Testing — and
   pastes the client id into octoweb. They see the unverified-app warning once;
   tokens then last. It falls under the personal-use exemption, costs nothing,
   and is the way in for Workspace accounts without app passwords. Setup takes
   a few minutes and Settings must walk through it step by step.
3. **octoweb-owned verified client.** One-click "Sign in with Google" for
   everyone, but weeks of review and likely an annual paid CASA. octoweb keeps
   mail on the device, which the server-based wording of the assessment rule
   favours, but community reports say on-device apps have still been asked for
   CASA. And the AI features send mail text to the configured agent's model
   provider, which is a transfer to a third-party server and would very likely
   bring Mail under the assessment and Google's Limited Use rules. Not worth it
   for a personal tool; revisit only if Mail ships to many users.

## Open questions

1. **Background brief.** Run the daily brief with no user action (needs the AI
   on a timer, like proactive learning), or only on request?
2. **Retention.** Is 90 days of local mail right, or should the whole mailbox
   be synced for offline search?
3. **A Today view (later).** A third tab merging open tasks, Important mail,
   returning snoozes and due reminders into one "what needs me" list. Worth it
   once Notes and Mail are in daily use; not before. Planned as phase 3 of
   `specs/003-tasks`.
