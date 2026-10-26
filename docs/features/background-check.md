# Background check

Every 5 minutes Meow quietly asks Claude (Haiku, read-only, through your
connectors) to look at your open tasks and at what is new since the last check:
tasks resolved at the source (PR merged or reviewed, Linear issue done, Slack
thread or email answered) are removed from the list, and new things waiting on
you are added. A removed task is remembered for 30 days, so the check never
adds it again. The Tasks header shows when it last checked. Set
`MEOW_SYNC_MINUTES` to change the interval, or to `0` to turn it off. Each check
uses a little of your Claude plan.

## Settings (environment variables)

| Variable | Default | Effect |
|---|---|---|
| `MEOW_SYNC_MINUTES` | `5`                         | Minutes between background task checks (`0` = off) |
