# dune — floating tiles

## Objective
User request 2026-10-19: tiles that float in the middle of the monitor, above the strip of columns;
Super+click should make a tile float.

## Decisions / constraints
- Floating is a state of a tile, not another kind of window: the tile leaves its `Layout` and goes to a
  per-output floating stack (`Dune::floating`), keeping its `TileId` and `Tile` (borders, focus, tabs,
  swallow, direction indicator keep working).
- Floating rects are in global coordinates on their output and do not scroll with the strip.
- Floats come first in `geometries` / `shown` (front first), so `tile_at` and drops hit them first; they
  are drawn first (on top) and raised in the `Space` so clicks reach them.
- New float: ~60% of the output, centered. Back to tiling: next to the strip tile under its middle (else at the end).
- Toggle keys: Mod+Shift+space, and Mod+left click without moving the pointer (a Mod+left drag still
  moves). Mod+left drag of a float moves it freely; Mod+right drag resizes it.
- Dialogs (toplevel with a parent, or fixed size min == max) float on their own, centered.
- A monitor that goes away hands its floats to the first one; floats are kept inside their output.
- TDD: off (no project config, no user choice). Runner: `cargo test --workspace`.
- Checks: cargo build without warnings, cargo test --workspace, nested smoke run, install.

## Tasks
- [x] T1 core: `floating` state, toggle (Mod+Shift+space, Mod+click), `arrange` with floats in front,
      draw order + raise, focus/hit testing, outputs going away; unit tests for the float rect helpers.
- [x] T2 Mod+left drag moves a float, Mod+right drag resizes it.
- [x] T3 dialogs float automatically (parent or fixed size).
- [x] T4 checks: build, tests, nested smoke, install.

## Acceptance criteria
- Mod+Shift+space or Mod+click on a tile: it floats centered above the columns; again: it goes back.
- Clicks and typing reach the float, not the tile under it.
- A file picker / dialog opens floating instead of splitting the strip.

## Progress
- 2026-10-19 doc created.
- 2026-10-19 T1-T4 done. `src/floating.rs` (float state helpers, toggle, raise, drag/resize, settle,
  dialogs), `Dune::floating` + `MoveDrag` in `src/state.rs`, `arrange` puts floats first and maps back to
  front (`src/tiling.rs`), render draws floats (border over window) above the strip (`src/render.rs`),
  Mod+Shift+space / Mod+click / float drags in `src/input.rs`, dialogs checked on commit
  (`src/handlers/compositor.rs`). Closing a float no longer indexes a missing layout.
- Checks: cargo build no warnings; cargo test --workspace 36+32+14 pass (incl. floating::tests).
  Nested smoke (scratch script, injected keys, debug log): 2 tiles -> Alt+Shift+space -> tile 2 at
  570x631 centered, listed first; again -> back in the strip; float + Alt+q -> tile removed, no panic.
  Installed (cargo install --offline --path .).
- Not verified: draw order on screen (no screenshot), Mod+click / Mod+drag of floats (harness cannot
  inject pointer), dialogs floating (no dialog app run), real TTY session.

## Next step
User tries on the TTY session: Super+click, Super+drag of a float, Super+right drag, a file picker.
