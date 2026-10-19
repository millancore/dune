# dune — scrolling columns (niri-like, X only), resizable tiles, terminal freeze

## Objective
User request 2026-10-19: (1) the terminal running `claude` freezes after a while; (2) unlimited tiles in X
like niri, but only in X; (3) tiles must be resizable.

## Decisions / constraints
- Layout per output becomes a strip of columns that scrolls horizontally (niri model). The Y direction
  is not infinite: tiles stacked in a column share the output's height.
- `:v` / Left / Right = new column next to the target; `:h` / Up / Down = same column, above/below it.
- Column width is a share of the output width (default 1/2; presets 1/3, 1/2, 2/3, full). Heights in a
  column are weights.
- The view scrolls just enough to show the focused column whole. Columns that end up fully off the output
  are unmapped (no bleeding onto a neighbouring monitor); partly visible ones stay drawn.
- Layout API kept (`insert`, `insert_on`, `remove`, `swap`, `geometries`, `neighbor`) so callers barely change.
- TDD: off (no project config, no user choice). Runner: `cargo test --workspace`. Checks: build, unit tests,
  nested smoke run, `cargo install`.

## Tasks
- [x] T1 dune's log reaches session.log: tracing wrote to stdout, the session script only keeps stderr
  (now stderr, no ANSI colours when not a terminal)
- [x] T2 layout.rs: columns model, scroll offset, widths/heights, resize ops, unit tests
- [x] T3 Compositor: view follows focus, unmap off-screen tiles, hit testing/borders only for shown tiles
- [x] T4 Resize: Mod+=/- column width, Mod+Shift+=/- tile height, Mod+R width presets, Mod+F full width,
  Mod+right-drag resizes with the mouse (right/down grows)
- [x] T7 Tabs (user addition 2026-10-19: "if one grows a lot over the other, the smaller becomes a tab"):
  a tile squeezed below 15% of its column folds into a 24px bar; focusing/clicking it opens it and folds
  the largest open tile; shrinking a tile (Mod+Shift+-) opens every tab of the column evenly. Bars are
  plain coloured strips (no title text: the compositor has no font renderer yet).
- [x] T5 Checks: cargo test, nested smoke run, install
- [x] T8 Tab titles (user go-ahead 2026-10-19): the font module moved from dune-term to dune-common
  (`dune_common::font`, swash + fontdb); src/tab_title.rs draws the window title (else app id, else
  `tile N`) on the bar, cut with `…`, font = `font_family` from the config (new key in dune's Config),
  cached per tile until title/size change. The nested backend now renders through `output_elements`
  like the TTY one (it used to draw only the solid decorations).
- [x] T9 Super+scroll resizes the column width of the tile under the pointer, Super+Shift+scroll its
  height (user request 2026-10-19); always Super, not the bindings modifier; a notch = one 5% step, up grows.
- [ ] T6 Freeze: root cause not found yet (see Evidence); needs the next freeze's log / user details

## Evidence
- Freeze: hypothesis "dune-shell wrapper deadlocks with blocking writes both ways" tested and REJECTED:
  scratch repro (inner program not reading stdin while writing 3 MB, 200 KB fed in) never blocked; the kernel
  drops/queues pty input instead of blocking the master writer.
- dune-term frame/buffer handling and clipboard threads read: no deadlock found.
- session.log only had dune-term's broken-pipe line: dune's own tracing goes to stdout (lost under greetd).

- 2026-10-19 checks: cargo build clean; cargo test --workspace 72 pass (layout 14, incl. scrolling, presets,
  heights, tabs). e2e_nested.py: all pass except the clipboard checks (pre-existing, same as before).
  Nested smoke (scratch script, injected keys, debug log of arranged geometries): 4 columns -> strip 2x the
  output, view shows columns 3-4; Alt+h x3 slides back to column 1; Alt+= / Alt+R / Alt+F widen 618 -> 680
  -> 825 -> 1241px. Tabs: `dune open --split h` stacks; 8x Alt+Shift+= -> tile 1 folds into a 24px bar;
  Alt+k opens it and folds tile 2; Alt+Shift+- -> 487/487. Installed (cargo install --offline --path .).
- Not checked: Mod+right-drag (no pointer injection in the harness); real TTY session; partly visible
  columns bleed onto a monitor placed right next to the output (user's monitors are stacked, unaffected).
- Known: Alt+R / Alt+F (modifier = alt) shadow readline's Alt+r / Alt+f in shells.
- T8/T9 checks: cargo test --workspace 74 pass (tab bar drawing, scroll step); e2e_nested.py: all checks
  passed (us, dvorak), clipboard included this run. Nested screenshot (grim of the host output, deleted
  after viewing): the folded tile shows a bar with "millancore@hx90:~" above the open terminal.
  Installed dune and dune-term. Super+scroll not exercised: niri owns Super while nested and the harness
  cannot inject axis events; for the TTY session.

## Next step
User tries it; for T6 send ~/.local/state/dune/session.log after the next freeze plus whether only that
terminal or the whole screen froze. Try Super+scroll / Super+Shift+scroll and Mod+right-drag on the TTY session.
