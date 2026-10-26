# Meow

Read ARCHITECTURE.md before adding or changing a feature.

- New feature work goes in `src/features/<name>/` (copy `src/features/update`),
  plus one line in `src/features/mod.rs` and its own `docs/features/<name>.md`.
- Never add a `UiEvent` variant, a `Controller` method, a field on `App` or a
  `poll_x()` in `App::tick`: reach the app through `Host`, wire menus and
  buttons with closures sending to the feature's `Mailbox`.
- A feature's settings are its own `#[serde(default)]` struct, read with
  `settings::Store::get` and saved with `settings::Store::set`.
- Shared lists (module declarations, `features::all`, the README's feature
  links) stay one item per line, in alphabetical order.
- What stays in `src/app/` is the app shared by all (chat, quick actions and
  plans, tasks, Activity, agents, calendar tools): change it in its own PR.
- Before a PR: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`.
