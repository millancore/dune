# dune — terminal-first tiling Wayland compositor (Smithay)

## Objective
A tiling Wayland compositor where the basic unit is a terminal tile. Typing
`@program args` in a tile runs the program *in place* (GUI windows swallow the
terminal and give it back on exit); `@program:v args` / `@program:h args` open the
program in a new tile split side-by-side (`v`) or stacked (`h`).

## Decisions / constraints
- Base: Smithay git master pinned to rev 023b78b1 (smallvil example API, `delegate_dispatch2!`).
- Terminal tiles use an external emulator (`$DUNE_TERMINAL`, default `foot`) — no embedded terminal.
- `@` is handled by dune's own PTY wrapper (`dune-shell`, src/shell.rs + src/picker.rs), independent of the user's shell; it talks to the compositor over `$DUNE_SOCKET`. (The earlier zsh integration was retired.)
- `:v` = vertical divider (side by side, vim `:vsplit`), `:h` = stacked. Easy to flip.
- Tile identity: terminals get `DUNE_TILE=<id>` (read from `/proc/<pid>/environ`); in-place `@prog` swallows by process ancestry after an IPC `swallow <tile> <shell pid>`.
- Phase 1 = nested winit backend only (runs inside an existing Wayland/X session). TTY/DRM backend is phase 2.
- TDD: off (no project config, no user choice). Checks: `cargo build`, `cargo test` (layout unit tests), nested smoke run.

## Tasks
- [x] T1 Cargo project + smallvil-based compositor skeleton (winit, xdg-shell, seat)
- [x] T2 Layout tree (split/remove/geometry/directional focus) with unit tests
- [x] T3 Tile manager: terminal spawn, DUNE_TILE/DUNE_SWALLOW matching, swallow stack, close handling, focus, borders
- [x] T4 IPC socket + `dune open` client + `dune init zsh` integration script
- [x] T5 Keybindings (new terminal, focus hjkl, close, resize, quit)
- [ ] T6 Nested smoke run (done) + README (pending)
- [x] T8 `@` tab completion (programs on PATH, :v/:h suffix menu, program's own arg completion)
- [x] T9 Keyboard layout from XKB_DEFAULT_{RULES,MODEL,LAYOUT,VARIANT,OPTIONS} (was hardcoded US; user uses us/dvorak)
- [x] T10 Shell-agnostic `@` layer (user choice 2026-10-19: PTY wrapper, not zsh-only, not compositor overlay):
  `dune` runs as the terminal's $SHELL (argv0 `dune-shell` symlink), forks the user's real shell on a PTY, passes bytes through
- [x] T11 Picker in the wrapper: `@` at an empty prompt (shell is the tty foreground pgrp, nothing typed since Enter) opens an
  in-terminal line editor with live PATH program list, `:v`/`:h` choices, file completion, Tab/arrows/Enter/Esc
- [x] T12 Compositor: spawn terminals with SHELL=wrapper; IPC `swallow <tile> <shell pid>` / `unswallow <tile>`;
  swallow by process ancestry instead of DUNE_SWALLOW env; DUNE_RUN executed by the wrapper
- [x] T13 Retire zsh integration (shell/dune.zsh, `dune init zsh`, zsh tests, line in ~/.config/zsh/.zshrc)
- [x] T14 Tests: Rust unit tests for parser/picker; pty integration test driving the wrapper with bash (and zsh)
- [x] T15 Config file ~/.config/dune/config (keyboard, terminal, modifier; env overrides). Root cause of QWERTY: user's
  wezterm is one long-lived process started before the niri env change, so dune launched from it had no XKB vars.
- [x] T16 Debug key injection over IPC (`keys` request, only with DUNE_DEBUG=1) for end-to-end tests.
- [x] T17 linux-dmabuf (GPU buffers from clients). Cause: Zed inside dune reported llvmpipe (software) while fine
  in niri; dune only offered wl_shm. DmabufState + v4 default feedback (render node via EGLDevice, needs smithay
  backend_drm), imports validated with the renderer; the winit backend now lives in `Dune::backend`.
  Evidence: alacritty in nested dune -> "dmabuf imported" logged; cargo test (60) and one e2e run pass.
- [x] T18 layer-shell (user priority 2026-10-19): bars, notifications, launchers, wallpapers; exclusive zones shrink the
  tiling area; keyboard-interactive layers (launchers) get focus; top/overlay above windows, background/bottom below
- [x] T19 X11 apps through xwayland-satellite (as niri does): started by dune on a free display number, DISPLAY passed
  to children; config `xwayland = satellite|off`
- 2026-10-19 T18: handlers/layer_shell.rs (map on the output's layer map, initial configure + arrange on commit,
  exclusive keyboard for top/overlay layers via `layer_focus`), tiles use the non-exclusive zone, pointer hit-test
  overlay/top > tiles > bottom/background, frame callbacks for layers. Checked nested: swaybg (wallpaper), fuzzel
  takes the keyboard and an injected Esc closes it, waybar with a minimal config reserves 30px (tile area y 0 -> 30).
  The user's waybar config targets HDMI-A-1/2, so it shows nothing on the nested `winit` output (expected).
- T19: dune starts `xwayland-satellite :N` on the first free display, DISPLAY passed to children, its log goes to
  $XDG_RUNTIME_DIR/dune-xwayland:N.log; needed wp_viewporter (added). Checked: xprop answers, a tkinter (X11)
  window is placed as a tile and its tile removed on close; satellite exits with dune. Satellite warns about missing
  fractional-scale, xdg-activation and primary selection (optional; later).
- Intermittent e2e root causes found: (1) the nested window takes focus on the host and the user's real typing
  leaked into test shells -> DUNE_TEST_INPUT=injected (with DUNE_DEBUG=1) ignores real input; (2) dune-term read the
  typed line before a busy shell (zsh plugins) had echoed it -> Enter is held until echo + 15ms quiet (max 300ms).
  e2e redesigned for `@` = desktop apps (test entry Exec=touch %F). One run: all checks pass (us, dvorak); unit
  tests 60 and shell_wrapper.py pass. Installed.
- Test hygiene learned: never pkill by name (may hit the user's mako/apps); notify-send reaches the host session.
- [ ] T20 Optional protocols for X11/apps: xdg-activation, primary selection, fractional scale
- [ ] T7 Real session (TTY/udev/DRM), user go-ahead 2026-10-19 after the initial commit (4d1ea3a). The real run can
  only be done by the user from a free VT (niri holds the seat here); first runs under `timeout` as a safety net.
  - [x] T7.1 Backend split: shared render elements (borders, direction bars); backend chosen at startup (inside a
    Wayland/X session -> nested winit; on a TTY -> udev; `--tty` / `--nested` to force)
  - [x] T7.2 libseat session + udev primary GPU + DRM/GBM/EGL + DrmCompositor; outputs for connected connectors,
    vblank-paced frames, dmabuf feedback from the real device
  - [x] T7.3 libinput: keyboard, relative/absolute pointer clamped to outputs, buttons, scroll; Ctrl+Alt+F1..F12 VT
    switch; pause/resume on session changes
  - [x] T7.4 Pointer cursor drawn by dune: client cursor surfaces + default xcursor theme
  - [ ] T7.5 Several monitors (the user has two: HDMI-A-1 2560x1080 at 0,0 and HDMI-A-2 1920x1080 at 311,1080 in niri)
    - [x] T7.5a Config `output.<NAME>.mode|position|off` in ~/.config/dune/config (parse + unit tests)
    - [x] T7.5b udev: mode from config, skip `off` monitors, positions from config (unset ones to the right), tests
    - [x] T7.5c One layout per output: new tiles go to the output of the target tile, else the active output (last
      one the pointer entered or a tile was focused on); focus/swap work across monitors; tiles of a monitor that
      goes away move to the first one
    - [x] T7.5d Popups and unbound layers use the right output; user's config gets the output section
    - [ ] T7.5e Real run on the two monitors (user, from tuigreet)
    - 2026-10-19 evidence: cargo build clean; cargo test --workspace 67 pass (new: config outputs, choose_mode,
      positions incl. the niri layout 0,0 / 311,1080). e2e nested: all pass except the 4 clipboard checks, which fail
      the same way with the previously installed binary (pre-existing, not from T7.5). Installed (cargo install
      --offline). Not run on hardware: niri holds the seat. Known: tiles of an unplugged monitor stay on the first
      one when it comes back; no keybinding to move a tile to another monitor yet (focus/swap by direction cross).
  - [ ] T7.6 Monitor hotplug
  - [x] T7.7 How to start it: user prefers picking it at login (greetd + tuigreet, F3 lists
    /usr/share/wayland-sessions). dist/dune-session (login shell env, XDG_SESSION_TYPE/CURRENT_DESKTOP, log to
    ~/.local/state/dune/session.log, exec dune --tty; installed to ~/.cargo/bin) + dist/dune.desktop (installed by
    the user with sudo, Exec with the absolute path). dune --tty exports WAYLAND_DISPLAY/DISPLAY/XDG_* /DUNE_SOCKET via
    dbus-update-activation-environment --systemd (tty mode only). Dry run with a fake dune: env and args correct.
  - Progress 2026-10-19: src/udev.rs (libseat session, primary GPU via udev, DRM/GBM/EGL GlesRenderer,
    DrmOutputManager + DrmScanner from smithay-drm-extras, one output per connected connector named like HDMI-A-1
    with make/model from libdisplay-info, outputs side by side, 16ms redraw timer + vblank-paced queue_frame,
    pause/resume on VT switch, dmabuf v4 feedback from the real render node, shm formats from the renderer);
    src/render.rs shared by both backends (decorations, pointer, OutputRenderElements, post_render frame callbacks);
    src/cursor.rs (xcursor theme or built-in arrow; client cursor surfaces); Backend enum in state; `--tty` /
    `--nested` (auto: nested when WAYLAND_DISPLAY/DISPLAY is set); relative pointer motion clamped to outputs;
    Ctrl+Alt+F1..F12 -> session VT switch. Tests: dune 18 (pointer clamping on two outputs, VT keys), all crates 64,
    one e2e nested run passes. NOT RUN on real hardware yet: no seatd and niri holds the logind session here, so the
    first real run is the user's, from a free VT under `timeout`.
  - Known gaps for the first real run: per-monitor tiling untested on hardware (T7.5e); monitor hotplug path untested (T7.6);
    no presentation-time; no keyboard LEDs.

## Progress / evidence
- cargo build: clean, no warnings. cargo test: 10 passed (layout + IPC codec).
- zsh -f tests/zsh_parse.zsh: all parser checks pass (fixed bug: `#` pattern needed EXTENDED_GLOB).
- Nested smoke run (inside niri): first terminal -> tile 1; `dune open --split v` -> tile 2;
  `--split h -- foot -T swallowed` -> tile 3 terminal, then foot swallowed tile 3; closing it restored the terminal;
  `--split v --cwd /etc -- vim hostname` -> tile 4, vim running in zsh in /etc with DUNE_SWALLOW=4.
- Not verified visually (host has no screencopy for grim); keybindings not exercised in the test.
- Known: SIGTERM leaves the control socket file (removed on next start).
- 2026-10-19 integration fix: shell/dune.zsh used a top-level `return` that aborted the rest of the user's zshrc outside dune;
  now wrapped in `if [[ -n $DUNE_SOCKET ]]`. `dune` installed via `cargo install --path .`. User's zsh reads
  ~/.config/zsh/.zshrc (ZDOTDIR), not ~/.zshrc — integration line appended there. E2E with real config: split + swallow OK.
- T8 done: `_dune_complete` registered for -first- (deferred to first prompt if compinit runs later).
  tests/zsh_complete.py drives a real zsh via pty: 6 typed cases + split menu pass; with completion disabled the
  @ cases and git delegation fail (test is meaningful). Real user config: `@alacrit<Tab>` -> `@alacritty`,
  `@nvim:v Carg<Tab>` -> `Cargo.`; _comps[-first-]=_dune_complete. Desktop-entry apps not included (PATH only).
- Open user question (2026-10-19): "create a layout first and then down to terminal" — answered, awaiting direction.

- T9: verified via log: no env -> English (US); LAYOUT=us VARIANT=dvorak -> English (Dvorak); invalid -> error + US fallback.
  Typing in Dvorak inside a window not checked by eye. Env vars added to ~/.config/niri/config.kdl environment block (backup: config.kdl.bak-dune); niri validate OK; niri-spawned processes receive them.

- T10-T14 (2026-10-19): cargo test 20 passed (layout, ipc, picker, escape parsing, ancestry).
  tests/shell_wrapper.py drives `dune shell` around bash and zsh (with rc files that run programs): live list, in-place run +
  swallow/unswallow, split request with Tab file completion, @ mid-line and inside running programs passes through, Esc cancel,
  DUNE_RUN typed only after startup (old binary fails this check, new passes). E2E nested with the user's real zsh config:
  foot -> dune-shell -> zsh; `dune open --split v -- foot -T swallowed` -> tile 2 swallowed, closing restores the terminal.
  Known: unswallow for programs that exit instantly is sent on the next Enter; SIGTERM leaves socket + .shell link.
  Removed: shell/dune.zsh, zsh tests, `dune init zsh`, line in ~/.config/zsh/.zshrc (backup .zshrc.bak-dune).
  Not checked by eye: picker drawing in a real foot window.

- T15/T16: cargo test 22 passed; launched without XKB env -> English (Dvorak) from the config file.
  E2E with injected keys: `@touch m1` in tile 1, `@:v`, `@touch m2` in tile 2 -> both files created (picker works in tile 2).
  User reports picker missing in the second tile: not reproduced; asked how that tile was opened.
- Open proposal (2026-10-19): user wants the terminal built into dune; recommended a separate `dune-term` client
  (alacritty_terminal + smithay-client-toolkit + cosmic-text), awaiting decision.

## Next step
User logs into Dune from tuigreet with both monitors (T7.5e); then a keybinding to move a tile to another monitor and an `autostart` config for cliphist/polkit/steam
