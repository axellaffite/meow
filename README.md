# Meow

**A pixel-art cat that lives at the bottom of your Mac's screen, and quietly
takes care of your day.**

Meow walks along the bottom of your screen, naps, plays with your mouse
pointer and, now and then, coughs up a hairball. Click it and it becomes your
assistant: ask it anything, and it plans your day, keeps your to-do list, reads
your Slack and remembers what you asked it to do later. It runs on your Claude
subscription.

## Talk to the cat

Click the cat, or press **⌥⇧Space** from anywhere, and a pixel speech bubble
opens. Type what you need and press Enter: *"what's on my plate today?"*,
*"summarize my unread emails"*, *"what did Ana ask me this morning?"*

While it works, the cat sits with a little `…` over its head. Close the bubble
and carry on: when the answer is ready, the cat shows a `!` until you come back
to read it. Several conversations can run side by side, one per task.

## What it does for you

**Plans your day.** Every morning at 7:00, before you even ask, Meow puts
together your day: the meetings ahead (with their room), the three things to
focus on first and why, the replies you owe, and the rest for later. Unanswered
invitations come with Accept and Decline buttons, and the agenda keeps itself
up to date as your calendar changes.

**Keeps your to-do list.** Meow gathers what needs you from Slack, GitHub,
Linear and your mail into one list, sorted by where it comes from. Every few
minutes it looks again: it adds what's new and removes what got done
elsewhere, like a pull request merged or an email answered. When you tick a
task yourself, Meow can draft the reply that says so.

**Brings Slack along.** Read and answer your direct messages and channels right
in the bubble, emoji and images included. Ask Meow privately about a
conversation, have it draft your reply (in your own writing style, if you like),
or turn "let's meet tomorrow" into a calendar event.

**Remembers things for later.** *"At 17:00, send me a summary of my unread
emails."* *"When Staples replies to Cyril, let me know."* Meow schedules it,
keeps watch, and tells you, or does what you asked, when the time comes.

**Tells you what happened.** While you were away, the Activity page collects
what came up: new messages, calendar changes, tasks found or done, scheduled
tasks that ran. The cat's `!` turns into a little bubble showing where the
news comes from.

**Gets work started for you.** With Autopilot on, when someone asks you for
something on Slack ("your PR has conflicts", "can you look at why the build
fails"), Meow prepares the work by itself. Anything that can't be undone, like
sending a message or publishing code, waits for your approval. It's off until
you turn it on.

**Hands off coding work.** Ask in the chat, or from a Slack conversation, and
Meow starts a coding agent in [Superset](https://superset.sh) on its own copy
of your project, ready for you to review.

## A real cat

Meow isn't only useful, it's good company.

- **Pet it** by stroking it with the pointer: it closes its eyes and little
  hearts float up.
- **Pick it up** by dragging it: it kicks, then hangs limp. Throw it and it
  bounces off, clings to the side of the screen, or hangs from the top. With
  several screens, toss it from one to the other.
- **Feed it** a bowl of kibble, or **let a mouse in** for it to chase, from its
  menu (right-click the cat).
- **Clean up after it**: now and then it coughs up a hairball that stays on the
  floor until you scrub it away, or for 10 seconds.
- **Dress it like yours**: pick a coat (Ginger, Willow, Alfie, a corgi, or
  Dédé, who is a pig and has his own habits), or show Meow a photo of your own
  cat and it takes on its colors.

## Get Meow

1. Download **Meow.dmg** from the
   [latest release](https://github.com/axellaffite/meow/releases/latest), open
   it and drag Meow to your Applications folder.
2. The first time, right-click Meow and choose **Open**.
3. Meow works through your Claude account: install
   [Claude Code](https://claude.com/claude-code) and sign in once. To let Meow
   see your calendar, mail, Linear or GitHub, connect them to your Claude
   account.

Meow keeps itself up to date: new versions install themselves while you're not
using it.

## Good to know

- **Slack** connects with your own sign-in, the way your browser does, rather
  than through a Slack app. Check that this is fine at your workplace before
  you use it; the [Slack page](docs/features/slack.md) explains what it means.
- **Meow acts when you ask.** Its background checks only read, and Autopilot
  holds every step that can't be undone until you approve it.

## Learn more

Every click, key and gesture: [Using Meow](docs/using.md).

Each feature has its own page with the details:
[Activity](docs/features/activity.md),
[Autopilot](docs/features/autopilot.md),
[Background check](docs/features/background-check.md),
[Coding agents](docs/features/coding-agents.md),
[Meow's data in the chat](docs/features/meow-data.md),
[Quick actions](docs/features/quick-actions.md),
[Scheduled tasks](docs/features/scheduled-tasks.md),
[Slack](docs/features/slack.md),
[Tasks](docs/features/tasks.md) and
[Updates](docs/features/update.md).

Building Meow yourself or working on it: see
[Developing Meow](docs/development.md).
