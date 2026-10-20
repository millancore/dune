# dune: Mod+t stacked → tabs, and a "+" in tab bars

**Objective**: Mod+t turns the windows stacked inside the focused tile into tabs of its
container; every tab bar ends with a "+" that adds an empty container (launcher in it)
as a new tab.

**Why**: user request (2026-10-20).

**Scope**: compositor only (`src/`). TDD: off (no project/session setting); checks:
`cargo test`, `cargo build --release`.

**Definition of the stack** (user clarified): the windows living inside one tile
(`Tile.stack`: swallowed by an app, or stacked into it). The accordion (rows folded
into bars) is NOT a stack: it is only for rows shrunk until useless
(`resize_height` / `TAB_THRESHOLD`), so Mod+t does not touch it.

## Tasks
- [x] T1 `Dune::stack_to_tabs` on Mod+t: each window below the top gets its own tile,
      added as a tab after the focused one; the focused one stays shown. Floats skipped
      (floats have no tab bars).
- [x] T2 "+" after the last tab (`plus_rect`, `PLUS_WIDTH`; `tab_rects` leaves room),
      drawn in the strip, click → `open_empty_tab` (empty tab with the launcher).

- [x] T3 Launcher stack option removed (user: the accordion is only the automatic
      fold of shrunk inner tiles): `@s`, Ctrl+s, `Dest.stack`, `Spot::Stack`,
      `Dune::stack_tile` gone. `Layout::stack_on` stays (used by `restore`/`Home::Stacked`).

## Progress / evidence
- First T1 converted accordion rows; dropped after the user's clarification
  (`Layout::stack_to_tabs` and its test removed).
- `cargo test`: 81 passed. `cargo build --release`: ok. `tab_bar_layout` test covers
  `plus_rect`. No clippy warnings from the new code.
- Not checked in a live session yet (no test harness for input/render).

## Next step
Install and try it in a session. T3: `cargo test` 81 passed, release build ok.
