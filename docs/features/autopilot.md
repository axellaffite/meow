# Autopilot

The sidebar's **Autopilot** page lets Meow start work by itself when a Slack
message asks you for something: "your PR #42 has conflicts", "can you look at
why the build fails", "send me the numbers for X". Turn it on with **Turn on**
on the page (it is off by default).

- **Triage:** once a DM, group message or mention (in a thread too) has been
  quiet for 45 seconds (so a request written in several messages is read
  whole), Claude (Haiku, with no tools) reads the conversation's last messages
  (or the thread's) and decides whether there is work to prepare. This works
  in any conversation, open in Meow or not, and leaves it unread. Greetings,
  news and decisions only you can make are left alone, as is a conversation
  you already answered. Add your own rules in `~/.config/meow/autopilot.md`,
  e.g. "Only for messages from Axel".
- **Work:** one job at a time, with your usual tools and connectors, on Opus
  (`MEOW_AUTOPILOT_MODEL` picks another). Code work happens in a new git
  worktree of your clone, at the tip of the PR's branch (found with `gh`, the
  clone found under `~/Developer` and the usual folders, or `MEOW_CODE_DIRS`),
  never in your own checkout. The agent merges, edits, builds, tests and
  commits there. A PR given by number alone ("PR #1727") is looked up among
  your open PRs and the ones you are asked to review. When no worktree can be
  made (no clone, the repository unknown), the work still runs, in an empty
  folder, and the agent is told why and to name the repository to `gh`.
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

## Settings (environment variables)

| Variable | Default | Effect |
|---|---|---|
| `MEOW_AUTOPILOT_MODEL` | `opus`                    | Claude model for Autopilot work |
| `MEOW_CODE_DIRS`  | `~/Developer`, `~/Projects`, `~/Code`… | Where Autopilot looks for clones (colon-separated) |
