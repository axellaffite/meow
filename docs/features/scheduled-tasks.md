# Scheduled tasks

Ask in the chat for something later and Claude schedules it instead of doing it
now:

- **At a time**: "at 17:00, summarize my unread emails", "every day at 9:00,
  list the PRs waiting for me". It runs at that time with the chat's
  permissions.
- **When something happens**: "when Staples replies to Cyril's email, let me
  know". Claude checks every 10 minutes (or as asked), read-only, and when it
  happens tells you with the details, or does what you asked.

Claude also sees your scheduled tasks, active and finished (for 7 days), with
what they do and their last result. So you can ask "start the Staples watch
again", "move the report to 18:00" or "cancel the PR reminder" without
repeating the details.

The **Scheduled** page lists them with their next run, what they do and their
last result; `×` deletes one. Results land in Activity. Runs happen in the
background, one at a time, and never block the chat.
