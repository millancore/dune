# dune — desktops, and launcher destinations

## Objective
User requests 2026-10-19:
- "agrega soporte para escritorios".
- In the launcher, choose whether the app is stacked in the current tile or opens in a new one, and
  choose monitor and desktop, all with a simple syntax that is quick to type.

## Decisions / constraints
- Desktops are per monitor (like niri/i3-per-output), numbered 1-9; each monitor shows one. Default 1.
- Mod+1..9 shows that desktop on the active monitor; Mod+Shift+1..9 sends the focused tile there
  (focus stays). Keys use the unshifted keysym (raw), so layouts that shift the digit row still work.
- A hidden desktop keeps its layout and floats; its windows are unmapped. A monitor that goes away gives
  its desktops to the first monitor (same number, merged).
- Launcher syntax: the last word of the line may be a destination starting with `@`, letters/digits in
  any order:
  - a digit: desktop (`firefox @3`)
  - `m` + digit: monitor, numbered left to right (`firefox @m2`, `firefox @m2 3` -> `@m23`/`@3m2`)
  - `s`: stacked in the current tile (shown on top of it, back when it closes); `n`: new tile (default)
  - `f`: floating
  Ctrl+arrows / Ctrl+f / app:v / app:h still pick the side; Ctrl+s toggles stacked. The overlay shows the
  destination as it is typed.
- Opening on another desktop does not switch to it (like i3 `assign`).
- Monitor or desktop elsewhere: the app opens next to the last tile there.
- TDD: off (no project config, no user choice). Runner: `cargo test --workspace`.
- Checks: cargo build without warnings, cargo test --workspace, nested smoke, install.

## Tasks
- [x] T1 `src/desktops.rs`: per-output current desktop, hidden desktops (layout + floats), show /
      move-to / place-on desktop, hidden tiles removed cleanly, monitor gone; unit tests.
- [x] T2 keys: Mod+1..9, Mod+Shift+1..9.
- [x] T3 launcher: `@` destination parsing, Ctrl+s, stacked / monitor / desktop placement for GUI and
      terminal apps, overlay label; unit tests.
- [x] T4 checks: build, tests, nested smoke, install.

## Acceptance criteria
- Mod+2 shows an empty desktop; Mod+1 brings the tiles back. Mod+Shift+2 moves the focused tile there.
- `foot @2` opens foot on desktop 2 without leaving desktop 1; `foot @s` stacks it on the current tile;
  `foot @m2` opens it on monitor 2.

## Progress
- 2026-10-19 doc created.
- 2026-10-19 T1-T4 done.
  - `src/desktops.rs`: `Desk {layout, floating, focused}`, `Dune.desks` / `Dune.hidden`; `show_desk`
    (remembers focus per desktop), `send_to_desk`, `place_on_desk`, `remove_from_hidden` (called by
    `remove_from_layouts`), `adopt_desks` (from `arrange` when a monitor goes away), `numbered_outputs`.
  - `place_toplevel`: a window whose tile is on a hidden desktop stays unmapped and does not take focus;
    `remove_toplevel` does not map the revealed window of a hidden tile.
  - Keys: Mod+1..9 / Mod+Shift+1..9 (raw keysym digits).
  - Picker (browse mode): a last word `@...` is set aside (`Picker::dest`, `apart`) for completion,
    Tab and the command. Launcher: `Dest::with` parses it, Ctrl+s toggles stacked, overlay shows the
    destination (or the `@` help when the word is wrong). `Dune::spot` -> `Spot::Stack(tile)` /
    `Spot::New {output, desk, target, side, float}`; `launch` handles GUI apps and terminal apps
    (`start_terminal` split out of `spawn_terminal`, with `DUNE_LAUNCH` for stacked terminals).
  - Checks: build 0 warnings; cargo test --workspace 95 pass. Nested smoke: Alt+2 empty / Alt+1 back;
    `foot @2` hidden then shown on Alt+2; `foot @s` stacked on tile 2; Alt+Shift+3 sends it; `htop @1`
    terminal tile on hidden desktop 1; `foot @m12` -> monitor 1 desktop 2. No panics. Installed.
- Not verified: two real monitors (`@m2`, monitor unplugged with hidden desktops), real TTY session,
  panel look on screen. No desktop indicator for bars yet (no IPC for it).

## Next step
User restarts the dune session and tries Mod+1..9 and the `@` syntax.

- 2026-10-19 user: desktops from the launcher "don't work" — the app opened on the other desktop without
  switching to it. Added config `follow` (default false): when true, the launcher shows the desktop the
  app opens on (GUI: before spawning, so its window maps and takes focus; terminal: after its tile is
  placed). Enabled in the user's ~/.config/dune/config.
  Checks: build 0 warnings; cargo test --workspace 97 pass (config `follow` test). Installed.
  Not verified: real session (needs restart of dune).
