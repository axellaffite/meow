# Using Meow

Everything the cat and its bubble respond to. For an overview, see the
[README](../README.md).

- **Click the cat** or press **⌥⇧Space** to open the chat, type a request, press Enter.
- While Claude works, the cat sits with a `…` bubble. If the reply arrives while
  the chat is closed, it shows a `!` until you click it.
- When there is unread news in **Activity**, the `!` becomes a bubble with the
  icons of where it comes from (Slack, GitHub, Linear…; past three sources, one
  stack icon with their number). Clicking the cat then opens Activity.
- Answers are rendered from Markdown: headings, bold/italic, code, lists,
  tables, quotes and clickable links.
- Working steps (tool calls, sub-agents, interim messages) are folded under a
  `▸ N steps` line above each answer; click it to show them.
- **Esc** cancels the request of the conversation on screen, or closes the
  chat. Other conversations (another task, a quick action) keep theirs running:
  each conversation runs one request at a time, side by side with the others.
- **New** (or `/new`, or ⌘N) starts a fresh conversation. Otherwise follow-ups
  continue the same Claude session (`--resume`).
- **Right-click the cat** for the quick actions, Open Chat, New Conversation,
  Settings and Quit. Quick actions are also in the chat's sidebar.
- **Settings** (at the bottom of the sidebar, in the cat's menu, or ⌘,) holds
  every choice Meow keeps: the cat's look, drafting Slack replies in your
  style, and Autopilot (on or off, when it starts, how it works).
- **Clicking a quick action shows its last result** (and when it ran);
  **[ RUN ]** runs it again. An action that never ran runs on the first click
  (in the background from the cat's menu; the cat shows `!` when done). Ask
  follow-up questions right under the result.
- **Drag the cat** to pick it up by the scruff: it kicks, then hangs limp and
  swings as you move it. Let go and it falls back down; drag fast to throw it
  (thrown hard at a screen edge it clings to it with its paws, then
  hops down, or now and then slides down the wall; thrown hard at the top it hangs there upside down, then drops; a
  gentle throw just bounces off). With several displays, carry or throw it
  across to another one: only the outer edges of your screens are walls.
- **Pet the cat** by stroking it: move the mouse back and forth over it (no
  click). It closes its eyes and little hearts float up.
- **Dress the cat like yours**: **Settings › Appearance › Theme** picks its coat
  (Ginger, Willow, Alfie, or Dédé, who is a pig). **Add from Cat Photo…** asks for a photo of your
  cat; Claude (Haiku, allowed only to read a shrunk copy of the photo) picks
  the color of its fur, stripes, blaze, muzzle, legs, tail and each eye, and
  names the theme (after the file's name when that looks like the cat's). The
  cat sits with a `…` bubble meanwhile, then wears it. Your themes are listed
  under the built-in ones; a new photo with the same name replaces the old
  theme, and **Remove** deletes the one being worn. They are saved in
  `~/Library/Application Support/Meow/settings.json`. The bubble's size and
  the style (Pixel Art or Classic, which relaunches Meow) are there too.
- **The cat stretches** now and then, and often as it wakes from a nap: a bow,
  front paws reaching ahead and rump in the air, or its back arched high,
  eyes shut. Walking into the edge of the screen, it sometimes rears up
  against it and claws it for a moment before turning back (not at an edge
  that leads on to another screen). Dédé the pig does neither.
- **Now and then the cat coughs up a hairball** (or on demand: **Hairball** in
  its menu). It stays on the floor until you clean it (scrub it with the mouse
  button down, or click it a few times, until it sparkles and is gone), or
  for 10 seconds at most.
- **Dédé the pig poops instead** (the item is called **Poop** then): he
  squats, drops it behind him, turns around to admire it, hearts floating up,
  and eats it. Wipe it before he does and he goes off disappointed.

## Look

The chat opens as the cat's pixel-art speech bubble (drag it by its background,
close it with Esc, the ✕, by clicking the cat, or just by clicking elsewhere), dark
with an 8-bit palette (Sweetie 16). Text uses three pixel fonts: VT323
for messages, Silkscreen for labels and Press Start 2P for titles, bundled in
`assets/fonts` under the SIL Open Font License.
