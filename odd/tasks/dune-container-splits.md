# dune: stacked tabs and side-by-side splits inside a column

## Objective
User requests 2026-10-19:
- "Cuando algo esta stack ahora no se muestran las pestañas": launcher `@s` must show tabs.
- "el :h si funciona, pero el :v no" / "esta creando un nuevo contenedor": `:v` must split
  inside the current container (chosen: "Dividir dentro del contenedor", i3 style), not open a
  new scrolling column.

## Decisions
- `@s` (launcher stacked) = a new tile in the target's column, the target folds into a tab
  above it (`Layout::stack_on`); closing it gives the place back. Floating target: old
  in-tile window stacking.
- Columns hold rows; a row holds tiles side by side with width shares. Heights and tabs are
  per row (a folded row shows one bar per tile).
- Explicit Left/Right with a target (`:v`, `app:v`, launcher Ctrl+←/→, direction indicator,
  drag-drop half) splits inside the row. No side (Mod+Enter, launcher default, unfloat,
  moving to another output/desk) = new column (`Layout::insert_column`).
- Launcher default place becomes "new column" (`Place::Column`).
- Widths inside a row (added 2026-10-19, "cambiar el tamaño de los tiles internos con Alt"):
  Mod+Ctrl+= / Mod+Ctrl+- resize the focused tile in its row (`Layout::resize_row_width`,
  min share 0.1); Alt+scroll over a tile with side-by-side neighbours does the same, else the
  scroll reaches the window. Mod+=/- still act on the column.

## Checks
TDD: off (no project/session config). Runner: `cargo test --workspace`, `cargo build --workspace`.

## Tasks
- [x] T0 `@s` stacks as a tab (`Layout::stack_on`, `Dune::stack_tile`, launcher/place_toplevel);
      closing an open tile reopens the tab above it. Test added; suite green; installed.
- [x] T1 layout rows: Row{tiles, widths}; find (c, r, k); insert_on Left/Right in-row;
      insert_column; remove; swap; stack_on; tabs; geometries; tests.
- [x] T2 callers: insert_tile (None -> column), unfloat, place_on_desk / Desk::add with
      Option<Side>, launcher Place::Column + Spot side Option.
- [x] T3 build, test, install.
- [x] T4 resize widths inside a row: Mod+Ctrl+=/-, Alt+scroll. Tests: row_width_resizes_inside_the_row,
      row_width_keeps_every_tile_above_the_minimum, ctrl_resizes_inside_the_row. 56 + 29 + 12 pass.

## Progress
- 2026-10-19 doc created; T0 done.
- 2026-10-19 T1-T3 done.
- 2026-10-19 T4 done (row widths), installed.

## Verification
- `cargo build --workspace`: no warnings. `cargo test --workspace`: 53 + 29 + 12 pass.
- New layout tests: side_splits_share_the_row, side_without_a_target_opens_a_column,
  folded_row_shows_a_bar_per_tile, stacked_tile_takes_the_place_and_folds_the_target_into_a_tab.
- Launcher tests updated for `Place::Column` default.
- Installed ~/.cargo/bin/dune. Not run in a live session (pending: restart dune, try `:v`, `@s`).

## Notes
- Moving a tile to the next monitor (swap past the edge) and unfloating still open a column.
- `Layout::insert(Split::Vertical)` (tests, output gone) = new column.
- `insert_on` beside a folded row opens that row.

## Next step
User restarts the session and checks `:v`, `:h`, `@s` tabs, and Alt+Ctrl+=/- or Alt+scroll on side-by-side tiles.
