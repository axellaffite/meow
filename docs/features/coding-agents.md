# Coding agents (Superset)

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

## Settings (environment variables)

| Variable | Default | Effect |
|---|---|---|
| `MEOW_SUPERSET_BIN` | `superset` from your PATH, else `~/.superset/bin/superset` | Path to the Superset CLI, for coding agents |
