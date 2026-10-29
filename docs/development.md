# Developing Meow

How Meow works under the hood, how to build it, and how to work on it. For
what the app does, see the [README](../README.md); for how the code is
organized and where new code goes, see [ARCHITECTURE.md](../ARCHITECTURE.md).

## How requests run

Requests go through `claude -p` (the Claude Code CLI in print mode), so they
use your regular Claude subscription, not API billing.

Requests use **Haiku** (latest version) by default, the cheapest Claude model,
which is plenty for fetching from connectors and summarizing; sub-agents use it
too. Claude runs with `--permission-mode auto` in your home directory, with your
usual Claude Code settings, MCP servers and skills, plus Meow's own tools (see
[Meow's data in the chat](features/meow-data.md)).

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

Meow used to be called Lazy: on first launch it moves data saved under the old
name (`~/Library/Application Support/Lazy`, `~/.config/lazy`) to the new folders.

## Settings (environment variables)

These apply to every request; each feature's page lists its own.

| Variable          | Default                       | Effect                          |
|-------------------|-------------------------------|---------------------------------|
| `MEOW_CLAUDE_BIN` | `claude` from your shell PATH | Path to the Claude Code CLI     |
| `MEOW_CWD`        | `$HOME`                       | Working directory for requests  |
| `MEOW_MODEL`      | `haiku`                       | Claude model for requests and their sub-agents (e.g. `sonnet`) |

## Develop

```sh
cargo test                                  # behaviour, sprites, CLI stream parsing
cargo run --example preview -- sprites.png  # render every pose to a PNG sheet
cargo run --example activity_preview -- activity.png  # the Activity page with sample news
cargo run --example news_bubble_preview -- bubble.png  # the cat's news bubble
cargo run --example model_preview -- model.png  # a 3D model rendered offscreen
cargo run --example model_window                # a 3D model in a window on the desktop
```

The 3D engine (`creature`, `motion`, `render`), and how to bring a model to
it, is described in [A 3D creature](3d.md).

To check the look of every page without screen access, run with
`MEOW_SNAPSHOT_DIR=<folder>`: Meow opens the bubble, saves each page (Chat,
Tasks, Plan, Autopilot, the Slack list, a conversation, one opened on a marked message and
Claude's panel, filled with sample messages, then each feature's page with your
data, e.g. `routines.png`) as a PNG there, then quits. Snapshots never connect to Slack.
