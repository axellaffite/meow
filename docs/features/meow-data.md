# Meow's data in the chat

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
