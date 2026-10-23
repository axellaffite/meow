# Meow

A pixel-art cat that walks along the bottom of your macOS screen and runs your
everyday requests through Claude Code.

Requests go through `claude -p` (the Claude Code CLI in print mode), so they
use your regular Claude subscription, not API billing.

## Use

- **Click the cat** or press **⌥⇧Space** to open the chat, type a request, press Enter.
- While Claude works, the cat sits with a `…` bubble. If the reply arrives while
  the chat is closed, it shows a `!` until you click it.
- When there is unread news in **Activity**, the `!` becomes a bubble with the
  icons of where it comes from (Slack, GitHub, Linear…; past three sources, one
  stack icon with their number). Clicking the cat then opens Activity.
- Answers are rendered from Markdown: headings, bold/italic, code, lists,
  tables, quotes and clickable links.
- Working steps (tool calls, sub-agents, interim messages) are folded under a
  `▸ N steps` line above each answer; click it to show them.
- **Esc** cancels the request of the conversation on screen, or closes the
  chat. Other conversations (another task, a quick action) keep theirs running:
  each conversation runs one request at a time, side by side with the others.
- **New** (or `/new`, or ⌘N) starts a fresh conversation. Otherwise follow-ups
  continue the same Claude session (`--resume`).
- **Right-click the cat** for the quick actions, Open Chat, New Conversation
  and Quit. Quick actions are also in the chat's sidebar.
- **Clicking a quick action shows its last result** (and when it ran);
  **[ RUN ]** runs it again. An action that never ran runs on the first click
  (in the background from the cat's menu; the cat shows `!` when done). Ask
  follow-up questions right under the result.
- **Drag the cat** to pick it up by the scruff: it kicks, then hangs limp and
  swings as you move it. Let go and it falls back down; drag fast to throw it
  (thrown hard at a screen edge it clings to it with its paws, then
  hops down, or now and then slides down the wall; thrown hard at the top it hangs there upside down, then drops; a
  gentle throw just bounces off). With several displays, carry or throw it
  across to another one: only the outer edges of your screens are walls.
- **Pet the cat** by stroking it: move the mouse back and forth over it (no
  click). It closes its eyes and little hearts float up.
- **Dress the cat like yours**: right-click the cat › **Theme** picks its coat
  (Ginger, Willow, Alfie, or Dédé, who is a pig). **Add from Cat Photo…** asks for a photo of your
  cat; Claude (Haiku, allowed only to read a shrunk copy of the photo) picks
  the color of its fur, stripes, blaze, muzzle, legs, tail and each eye, and
  names the theme (after the file's name when that looks like the cat's). The
  cat sits with a `…` bubble meanwhile, then wears it. Your themes are listed
  under the built-in ones; a new photo with the same name replaces the old
  theme, and **Remove** deletes the one being worn. They are saved in
  `~/Library/Application Support/Meow/settings.json`.
- **Now and then the cat coughs up a hairball** (or on demand: **Hairball** in
  its menu). It stays on the floor until you clean it: scrub it with the mouse
  button down, or click it a few times, until it sparkles and is gone.
- **Dédé the pig poops instead** (the item is called **Poop** then): he
  squats, drops it behind him, turns around to admire it, hearts floating up,
  and eats it. Wipe it before he does and he goes off disappointed.

Requests use **Haiku** (latest version) by default, the cheapest Claude model,
which is plenty for fetching from connectors and summarizing; sub-agents use it
too. Claude runs with `--permission-mode auto` in your home directory, with your
usual Claude Code settings, MCP servers and skills, plus Meow's own tools (see
[Meow's data in the chat](#meows-data-in-the-chat)).

## Look

The chat opens as the cat's pixel-art speech bubble (drag it by its background,
close it with Esc, the ✕, by clicking the cat, or just by clicking elsewhere), dark
with an 8-bit palette (Sweetie 16). Text uses three pixel fonts: VT323
for messages, Silkscreen for labels and Press Start 2P for titles, bundled in
`assets/fonts` under the SIL Open Font License.

## Build and run

Requires Rust and Claude Code (`claude` logged in).

```sh
cargo run                       # run from the terminal
scripts/bundle.sh               # build target/release/Meow.app (--universal: Apple silicon + Intel)
scripts/bundle.sh --install     # ...and copy it to ~/Applications
```

To start it at login, add `Meow.app` in System Settings › General › Login Items.

The Slack sign-in lives in the Keychain, which trusts an app by its signature.
Run `scripts/setup-dev-signing.sh` once: it creates a self-signed "Meow Dev"
identity, and every `cargo run` / `cargo test` binary (and `scripts/bundle.sh`)
is then signed with it, so Keychain access granted once survives rebuilds.

### Releases and updates

Each push to `main` runs `.github/workflows/release.yml`: it tests, builds a
universal `Meow.app` (Apple silicon and Intel) and publishes it as the GitHub
release `commit-<short hash>` and as `latest`, which is replaced every time.
Each holds `Meow.dmg` (open it and drag Meow to Applications), `Meow.zip` and
`version.json` (the commit and the zip's SHA-256). The app is signed ad hoc and
not notarized: the first time, right-click it and choose **Open**.

A release build knows its commit (right-click the cat: **Version 90220bc**).
A minute after launch, then every 6 hours (`MEOW_UPDATE_HOURS`, `0` turns it
off), it reads `latest/version.json`. On another commit it downloads the zip,
checks its SHA-256 and signature, and unpacks it beside the app; once nothing
is running and the chat is closed, it swaps the two and relaunches. To check
right away, right-click the cat › **Check for Updates** (the answer comes as a
banner), or type `/update` in the chat: Meow says whether it is up to date,
downloads a newer build if there is one (it installs the same way), or why it
cannot update (a local build, updates turned off). Builds made
locally (`cargo run`, `scripts/bundle.sh` without `MEOW_COMMIT`) never update.
The release files must be public: while the repository is private, the check
fails and is tried again 30 minutes later. As the signature changes with each
build, macOS asks again for Keychain access (the Slack sign-in) after an
update. `MEOW_UPDATE_URL` makes the app read another folder of release files,
to try an update from a local server.

To build a release locally: `MEOW_COMMIT=$(git rev-parse HEAD) scripts/bundle.sh
--universal && MEOW_COMMIT=$(git rev-parse HEAD) scripts/package.sh` (files in
`dist/`).

### Tasks

The chat window has a sidebar: **Chat**, **Tasks** (with the number of open
tasks) and your quick actions. Right-click the cat › **Tasks** (or ⌘T) opens it
on the task list, grouped by source (GitHub, Linear, Slack…). Quick actions add
the things you need to act on, click a task to open it (hovering shows it is clickable), or **[ CHAT ]** to
talk about it in its own conversation, which remembers its Claude session
across restarts. You can also type your own tasks. Re-running an action merges: new items are added and checked ones stay
checked. Checking a task only ticks it here (nothing changes in Linear, Slack…),
and done tasks leave the list for **See completed**, at the bottom, where they
show when they were done and stay for 30 days. The list is saved in
`~/Library/Application Support/Meow/tasks.json`.

### Background check

Every 5 minutes Meow quietly asks Claude (Haiku, read-only, through your
connectors) to look at your open tasks and at what is new since the last check:
tasks resolved at the source (PR merged or reviewed, Linear issue done, Slack
thread or email answered) are ticked off and marked `· auto`, and new things
waiting on you are added. The Tasks header shows when it last checked. Set
`MEOW_SYNC_MINUTES` to change the interval, or to `0` to turn it off. Each check
uses a little of your Claude plan.

### Quick actions

The defaults are **Today's planning** (agenda, priorities, what needs a reply)
and **Pending tasks** (Linear, GitHub reviews and PRs, Slack, Gmail). Both only
read; they never send or change anything. Each source is checked by its own
Claude sub-agent, in parallel, then the results are combined.

**Today's planning runs by itself every day at 7:00**, or as soon as Meow is
running after that (Mac asleep, logged in late), in the background: the cat
shows `!` when your plan is ready. It waits while you are using the chat, and a
failed run is tried again 15 minutes later. Through the day, the plan's agenda
is kept current from your calendar every 15 minutes (`MEOW_AGENDA_MINUTES`, 0
turns it off), by a quiet run that may only read the calendar. Invitations
you have not answered show **Accept** and **Decline**: each click is a run that
may only call the calendar's respond tool. Its result is a **plan page** rather
than text: the date and a one-line summary, today's agenda (each event with how long it lasts and its meeting room, past events dimmed,
the current one highlighted), the 3 things to focus on first with why, the
replies you owe, and the rest for later. Every item is a task: tick it on the
plan or in Tasks, click it to open its link. Type under the plan to ask about
your day.

To replace the actions, create `~/.config/meow/actions.json` and restart the app.
`daily_at` (24-hour time) runs an action every day; `"layout": "plan"` shows its
result as a plan page:

```json
[
  { "title": "Today's planning", "prompt": "Plan my day from my calendar, Linear and Slack.",
    "daily_at": "07:30", "layout": "plan" },
  { "title": "Inbox triage", "prompt": "Summarize unread emails that need an answer. Only read." }
]
```

Every quick action, yours included, also reports its actionable items to the
task list.

They use the connectors of your Claude account (Slack, Google Calendar, Gmail,
Linear, GitHub…). Sign in to them once with `/mcp` in Claude Code.

### Activity

The **Activity** page collects what happened while you were away: Slack DMs and
mentions, calendar changes found by the agenda refresh (new, moved or cancelled
events, new invitations, answers sent), tasks found or resolved by the
background check, and scheduled tasks that ran or fired. Each item shows the
pixel-art icon of where it comes from (Slack, Calendar, GitHub, Linear, Mail,
Notion…): one icon per source for up to three, and past that a single stack
icon with their number; hover the icons to see the sources. A Slack sender
keeps one item per conversation, with only their last message. Each new item also
shows a macOS banner (the bundled app asks permission once; a `cargo run` build
posts through `osascript`). Opening the page marks everything read; `×`
dismisses one item, **Clear all** empties the list, and items go after 7 days.
Clicking an item opens what it is about: a Slack message opens the Slack page scrolled to that message, highlighted (or to its thread when the reply itself is not loaded).

### Scheduled tasks

Ask in the chat for something later and Claude schedules it instead of doing it
now:

- **At a time**: "at 17:00, summarize my unread emails", "every day at 9:00,
  list the PRs waiting for me". It runs at that time with the chat's
  permissions.
- **When something happens**: "when Staples replies to Cyril's email, let me
  know". Claude checks every 10 minutes (or as asked), read-only, and when it
  happens tells you with the details, or does what you asked.

Claude also sees your scheduled tasks, active and finished (for 7 days), with
what they do and their last result. So you can ask "start the Staples watch
again", "move the report to 18:00" or "cancel the PR reminder" without
repeating the details.

The **Scheduled** page lists them with their next run, what they do and their
last result; `×` deletes one. Results land in Activity. Runs happen in the
background, one at a time, and never block the chat.

### Slack

The sidebar's **Slack** page (also right-click the cat › **Slack**, or ⌘L) is a
live, two-way Slack chat, separate from Claude. Two tabs list your **Direct**
and group messages and your **Channels** (the ones in your Slack sidebar, most
recent first), each with how many conversations have unread messages; open one
to read it and reply from the input. Emoji show as emoji: standard
`:shortcodes:` (with skin tones) and your workspace's custom ones, whose images
are downloaded when first seen and cached in `~/Library/Caches/Meow/emoji`. Attached
images show in the conversation too (their thumbnail, fetched with your session
from `files.slack.com` only, cached in `~/Library/Caches/Meow/slack-files`);
click one to open it in Slack. New messages arrive within a few seconds and the cat shows `!` when
someone writes to you while you are elsewhere.

**Sign-in uses no Slack app.** Click **Sign in to Slack** and log in with your
usual SSO in the window that opens. Meow then reads the same session token and
`d` cookie the Slack web app itself uses, and keeps them in the macOS Keychain —
never in a file or a log. This is how unofficial clients like wee-slack and
slackdump work.

- **This goes around your workspace's app-approval control on purpose.** Slack
  can't tell it apart from your browser, but your IT or security team may not be
  fine with it — only you can judge that.
- **It's unofficial and unsupported by Slack**, and may break if Slack changes
  its web client.
- The token is as powerful as your login; it can't be revoked on its own
  (signing out of all Slack sessions ends it). When the session ends, Meow asks
  you to sign in again.

To try it without the sign-in window (e.g. to check your workspace allows this
access), set `MEOW_SLACK_TOKEN` (`xoxc-…`), `MEOW_SLACK_COOKIE` (`xoxd-…`) and
`MEOW_SLACK_DOMAIN` (your workspace subdomain) before launching.

### Coding agents (Superset)

Meow can hand coding work to a Claude Code agent running in
[Superset](https://superset.sh), from the chat or from a Slack conversation.
Each agent gets a **new worktree** of one of your Superset projects (branch
`meow/<name>-<suffix>`), opened in the Superset app, and runs the `claude`
preset there (`--permission-mode auto`).

- **Chat:** type `/agent <what to do>`. Claude (read-only) writes the agent's
  prompt and picks the project, then the agent starts right away. The prompt
  and the steps fold under the turn, and the agent's messages show there as it
  works.
- **Slack:** in a conversation, click **Agent** (with an instruction typed in
  the reply box, or none to do what the conversation asks). Claude writes the
  prompt from the recent messages, with no tools, and shows it in the private
  panel; nothing starts until you click **Start agent**. The agent's progress
  and latest message then show in that panel, also after you reopen the
  conversation. Nothing is posted to Slack.

Meow follows the agent until its last message says it is done (or blocked), or
it has been quiet for 20 minutes; after that, see its workspace in Superset. You
can't send it follow-ups from Meow, and Meow never closes or deletes the
workspaces: they are yours to review, merge and delete in Superset. Requires the
Superset app (its `superset` CLI is found on your PATH or in `~/.superset/bin`).

### Autopilot

The sidebar's **Autopilot** page lets Meow start work by itself when a Slack
message asks you for something: "your PR #42 has conflicts", "can you look at
why the build fails", "send me the numbers for X". Turn it on with **Turn on**
on the page (it is off by default).

- **Triage:** once a DM, group message or mention has been quiet for 45
  seconds (so a request written in several messages is read whole), Claude
  (Haiku, with no tools) reads the conversation's last messages and decides
  whether there is work to prepare. Greetings, news and decisions only you can
  make are left alone, as is a conversation you already answered. Add your own
  rules in `~/.config/meow/autopilot.md`, e.g. "Only for messages from Axel".
- **Work:** one job at a time, with your usual tools and connectors, on Opus
  (`MEOW_AUTOPILOT_MODEL` picks another). Code work happens in a new git
  worktree of your clone, at the tip of the PR's branch (found with `gh`, the
  clone found under `~/Developer` and the usual folders, or `MEOW_CODE_DIRS`),
  never in your own checkout. The agent merges, edits, builds, tests and
  commits there.
- **Nothing irreversible without you:** every tool call goes through Meow's
  guard (the app's binary as a Claude Code `PreToolUse` hook, `meow --guard`).
  Reading, searching, and editing files inside the job's folder go through.
  Anything else is held: pushing, `gh pr comment/merge/…`, POST requests,
  publishing or deploying, deleting outside the folder, and every connector
  tool that does more than read (sending a Slack message or an email, saving a
  Linear issue…). The held call is recorded exactly as Claude made it, and
  Claude carries on without it.
- **Approval:** the job's card shows its summary and each held call (the
  command, or the message with where it goes) with **Approve** and **Reject**.
  Approving resumes the job's Claude session, and the guard lets through that
  one call, with that exact input, once. Results land in Activity, and the
  sidebar shows how many jobs wait for you.

`×` removes a job and its worktree; **Open folder** shows the worktree in
Finder. Jobs are saved in `~/Library/Application Support/Meow/autopilot.json`,
their worktrees under `…/Meow/autopilot/<id>/work`, and finished ones go after
7 days.

Meow used to be called Lazy: on first launch it moves data saved under the old
name (`~/Library/Application Support/Lazy`, `~/.config/lazy`) to the new folders.

### Meow's data in the chat

Claude knows it is talking to you from Meow, and can look at what Meow sees.
Chat messages, quick actions, task conversations and scheduled tasks run with
a short description of Meow added to Claude's system prompt, and with Meow
itself as an MCP server named `meow` (the app's binary, started by Claude as
`meow --mcp`). Its tools only read:

- **Slack**, through Meow's own Slack session: `slack_conversations` (DMs,
  group DMs and channels, with unread state), `slack_history`, `slack_thread`
  and `slack_search` (Slack's search syntax, e.g. `from:@ana in:#eng`). So "what
  did Ana ask me this morning?" works without a Slack connector. The session is
  read from the Keychain only when a tool needs Slack.
- **Meow**: `meow_tasks`, `meow_activity`, `meow_quick_actions` (each action's
  last result, today's plan among them) and `meow_scheduled`.

Nothing can be sent or changed through them; replies on Slack go through the
Slack page. Runs that are deliberately cut off (offline prompts, the calendar
and RSVP runs, reading a cat photo) do not get them.

### Settings (environment variables)

| Variable          | Default                       | Effect                          |
|-------------------|-------------------------------|---------------------------------|
| `MEOW_CLAUDE_BIN` | `claude` from your shell PATH | Path to the Claude Code CLI     |
| `MEOW_CWD`        | `$HOME`                       | Working directory for requests  |
| `MEOW_MODEL`      | `haiku`                       | Claude model for requests and their sub-agents (e.g. `sonnet`) |
| `MEOW_SYNC_MINUTES` | `5`                         | Minutes between background task checks (`0` = off) |
| `MEOW_SLACK_TOKEN` | *(none)*                     | Slack session token (`xoxc-…`), to skip the sign-in window |
| `MEOW_SLACK_COOKIE` | *(none)*                    | Slack `d` cookie (`xoxd-…`), used with `MEOW_SLACK_TOKEN` |
| `MEOW_SLACK_DOMAIN` | *(none)*                    | Slack workspace subdomain, used with `MEOW_SLACK_TOKEN` |
| `MEOW_AUTOPILOT_MODEL` | `opus`                    | Claude model for Autopilot work |
| `MEOW_CODE_DIRS`  | `~/Developer`, `~/Projects`, `~/Code`… | Where Autopilot looks for clones (colon-separated) |
| `MEOW_SUPERSET_BIN` | `superset` from your PATH, else `~/.superset/bin/superset` | Path to the Superset CLI, for coding agents |

## Develop

```sh
cargo test                                  # behaviour, sprites, CLI stream parsing
cargo run --example preview -- sprites.png  # render every pose to a PNG sheet
cargo run --example activity_preview -- activity.png  # the Activity page with sample news
cargo run --example news_bubble_preview -- bubble.png  # the cat's news bubble
```

To check the look of every page without screen access, run with
`MEOW_SNAPSHOT_DIR=<folder>`: Meow opens the bubble, saves each page (Chat,
Tasks, Plan, Autopilot, the Slack list, a conversation, one opened on a marked message and
Claude's panel, filled with sample messages) as a PNG there, then quits. Snapshots never connect to Slack.

| File                       | Role                                                      |
|----------------------------|-----------------------------------------------------------|
| `src/sprites.rs`           | Pixel art as text grids, composed into RGB frames         |
| `src/cat_photo.rs`         | Themes from a cat photo: shrinking it, the prompt, reading the colors |
| `src/behavior.rs`          | Cat state machine (walk, sleep, held, falling with gravity) |
| `src/actions.rs`           | Quick actions: defaults and `actions.json` loading        |
| `src/tasks.rs`             | Task list: merge, done state, persistence, `meow-tasks` parsing |
| `src/conversations.rs`     | The main chat and one conversation per task               |
| `src/legacy.rs`            | One-time move of data saved under the old name (Lazy)     |
| `src/action_results.rs`    | Last result of each quick action, saved between launches  |
| `src/sync.rs`              | Background task check: prompt and result parsing          |
| `src/plan.rs`              | Day plan: `meow-plan` parsing, agenda state, plan items as tasks |
| `src/schedule.rs`          | Daily quick actions: when one is due                       |
| `src/clock.rs`             | Local date and time                                        |
| `src/transcript.rs`        | Conversation model: turns with foldable working steps      |
| `src/markdown.rs`          | Markdown → styled spans for the transcript                |
| `src/claude.rs`            | Runs `claude -p --output-format stream-json`, parses it   |
| `src/mcp.rs`               | A minimal stdio MCP server (JSON-RPC, tools only)         |
| `src/context/`             | Meow as Claude's `meow` MCP server: system prompt, Slack and local-data tools |
| `src/autopilot.rs`         | Autopilot jobs: triage prompt and plan, work prompt, held actions |
| `src/guard.rs`             | `meow --guard`, the hook holding calls that cannot be undone |
| `src/worktree.rs`          | Finding a repository's clone, worktrees at a PR's branch  |
| `src/superset.rs`          | The `superset` CLI: projects, workspaces, agents          |
| `src/agent.rs`             | Coding agents: `meow-agent` proposals, prompts, transcript parsing |
| `src/agent_run.rs`         | One agent launch: worktree, open, start, follow its transcript |
| `src/slack/`               | Live Slack: sign-in credentials, Web API, background client, message model and `mrkdwn` formatting |
| `src/app/`                 | 30 Hz tick tying the cat, chat, hotkey and runs together  |
| `src/ui/`                  | AppKit: speech-bubble window, sidebar, chat/task/plan pages, theme, menus |
