# dune — waybar desktop indicator, and autostart from the config

## Objective
User request 2026-10-19: "agrega un indicador de escritorio waybar y arranca el waybar con dune, desde la
config".

## Decisions / constraints
- Indicator through the standard `ext-workspace-v1` protocol (waybar 0.15 `ext/workspaces` module, also
  other bars), not a dune-only script: one workspace group per monitor, workspaces 1-9 in each.
- State: the desktop shown is `active`; empty desktops not shown are `hidden` (waybar hides those by
  default, like niri's `hide-empty`). Clicking a desktop (`activate` + `commit`) shows it on its monitor.
- Autostart: `exec = <command>` lines in `~/.config/dune/config`, any number, run once when dune starts
  (after WAYLAND_DISPLAY / DISPLAY are set). Not re-run on reload; a change only warns.
- The user's waybar config (`~/.config/waybar/config.jsonc`) is used by niri: it stays untouched. A dune
  copy `~/.config/waybar/dune.jsonc` swaps `niri/workspaces` for `ext/workspaces` and drops niri-only
  modules; dune's config starts `waybar -c ~/.config/waybar/dune.jsonc`.
- TDD: off (no project config, no user choice). Runner: `cargo test --workspace`.
- Checks: cargo build without warnings, cargo test --workspace, nested smoke (waybar inside nested dune,
  workspace events), install.

## Tasks
- [x] T1 `src/workspaces.rs`: ext-workspace-v1 global, groups per monitor, 9 workspaces each, state
      sync after every arrange, activate on commit (`show_desk_on`).
- [x] T2 config `exec` lines, run at startup; docs in the config module comment.
- [x] T3 user setup: `~/.config/waybar/dune.jsonc`, `exec = waybar ...` in `~/.config/dune/config`.
- [x] T4 checks: build, tests, nested smoke, install.

## Acceptance criteria
- Waybar started by dune shows the desktops in use on each monitor, the shown one highlighted; it follows
  Mod+N and clicking a desktop switches to it.

## Progress
- 2026-10-19 doc created.
- 2026-10-19 T1-T4 done.
  - `src/workspaces.rs`: ext_workspace_manager_v1 global (v1); per bound manager an `Instance` caching
    groups (per monitor, `output_enter` sent once the bar has that wl_output) and desktop handles with
    the last state; `refresh_workspaces` at the end of `arrange` sends only changes + `done`. Activate is
    kept in the manager data and applied on commit with `Dune::show_desk_on` (new, in desktops.rs).
  - Config: `exec` lines (`Config.exec`, each line counts, `execs`), run at startup by `autostart` in
    main.rs (sh -c, from $HOME, reaped); a changed `exec` only warns on reload.
  - User files: `~/.config/waybar/dune.jsonc` (copy of config.jsonc: `ext/workspaces` with
    `on-click: activate`, `niri/language` dropped from the modules, `niri msg action spawn --` removed from
    on-click commands); `~/.config/dune/config`: `exec = waybar -c ~/.config/waybar/dune.jsonc -s
    ~/.config/waybar/style.css`. `config.jsonc` (niri) untouched.
  - Checks: build 0 warnings; cargo test --workspace 96 pass. Nested smoke with waybar started by `exec`
    (WAYLAND_DEBUG): group output_enter, desktop 1 active / rest hidden, Alt+2/Alt+3/Alt+1 send the right
    state changes + done; tiles leave room for the bar. No panics. Installed.
- Not verified: clicking a desktop in waybar (no pointer injection; activate+commit path untested), the
  bar's look (host is dune, no screen capture), two real monitors.

## Next step
User restarts the dune session.
