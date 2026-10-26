# Releases and updates

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
