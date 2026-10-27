# Privacy: private tiles and locked desktops

Made for screen sharing: what is shared is what you see.

- A **private tile** (`Mod+/` → private, or the `private_tile` key) is blocked on screen and
  in every capture. Tab bars and dune-finder show it as `🔒 <app>`, never its title.
- A **locked desktop**: a right click on it in waybar locks it (`🔒 n`). It is never shown;
  `Mod+N`, the bar, dune-finder and notifications leave it alone. Locking the shown one
  switches to another desktop. Windows can still be sent there. Another right click, or
  choosing a window from `:l` in dune-finder, unlocks it.

```text
  ┌──────────────────────────────────────┐
  │┌──────────────┐┌────────────────────┐│
  ││              ││░░░░░░░░░░░░░░░░░░░░││
  ││   editor     ││░░░░ 🔒 slack ░░░░░░░││  ← private tile: blocked on
  ││              ││░░░░░░░░░░░░░░░░░░░░││    screen and in the stream
  │└──────────────┘└────────────────────┘│
  └──────────────────────────────────────┘
```
