# dune-term — dune's own terminal emulator

## Objective
The basic unit of dune is the terminal, so dune ships its own: `dune-term`, a separate Wayland
client in the same Cargo workspace (user decision 2026-10-19: separate program, not inside the
compositor process, so a terminal crash/hang only takes down its tile).

## Libraries (agreed)
- `alacritty_terminal` — VT parsing, grid, scrollback, selection, PTY
- `smithay-client-toolkit` — Wayland window, seat/keyboard (keymap comes from dune), clipboard
- `swash` + `fontdb` — glyph rasterization and font lookup (cosmic-text not needed for a monospace grid)
- Rendering: CPU into wl_shm buffers first; wgpu later (phase 4)

## Constraints
- Workspace member `dune-term/`; the compositor crate stays at the root.
- Phase 1 keeps the `@` layer as the existing `dune-shell` PTY wrapper (works in any terminal).
- TDD: off. Checks: cargo build (no warnings), cargo test, end-to-end inside nested dune with
  injected keys (DUNE_DEBUG=1 `keys` IPC), observing side effects (files created by typed commands).
- ~400 changed lines per task is a planning heuristic only.

## Tasks
### Phase 1 — usable terminal
- [x] P1.1 Workspace + crate scaffold; sctk window with shm buffer (solid background), close/resize
- [x] P1.2 PTY + alacritty_terminal Term on a thread; wakeups redraw; child exit closes the window
- [x] P1.3 Font loading (fontconfig monospace via fontdb) + swash glyph cache; draw grid: fg/bg colors (named, indexed, rgb), inverse, cursor
- [x] P1.4 Keyboard → bytes (text, Enter, Backspace, Tab, Esc, arrows incl. app-cursor mode, Home/End/PgUp/PgDn/Del/Ins, F1–F12, Ctrl, Alt); key repeat
- [x] P1.5 Resize → grid + PTY winsize
- [x] P1.6 dune uses dune-term as default terminal; e2e test with injected keys
### Phase 2 — comfort
- [x] P2.1 Scrollback (wheel, Shift+PgUp/PgDn)
- [x] P2.2 Mouse selection + clipboard (wl_data_device), paste (Ctrl+Shift+V), bracketed paste
- [x] P2.4 Font family/size from config + zoom (Ctrl+= / Ctrl+- / Ctrl+0). Cause of "small text": 16px Inconsolata vs
  the user's wezterm JetBrains Mono 17pt; both outputs are scale 1, so HiDPI scale is deferred (P2.5)
- [ ] P2.5 HiDPI buffer/fractional scale (only when an output uses scale > 1)
- [x] P2.3 Bold/italic faces, fallback fonts for missing glyphs, wide chars/emoji
### Phase 3 — native @
- [x] P3.1 Extract shared code into workspace crate `dune-common`: picker, IPC protocol (Request/Split/send), prompt
  interception state machine (typed-since-Enter, terminal-reply escapes); dune keeps behaviour, tests still pass
- [x] P3.2 dune-term started as `dune-shell` (by dune) runs DUNE_USER_SHELL directly and detects the prompt itself
  (tcgetpgrp(pty) == shell pid); outside dune (no DUNE_SOCKET) no @ handling
- [x] P3.3 Native picker: drawn by dune-term as an overlay at the cursor (input line + list + hint); keys via the shared
  picker; run in place (swallow IPC + type into PTY), splits (open IPC), unswallow on prompt return/Enter, DUNE_RUN
- [x] P3.4 Tests: unit (interception, overlay placement/render); e2e nested passes with no dune-shell process in tiles
### Phase 3b — new syntax (user request 2026-10-19)
`@` is only for desktop-entry applications; the `:v`/`:h` split suffix works on any command without `@`
(`vim:h test.txt`). Decisions: `@app` still replaces the tile (swallow), `@app:v` opens it in a new tile, extra words
are passed to the app (%f/%u/%F/%U, else appended); bare `:v`/`:h` opens an empty terminal split; the no-@ form needs
dune-term (it reads the line from its own screen); other terminals via dune-shell keep only the `@` form.
- [x] P3b.1 Desktop entries in dune-common: XDG dirs (+ flatpak exports), Name/Exec/NoDisplay/Hidden/Type, field codes
- [x] P3b.2 Picker lists desktop apps (match name words/id/exec, show Name, insert id) and resolves @app to its Exec
- [x] P3b.3 dune-term: `cmd:v args` / `cmd:h args` at the prompt → Ctrl-U to the shell + open request (line read from
  the grid between where typing started and the cursor)
- [x] P3b.4 Tests: unit (desktop parsing, field codes, matching, split-line parsing) + e2e (`touch:v file`, `@app`)
### Phase 3c — direction indicator (user request 2026-10-19)
dune keeps a per-tile direction indicator changed with configurable keys (default Super+arrows; pressing the same
arrow again clears it), drawn as a colored bar on that side of the tile border. On Enter at the prompt, dune-term
asks dune for the indicator: set → the command line (or `@app`) opens in a new tile on that side, line cleared in the
shell; unset → runs in place as usual. One-shot: the indicator resets once used. `:v`/`:h` stay as a typed shortcut.
- [x] P3c.1 Protocol: `Side` (left/right/up/down) for opens; `direction <tile>` request answered by dune (and reset)
- [x] P3c.2 dune: per-tile indicator, key specs from config (`direction_left|right|up|down = super+Left`...),
  border bar, layout insert before/after, answer the request
- [x] P3c.3 dune-term: on Enter at the prompt query the indicator; split the typed line / `@app` to that side
- [x] P3c.4 Tests: unit (layout before, key specs, protocol) + one e2e run (indicator + Enter -> new tile)
- [x] P3b.5 Investigate intermittent e2e failures (keys not triggering @ in ~3/7 runs); one run at a time
### Phase 3d — `:` trigger (user request 2026-10-19)
`@` did not open the picker in the TTY session (cause not found: no compositor binding takes Shift+2 and nested
e2e opens it). User chose `:` as the trigger instead; bare `:v` / `:h` (picker input `v`/`h`) still open an empty split.
- [x] P3d.1 `picker::TRIGGER = ':'` used by at.rs, dune-term + dune-shell drawing, debug trace; picker resolves bare
  `v`/`h` to an empty split; parse_split_command rejects `:app...` lines but keeps `:v`
- [x] P3d.2 Tests updated (unit, e2e_nested, shell_wrapper); installed to ~/.cargo/bin
- [ ] P3d.3 User checks `:` in the TTY session (if it still fails there, the cause is TTY-specific, not the key)

### Phase 4 — GPU
- [ ] P4.1 wgpu renderer

## Progress / evidence
- 2026-10-19 phase 1 done. dune-term/src: app.rs (sctk window, PTY thread via alacritty EventLoop, frame-paced redraw),
  font.rs (fc-match then fontdb; swash alpha glyph cache), render.rs (cells, colors, inverse, underline, strikeout, cursor
  shapes, unfocused hollow cursor), keys.rs (xterm encoding), palette.rs (16 base + 256 + truecolor, OSC overrides).
- cargo test -p dune-term: 7 passed (keys, palette, font). Workspace build: no warnings.
- Visual check: ran in niri with a color demo; bold/colors/inverse/underline/256/truecolor/UTF-8 and cursor render
  correctly. (Screenshot captured the whole desktop by mistake; deleted, contents not used. Don't screenshot the host again.)
- dune: default terminal is dune-term, looked up next to the dune binary (niri's PATH lacks ~/.cargo/bin).
- tests/e2e_nested.py: nested dune with dune-term tiles, injected keys, US and Dvorak configs: @ in tile 1, @:v, @ in
  tile 2, Alt+h back to resized tile 1 and typing -> all pass. Installed both binaries; user config terminal = dune-term.
- Not done in phase 1: scrollback UI, selection/clipboard, bold/italic faces, fallback fonts, HiDPI.

- 2026-10-19 phase 2: font.rs (4 faces via fontconfig, synthetic bold/italic with swash embolden/skew,
  per-char fallback via `fc-match family:charset=XXXX`, color emoji box-scaled to 2 cells); config.rs (font_family,
  font_size in points from ~/.config/dune/config); keys.rs (Shortcut: Ctrl+Shift+C/V, Shift+PgUp/PgDn, zoom; SGR mouse);
  app.rs (wheel: scrollback / alternate-scroll arrows / SGR wheel reports; click/double/triple selection; middle-click
  paste; clipboard source + paste on a thread; bracketed paste; OSC 52 store/load); render.rs (styles, color glyphs,
  selection highlight).
- cargo test -p dune-term: 15 passed (incl. offscreen render tests: bold/italic differ, emoji has color, selection bg).
  e2e_nested.py (us + dvorak): previous checks + OSC 52 copy -> Ctrl+Shift+V into `cat` -> file; wl-paste in the
  nested session reads the same clipboard. All pass. User config: font_family = JetBrains Mono, font_size = 17.
- Not verified end to end: mouse selection/drag, wheel scrolling, Shift+PgUp (no pointer injection; logic only).

- 2026-10-19 phase 3: new workspace crate dune-common: protocol.rs (Split, TileId, Request, send, socket_path),
  picker.rs (moved from dune), at.rs (AtLayer: @ interception, picker, swallow/unswallow, splits, DUNE_RUN after settle;
  send injectable for tests; wrapped_user_shell()). dune: layout/ipc use it; src/shell.rs (dune-shell, for other
  terminals) is now only I/O + escape-code drawing over AtLayer. dune-term: when $SHELL is dune-shell, starts
  DUNE_USER_SHELL itself, prompt = tcgetpgrp(pty dup) == shell pid, 200ms timer ticks the layer, native overlay
  (render.rs draw_picker/panel_place: @ line with own cursor + panel below/above + hint).
- Tests: dune 11, dune-common 18 (AtLayer behaviour incl. swallow/unswallow, split, DUNE_RUN settle, terminal replies),
  dune-term 17 (panel placement, overlay drawn/absent). shell_wrapper.py (bash, zsh) passes after the refactor.
  e2e_nested.py (us + dvorak): no dune-shell under dune-term tiles; @ in tiles 1 and 2; @dune-term:v -> new tile
  swallowed by the GUI program, Alt+q restores the terminal; clipboard checks. All pass. Both binaries installed.
- Not checked by eye: the native panel inside a real dune window (offscreen render test only).

- 2026-10-19 phase 3b: dune-common desktop.rs (XDG dirs incl. flatpak, [Desktop Entry] only, localized Name,
  NoDisplay/Hidden, field codes, rank by id/name word/program); picker lists apps (label "Name  (id)", inserts id),
  resolves @app to Exec + args; AtLayer apps injectable; parse_split_command + AtLayer::split_command; dune-term
  reads the typed line from its grid (line_start recorded at the first key after the prompt, up to the cursor),
  Ctrl-U + open request. DUNE_DEBUG=1 traces the @ layer in dune-term on stderr.
- Tests: dune-common 26 pass. e2e adds `touch:v file` and `@dunetes` (desktop entry via XDG_DATA_DIRS; using
  XDG_DATA_HOME made atuin start slowly). e2e is intermittent (~3/7 runs fail: typed keys not triggering @); cause
  not found yet. A loop re-running e2e until failure was stopped by the user: run it once at a time.

- 2026-10-19 phase 3c: protocol `Side` (l/r/u/d; v/h still accepted), `direction <tile>` answered by dune and
  cleared (first request with a reply; ipc writes the answer); layout `insert_on` (before/after); dune per-tile
  `directions`, `toggle_direction`, config `direction_left|right|up|down` key specs (default super+arrows, exact
  modifiers, invalid spec -> no binding + warning), orange bar on that side of the tile (winit.rs); AtLayer
  `take_direction` (injectable) used by run_line (@app) and by dune-term on Enter with a typed line (open_beside +
  Ctrl-U); empty lines don't consume the indicator.
- Tests: dune 14, dune-common 29, dune-term 17 (no warnings). shell_wrapper.py updated (open code `r`, own desktop
  entry, XDG_DATA_DIRS isolated) -> passes. e2e_nested.py adds Super+Right + Enter -> new tile, then next command in
  place (one-shot) -> one run: all checks passed (us, dvorak). Installed both binaries.
- P3b.5 (intermittent e2e) still open: this single run passed.
- Note: nested in niri, Super+arrows are grabbed by niri; configure e.g. direction_right = ctrl+alt+Right to try there.

- P3d (2026-10-19): cargo test --workspace 28+32+14 pass; shell_wrapper.py all pass (bash, zsh); e2e_nested 3 runs:
  2 all pass, 1 dvorak run failed (desktop entry + clipboard, known intermittent P3b.5).

## Next step
User tries `:` in the TTY session (P3d.3); user tries the indicator; P3b.5 if failures show up; then phase 4 or dune README/tty backend.
