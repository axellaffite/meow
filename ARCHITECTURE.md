# Architecture

## Why it is shaped this way

Meow grows by features developed in parallel, often by several agents at once
in separate worktrees. Before this layout, every feature had to add lines to
the same ten shared files: the `App` struct, `tick()` and the `handle()` match
in `src/app/mod.rs`, the `UiEvent` enum, one Objective-C method per button in
`src/ui/controller.rs`, the menus, the sidebar's `Page` enum, `Settings`, the
README. Unrelated PRs then conflicted on those lists.

The rule now: **a new feature adds files; it changes at most one line of a
shared file.**

## Layout

```
src/
  main.rs             starts the app (or `meow --mcp` / `meow --guard`)
  lib.rs, *.rs        the `meow` library: pure logic (parsing, models, prompts,
                      state machines), no AppKit, tested with `cargo test`
  app/                the core: the 30 Hz tick, the windows, the conversations
    feature.rs        the `Feature` and `Host` traits, and how the app polls features
    mailbox.rs        a feature's own message queue
  features/           one folder per feature (the app side of it)
    mod.rs            the registry: `mod x;` and one entry in `all()`
    testing.rs        what feature tests share: a fake `Host`, menu helpers
    appearance/       mod.rs, tests.rs
    agenda/           mod.rs, tests.rs
    background_check/ mod.rs, tests.rs
    daily_actions/    mod.rs, tests.rs
    routines/         mod.rs (state, messages, `impl Feature`), page.rs, tests.rs
    slack/            mod.rs, one file per part (conversations, assist, voice,
                      emoji, downloads, events, autopilot), snapshot.rs, tests.rs
    update/           mod.rs, tests.rs
  ui/                 AppKit building blocks: windows, pages, widgets, menus
    action.rs         Objective-C targets and delegates that run closures
docs/features/        one page of user documentation per feature
```

## A feature

A feature lives in `src/features/<name>/` and owns:

- **its state**: a struct implementing `Feature`, never a field on `App`;
- **its messages**: a private `enum Msg`, sent through its `Mailbox<Msg>` by
  its menu items, buttons and commands, and drained in `poll`;
- **its UI wiring**: menu items as `MenuEntry` values (actions, check marks,
  labels) whose closures send to the mailbox (`ui::action::Action` does the
  same for buttons);
- **its settings**: its own `#[serde(default)]` struct, read with
  `Store::get` and saved with `Store::set`, which leaves the other owners'
  fields alone (`settings.json` stays one flat file), and its sections of the
  Settings page as `SettingsSection` values (toggles, choices, texts,
  buttons, notes) whose closures send to the mailbox, like menu items;
- **its page**, if any: a `PageSpec` (sidebar row, header, count chip) and the
  view it builds and draws itself (`features/routines/page.rs`);
- **its part in the chat**: `/name` commands, text added to each chat prompt,
  and fenced blocks it reads out of Claude's replies;
- **its tests**: `tests.rs`, against a fake `Host`, so no AppKit is needed;
- **its docs**: `docs/features/<name>.md`, with its environment variables.

Pure logic that does not need the app (parsing, prompts, file formats) goes in
the `meow` library as before (e.g. `src/update.rs`), where it is tested on its
own; the feature folder holds the app side.

```rust
pub(crate) trait Feature {
    fn poll(&mut self, host: &mut dyn Host);             // every tick
    fn startup_error(&self) -> Option<String>;           // saved data unreadable
    fn command(&mut self, text: &str, chat: &ChatId) -> bool;          // `/update`…
    fn chat_instruction(&self, now: u64) -> Option<String>;            // into chat prompts
    fn read_reply(&mut self, chat: &ChatId, text: &str, done: bool) -> String; // its blocks out
    fn menu(&self) -> Vec<MenuEntry>;                    // cat's menu items
    fn menu_section(&self) -> MenuSection;               // with the cat's, or above Quit
    fn settings(&self) -> Vec<SettingsSection>;          // its part of the Settings page
    fn pages(&self) -> Vec<PageSpec>;                    // sidebar rows, in `order`
    fn build_page(&mut self, id, kit: &PageKit) -> Option<PageView>;   // once each, at launch
    fn page_shown(&mut self, id);                        // about to be shown
    fn open_link(&mut self, link: &Link, host) -> bool;  // an Activity item it shows
    fn escape(&mut self, host) -> bool;                  // Esc on its page
    fn quit(&mut self);                                  // save before quitting
    fn snapshot_steps(&self) -> Vec<&'static str>;       // snapshot mode, by default its pages
    fn show_snapshot(&mut self, step, host);
    fn is_busy(&self) -> bool;                           // a restart would lose work
    fn thinking(&self) -> bool;                          // the cat shows `…`
}
```

Every method but `poll` has a default, so a feature implements only what it
uses. A feature reaches the rest of the app only through `Host`: write in a
conversation (and flag it with `!` over the cat), post a banner, record an
Activity item (and mark items read), open a page or ask whether one is on
screen, get the Claude setup and the calendar tools, start and follow coding
agents, read and change
the task list (then say it changed), show a status in the Tasks header, read
the quick actions and their results, update a day plan's agenda, run a quick
action, read and save its settings,
dress the cat, size the chat bubble, have its Settings sections asked for again, set its
sidebar count, ask whether the app is idle, quit. When a feature needs
something new from the app, add one method to `Host` rather than handing out
`App`. Features do not call each other.

Data the app owns and several parts share, like the task list, stays in the
app; features reach it through `Host`, never keep their own copy.

Drawing happens in `poll`: a feature marks itself dirty when its data changes
or its page is shown, and redraws its page and count there. Snapshot mode
(`MEOW_SNAPSHOT_DIR`) saves every feature page as `<id>.png`, with no change
to `app/snapshot.rs`; there `Host::claude` is `None`, so no feature runs
Claude to act or change data while snapshots are taken.

`ui/` never imports from `app/` or `features/`: the examples compile it on
its own (`#[path = "../src/ui/mod.rs"]`).

### Adding one

1. `src/features/<name>/mod.rs`: the struct, `enum Msg`, a `Mailbox<Msg>`,
   `impl Feature`. Copy `features/update` as a starting point.
2. `src/features/<name>/tests.rs`: drive it through `command`, its menu
   (`testing::click`, `testing::describe`) and `poll`, with `testing::FakeHost`.
3. `src/features/mod.rs`: `mod <name>;` and one line in `all()`, in
   alphabetical order.
4. `docs/features/<name>.md`, and one link in the README's list, in
   alphabetical order.

Do **not** add a variant to `UiEvent`, a method to `Controller`, a field to
`App` or a `poll_x()` to `tick()`: those are the shared lists this layout
exists to retire.

## Keeping merges clean

- Shared lists (module declarations, the registry, the README's links) hold one
  item per line, in alphabetical order: two branches then add different lines.
  `features::all` is marked `#[rustfmt::skip]` so rustfmt keeps it that way.
- Keep PRs to one feature. A change to the core (`app/`, `Host`, `ui/` widgets)
  goes in its own small PR, merged first.
- Rebase open branches as soon as a core PR lands.

## What is a feature, and what is the app

Every part that can stand alone is a feature in `src/features/`: `agenda`,
`appearance`, `background_check`, `daily_actions`, `routines`, `slack` (with
Autopilot, which works from Slack messages) and `update`.

The rest is the app itself, shared by everything else; it stays in
`src/app/`, and features reach it through `Host`:

- the chat: conversations and requests (`requests.rs`), quick actions
  (`quick_actions.rs`, a quick action is a kind of conversation) and their
  day plan page with invitation answers (`plan.rs`, `rsvp.rs`);
- the task list and the Tasks page (`task_actions.rs`), which the chat, plans,
  the background check and Slack all use;
- Activity (`activity.rs`): features write to it, its items open their pages
  (`Feature::open_link`), and it drives the news bubble over the cat;
- the Settings page (`ui/settings_page.rs`, drawn in `feature.rs`): the
  features' sections, in their order, at the bottom of the sidebar;
- coding agents (`agents.rs`), started from the chat's `/agent` and from
  Slack's panel;
- the calendar tools found at launch (`calendar.rs`), used by Slack, the
  agenda and invitation answers.

`UiEvent` and `Controller` only carry the app's own controls; features wire
theirs with closures (`ui::action`). Large views may stay in `src/ui/` (the
Slack and Autopilot pages do), as long as they report through closures and
import nothing from `app/` or `features/`.

When one feature needs another's work, it goes through the app: Slack's panel
starts coding agents through `Host`, and redraws when `Host::agents_revision`
changes.

## Module map (library)

| File                       | Role                                                      |
|----------------------------|-----------------------------------------------------------|
| `src/sprites.rs`           | Pixel art as text grids, composed into RGB frames         |
| `src/cat_photo.rs`         | Themes from a cat photo: shrinking it, the prompt, reading the colors |
| `src/appearance.rs`        | The coat worn, themes from photos, the bubble's size      |
| `src/settings.rs`          | `settings.json`, each owner reading and saving its own fields |
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
| `src/update.rs`            | Releases: `version.json`, download, staging, swap and relaunch |
| `src/slack/`               | Live Slack: sign-in credentials, Web API, background client, message model and `mrkdwn` formatting |
| `src/routines.rs`          | Scheduled tasks: `meow-schedule` parsing, when each is due, prompts |
| `src/creature/`            | A 3D creature from a GLB: skeleton, sockets, clips, poses (docs/3d.md) |
| `src/motion/`              | Plays a 3D creature's clips: crossfades, overlays, springs, frames |
| `src/render/`              | Draws a 3D creature with Metal: shaders, offscreen, pacing, hit test |
| `src/app/`                 | The app: the 30 Hz tick, the windows, the chat, tasks, Activity, agents |
| `src/ui/`                  | AppKit: speech-bubble window, sidebar, chat/task/plan pages, theme, menus |

Moved features are not listed here: each folder's `mod.rs` comment says what
it does.
