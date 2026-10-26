# Quick actions

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
