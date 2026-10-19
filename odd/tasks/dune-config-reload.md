# dune config hot reload

File: `odd/tasks/dune-config-reload.md` · Engram mirror: `odd/dune-config-reload/tasks`

## Objective
Saving `~/.config/dune/config` applies it to the running session, like niri, without logging out.

## Problem / why
The config is read once in `Dune::new`. Changing the keyboard layout (dvorak) or the mouse needs a new session.

## Scope
- Watch the config file (poll its mtime every 500 ms, as niri does; editors that replace the file still work).
- Apply live: keyboard (`keyboard_*`), `modifier`, `direction_*`, `terminal`, `input.*` on all current
  mice/touchpads, `font_family` of tab titles, `output.*.position`.
- Not live (logged as "restart dune to apply"): `output.*.mode`, `output.*.off`, `xwayland`.
- Out of scope: dune-term reloading its own font settings.

## Constraints
- No new dependencies (calloop timer, std mtime).
- Environment variables still win over the file.
- A broken keyboard layout on reload keeps the current one.

## TDD / checks
- TDD: off (no project config, no user choice). Runner: `cargo test --workspace`.
- Checks: cargo build without warnings, cargo test --workspace, nested run editing the config live, install.

## Tasks
- [x] T1 config: `Config::restart_needed(&old)` listing settings that cannot change live; unit test.
- [x] T2 watcher: mtime poll timer started for both backends; reload on change.
- [x] T3 apply: keyboard, tab titles, output positions, libinput devices (udev keeps the device list).
- [x] T4 checks: build, tests, nested live edit, install.

## Acceptance criteria
- Changing `keyboard_variant` and saving changes the keys typed in the running session.
- Changing `input.left_handed` applies to the mouse without replugging it.
- A setting that needs a restart is logged as such, nothing breaks.

## Progress
- 2026-10-19 doc created.
- 2026-10-19 T1-T4 done. `src/reload.rs` (watch + `Dune::reload_config`), `Config::xkb`/`restart_needed`
  in `src/config.rs`, udev keeps `input_devices` and `configure_input_devices`, `place_outputs` pub.
- Checks: cargo build no warnings; cargo test --workspace 30+32+14 pass (incl. config::tests::restart_needed).
  Nested live run with temp XDG_CONFIG_HOME: append (dvorak) -> "config changed, applying it"; bad layout
  -> "keeping the current one"; xwayland/output mode -> "restart dune to apply it"; replace-by-rename
  detected. Installed (cargo install --offline --path .).
- Not verified: typed keys actually switching to dvorak and left-handed on a real TTY session (needs the user).

## Next step
User logs into a new session once (old dune binary has no watcher), then edits the config live.
