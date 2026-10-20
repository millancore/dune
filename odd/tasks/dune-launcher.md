# dune — compositor launcher (rofi-like)

## Objective
User request 2026-10-19: the `:` launcher inside the terminal is unreliable (after typing and erasing,
`:` no longer opens it, because the shell owns line editing). Instead of fixing it, add a launcher drawn
by the compositor, like rofi, where the user also decides where the app opens.

## Decisions / constraints
- User chose: only the rofi-like launcher (`:` stays as it is); destination picked with keys inside the
  overlay.
- Mod+d opens it on the active output, over everything; it takes the keyboard until Enter / Esc.
- Search and completion reuse `dune_common::picker::Picker` (same apps, `app:v` / `app:h` still work).
- Destination next to the focused tile: Ctrl+Left/Right/Up/Down pick the side, Ctrl+f floats it in the
  middle. Default: right (a new column, like Mod+Enter). Shown in the overlay.
- Opening reuses `spawn_terminal(.., run)`: a tile running the command, swallowed by the app window.
- TDD: off (no project config, no user choice). Runner: `cargo test --workspace`.
- Checks: cargo build without warnings, cargo test --workspace, nested smoke run, install.

## Tasks
- [x] T1 `src/launcher.rs`: launcher state (picker + destination), key handling, where the line opens;
      unit tests.
- [x] T2 overlay drawing (panel with input, suggestions, destination, hint) and render integration.
- [x] T3 wiring: Mod+d, keyboard interception while open, open on the side or floating.
- [x] T4 checks: build, tests, nested smoke, install.
- [x] T5 `App.terminal` (desktop `Terminal=true`) in dune-common; the launcher says whether the line is a
      GUI app.
- [x] T6 GUI apps launched directly (no shadow terminal), env `DUNE_LAUNCH=<n>`; `place_toplevel` puts
      the first window of that launch at the chosen side / floating. Terminal apps and typed commands
      keep the terminal tile.
- [x] T7 checks: build, tests, nested smoke, install.

## Acceptance criteria
- Mod+d shows the launcher; typing filters apps; Enter opens the selection; Esc closes it.
- Ctrl+arrows / Ctrl+f change where it opens, visible before Enter; the app opens there.
- Typing and erasing never breaks it.

## Progress
- 2026-10-19 doc created.
- 2026-10-19 T1-T4 done. `Picker::browser` (dune-common) lists every app before typing, Backspace on
  empty keeps it open, Enter on empty runs the highlighted app. `src/launcher.rs`: `Launcher` (session,
  place, panel drawn with dune_common::font, cached until a key changes it), `Dune::toggle_launcher` /
  `launcher_key`. `spawn_terminal` returns the new tile (floated right away for Ctrl+f). Input: Mod+d,
  every key goes to the launcher while open (releases swallowed); render draws the panel first on its output.
  Fixed during smoke: the release of Mod+d closed the launcher again.
- Checks: cargo build no warnings; cargo test --workspace 88 pass (launcher 5, picker browser 1).
  Nested smoke (injected keys): type+erase then htop with Ctrl+Up -> tile above; Ctrl+f + Enter -> new
  tile floating centered; Mod+d, Esc -> keyboard back (Mod+Enter works). No panics. Installed.
- Not verified: panel look on screen (no screenshot), real TTY session, key repeat (held keys do not
  repeat inside the launcher).

- 2026-10-19 user: the launcher is no longer in the terminal, so GUI apps should open like rofi does,
  without a terminal behind them (T5-T7).

- 2026-10-19 T5-T7 done. `App.terminal` (Terminal=true). Launcher: `Step::Open.terminal` is false when the
  line is what a GUI app runs (`runs_gui_app`). `Dune::launch` spawns `sh -c exec <cmd>` with
  `DUNE_LAUNCH=<n>` (no DUNE_TILE/DUNE_RUN), records `Launch {place, target}` (dropped after 60 s);
  `place_toplevel` gives its first window a new tile there, floating for Ctrl+f.
  Checks: build 0 warnings; cargo test --workspace 90 pass; nested smoke: foot + Ctrl+Up -> "placing
  window: launcher" tile above; foot + Ctrl+f -> floating centered; htop -> terminal tile. Installed.
- Limits: windows from another process (XWayland apps via xwayland-satellite, D-Bus activated or
  already running single-instance apps) cannot be matched and open as a normal new tile (right of focus).

## Next step
User restarts the dune session and tries the launcher with GUI apps.
