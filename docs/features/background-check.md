# Background check

Every 5 minutes Meow quietly asks Claude (Haiku, read-only, through your
connectors) to look at your open tasks and at what is new since the last check:
tasks resolved at the source (PR merged or reviewed, Linear issue done, Slack
thread or email answered) are removed from the list, and new things waiting on
you are added. A removed task is remembered for 30 days, so the check never
adds it again. The Tasks header shows when it last checked. Set
`MEOW_SYNC_MINUTES` to change the interval, or to `0` to turn it off. Each check
uses a little of your Claude plan.

## What becomes a task

The check, quick actions and the day plan follow the same rules, drawn from the
tasks people ticked off without doing anything. Never a task: a message to a
whole channel that does not name you, reminders, bots and logistics (lunch,
receipts, time sheets, meeting rooms), a message saying there is nothing for you
to do, preparing or joining a meeting, a question someone else already answered,
and unblocking, reviewing, chasing or triaging the Linear issues of the people
you manage (a Linear issue is yours only when it is assigned to you, a pull
request only when your review is requested by name).

## Settings › Tasks

- **Grace period for messages to answer** (Off, 10 min, 20 min, 30 min — the
  default — or 1 h): a Slack message or an email to answer stays out of the
  list until this long after it was posted. The check keeps looking at it
  meanwhile: answered by then, it goes without a word; otherwise it shows, with
  its banner. The Tasks header says how many are on hold.
- **Merge repeats of the same request** (on by default): a new task about a
  request already in the list is merged into it, so it never shows twice. Two
  tasks are the same request when they name the same Linear issue (`ENG-60`)
  or pull request (`infra#707`, or its GitHub link), come from the same
  DM or the same channel thread, or when a Linear review link matches a GitHub
  pull request's title. The check is also asked to report one task per request.

Tasks the check finds resolved go to Activity without a banner.

## Settings (environment variables)

| Variable | Default | Effect |
|---|---|---|
| `MEOW_SYNC_MINUTES` | `5`                         | Minutes between background task checks (`0` = off) |
