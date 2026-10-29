# Activity

The **Activity** page collects what happened while you were away: Slack DMs and
mentions, calendar changes found by the agenda refresh (new, moved or cancelled
events, new invitations, answers sent), tasks found or resolved by the
background check, and scheduled tasks that ran or fired. Each item shows the
pixel-art icon of where it comes from (Slack, Calendar, GitHub, Linear, Mail,
Notion…): one icon per source for up to three, and past that a single stack
icon with their number; hover the icons to see the sources. A Slack sender
keeps one item per conversation, with only their last message. Each new item also
shows a macOS banner (the bundled app asks permission once; a `cargo run` build
posts through `osascript`). Opening the page marks everything read; `×`
dismisses one item, **Clear all** empties the list, and items go after 7 days.
Clicking an item opens what it is about: a Slack message opens the Slack page scrolled to that message, highlighted (or to its thread when the reply itself is not loaded).
While news is unread, a bubble over the cat shows where it comes from; turn off **Show news over the cat** in Settings › Appearance to keep only the cat's `!`.
