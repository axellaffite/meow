# Slack

The sidebar's **Slack** page (also right-click the cat › **Slack**, or ⌘L) is a
live, two-way Slack chat, separate from Claude. Two tabs list your **Direct**
and group messages and your **Channels** (the ones in your Slack sidebar, most
recent first), each with how many conversations have unread messages; open one
to read it and reply from the input. Emoji show as emoji: standard
`:shortcodes:` (with skin tones) and your workspace's custom ones, whose images
are downloaded when first seen and cached in `~/Library/Caches/Meow/emoji`. Attached
images show in the conversation too (their thumbnail, fetched with your session
from `files.slack.com` only, cached in `~/Library/Caches/Meow/slack-files`);
click one to open it in Slack. New messages arrive within a few seconds and the cat shows `!` when
someone writes to you while you are elsewhere.

**Sign-in uses no Slack app.** Click **Sign in to Slack** and log in with your
usual SSO in the window that opens. Meow then reads the same session token and
`d` cookie the Slack web app itself uses, and keeps them in the macOS Keychain —
never in a file or a log. This is how unofficial clients like wee-slack and
slackdump work.

- **This goes around your workspace's app-approval control on purpose.** Slack
  can't tell it apart from your browser, but your IT or security team may not be
  fine with it — only you can judge that.
- **It's unofficial and unsupported by Slack**, and may break if Slack changes
  its web client.
- The token is as powerful as your login; it can't be revoked on its own
  (signing out of all Slack sessions ends it). When the session ends, Meow asks
  you to sign in again.

To try it without the sign-in window (e.g. to check your workspace allows this
access), set `MEOW_SLACK_TOKEN` (`xoxc-…`), `MEOW_SLACK_COOKIE` (`xoxd-…`) and
`MEOW_SLACK_DOMAIN` (your workspace subdomain) before launching.

## Settings (environment variables)

| Variable | Default | Effect |
|---|---|---|
| `MEOW_SLACK_TOKEN` | *(none)*                     | Slack session token (`xoxc-…`), to skip the sign-in window |
| `MEOW_SLACK_COOKIE` | *(none)*                    | Slack `d` cookie (`xoxd-…`), used with `MEOW_SLACK_TOKEN` |
| `MEOW_SLACK_DOMAIN` | *(none)*                    | Slack workspace subdomain, used with `MEOW_SLACK_TOKEN` |
