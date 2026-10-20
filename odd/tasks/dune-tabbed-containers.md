# dune: tabbed containers

## Objective
User request 2026-10-20: a container (a tile slot in a row) can hold several programs as
tabs. They share the same geometry, as layers; only the active one is shown. The tab bar
goes at the top, tabs aligned left; later its placement must be configurable.

## Problem
Today the only way to put two apps in one place is the accordion "stack" (`Layout::stack_on`,
launcher Ctrl+S / `@s`): the old row folds into a 24px title bar *above* the new one.
That is a different thing and stays (user decision 2026-10-20: keep both mechanisms).

## Decisions / constraints
- Model (layout.rs, pure): `Layout.groups: Vec<Vec<TileId>>`. The row slot holds the group's
  *active* tab; inactive tabs live only in their group. `find`/`contains` map an inactive tab
  to its slot. A group of one dissolves (no bar).
- Removing the active tab activates its neighbour (next, else previous) in the same slot;
  focus goes to that sibling.
- `geometries` gives the active tab the container rect minus the bar; bars are kept in the
  layout (`tab_bars()`), each with its rect, tabs and active one. Placement goes through one
  function (`split_tab_bar`, top) and tab labels are laid left-aligned (`tab_rects`), the hook
  for a future `tab_bar = ...` config setting. Not configurable yet.
- Inactive tabs: window unmapped, configured to the active tab's size (switching is instant).
- Focusing an inactive tab (by any means) activates it, like accordion tabs open on focus.
- `Home::Tab { with, index }` so a tab leaving (float, desk move) comes back into its group.
- Keys (Mod = configured modifier):
  - Mod+Tab / Mod+Shift+Tab: next / previous tab of the focused container.
  - Mod+Ctrl+Direction: alone → join the neighbour container in that direction as a tab;
    in a group → leave it, into a container of its own on that side.
    (Not available when the modifier itself is ctrl+alt.)
  - Click on a tab in the bar: activate and focus it.
- Launcher: Ctrl+T toggles "open as a tab of the focused container", `@t` word too.
- Accordion stack keeps working unchanged.

## Checks
TDD: off (no project/session config). Runner: `cargo test --workspace`, `cargo build --workspace`
(no warnings). Nested smoke run if possible.

## Tasks
- [x] T1 layout.rs: groups, add/activate/cycle/remove, home/restore, bars, geometries (+ tests)
- [x] T2 tiling.rs: arrange (unmap inactive, configure size, collect bars), focus activates,
      close focuses the sibling
- [x] T3 Tab bar drawing (titles, active highlighted) and render elements
- [x] T4 Input: keys, click on a tab; launcher Ctrl+T / `@t` (+ tests)
- [x] T5 Config doc comment for the keys; build, test (smoke run not done, see Verification)
- [x] T6 Mod+drag a tile (tiled or float) onto a tab bar: it joins that container as a tab;
      the whole container is highlighted while dragging over its bar (user request 2026-10-20)

## Progress
Started 2026-10-20. T1-T5 done 2026-10-20 (nested smoke run not done).

## Verification
- T1: `cargo test -p dune layout`: 36 passed (7 new tab tests).
- T2: `cargo build -p dune` clean.
- T3: `cargo test -p dune tab_title`: 2 passed (new `strip_lights_the_shown_tab`).
- T4: new tests `tab_keys` (input.rs), `@t` / Ctrl+T cases in launcher tests.
- T5: `cargo build --workspace`: 0 warnings, 0 errors. `cargo test --workspace`: 77 + 32 + 18 passed, 0 failed.
- T6: `cargo build --workspace` clean, `cargo test --workspace` 77 + 32 + 18 passed. Not checked live.
- Not done: nested smoke run (`tests/e2e_nested.py` opens a window on the live desktop) and a
  live check in a real session (needs install + session restart).

## Notes
- A just-launched tab without a window yet: its container keeps showing a tab that has one
  (no reflow of the strip while it starts).
- The focus border of a tabbed container takes in its bar.
- Mod+Shift+Tab is matched both as Tab+Shift and as ISO_Left_Tab.

## Next step
Install, restart the dune session and check: Ctrl+T in the launcher, Mod+Tab, Mod+Ctrl+arrows,
clicking tabs, closing the shown tab, Mod+dragging a tile onto a tab bar.
