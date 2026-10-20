# dune: Super+Enter opens an empty container with the launcher

## Objective
User request 2026-10-20: Mod+Enter opens an empty container (where a terminal used to
open) with the launcher drawn in its middle. Enter right away puts a terminal in it;
choosing or typing a program puts that program in it instead. The general launcher
(Mod+d) does not change.

## Decisions / constraints
- The empty container is a tile with no window yet, placed like Mod+Enter's terminal was
  (`insert_tile(id, None, None)`: a new column right of the focused one). It takes space
  while empty (`Tile.reserved`), is focused and has its border.
- Launcher "slot" session (`Launcher::open_in`): same picker, but
  - Enter with nothing typed and the list not browsed (no Up/Down/Tab) → terminal.
  - A console program → a terminal running it, in the container (`DUNE_TILE`).
  - A graphical app → `Spot::Into(tile)`: its first window goes into the container.
  - Ctrl keys and `@` words do not apply (the place is the container); its hint says so.
- Panel centered in the container (clamped to it), as wide as fits.
- Esc / closing the launcher: the empty container goes away.
- An empty container left with no window (an app that never opened one) can be closed
  with Mod+q; a forgotten launch (60 s) drops it at the next launch.
- `spawn_terminal` (IPC, `@prog` paths) stays as it is.

## Checks
TDD: off (no project/session config). Runner: `cargo test --workspace`, `cargo build --workspace`
(no warnings).

## Tasks
- [x] T1 launcher.rs: slot session, Enter → terminal rule, `Spot::Into`, hint (+ tests)
- [x] T2 tiling/state/input: reserved tile, Mod+Enter action, place window into slot,
      cancel/close cleanup
- [x] T3 render: panel centered in the slot
- [x] T4 Config doc for Mod+Enter; build, test

## Progress
Started 2026-10-20. T1-T4 done 2026-10-20 (not checked live).

## Verification
- T1: 3 new launcher tests (`enter_alone_in_an_empty_container_is_a_terminal`,
  `what_is_chosen_goes_into_the_empty_container`, `browsing_in_an_empty_container_runs_the_highlighted_app`).
- T2-T4: `cargo build --workspace` 0 warnings; `cargo test --workspace` 80 + 32 + 18 passed, 0 failed.
- Not done: live check in a real session (needs install + session restart).

## Next step
Install, restart the session and check: Mod+Enter then Enter (terminal), Mod+Enter then
an app (GUI and console), Esc, Mod+d unchanged.
