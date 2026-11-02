# Notifications

Everything that happens in the background goes to the Activity page; some of it
also shows a macOS banner. **Settings › Notifications** decides which:

- **Show macOS banners**: off, Meow never shows one (updates included).
- One switch per kind of news: **Slack messages**, **Calendar changes**, **New
  tasks**, **Scheduled tasks** and **Autopilot**. A kind turned off still goes to
  Activity, and still shows the cat's `!`; only its banner goes.

Tasks the background check finds resolved at the source never show a banner:
they need nothing from you. New tasks held by the grace period (see
[Background check](background-check.md)) show their banner when they join the
list, not when they are found.

The settings are saved in `settings.json` under `banners`
(`{"on": true, "muted": ["slack"]}`). No environment variable.
