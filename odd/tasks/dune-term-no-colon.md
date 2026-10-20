# dune: drop the `:` picker from terminals

## Objective
The compositor launcher (super+e) is the way to start apps. Typing `:` at an empty
prompt in a terminal tile must no longer open the picker; it goes to the shell.

## Scope
- `dune-common/src/at.rs`: no `:` trigger, no picker state, no `apps` source.
- `dune-term`: no picker drawing (render.rs), no picker handling (app.rs).
- `src/shell.rs` (dune-shell wrapper): no picker drawing.
- Keep: `picker.rs` logic (used by the launcher), `cmd:v`/`cmd:h` split commands,
  direction indicator, `DUNE_RUN`, swallow.

## Checks
TDD: off (no project/session config). Runner: `cargo test --workspace`, `cargo build`.

## Tasks
- [x] T1 Remove the trigger and picker from `AtLayer` (+ tests)
- [x] T2 Remove picker drawing/handling in dune-term and dune-shell
- [x] T3 Build and test the workspace

## Progress
Started 2026-10-19. Done 2026-10-19.

## Verification
- `cargo build --workspace`: clean, no warnings.
- `cargo test --workspace`: 49 + 29 + 12 passed, 0 failed.
- New test `colon_goes_to_the_shell` (at.rs): `:` at an empty prompt reaches the shell.
- Installed `dune` and `dune-term` to ~/.cargo/bin; needs a session restart (pending, manual).

## Notes
- Removed: `AtLayer` picker/apps, dune-term picker drawing (render.rs) and paste-into-picker,
  dune-shell escape-code picker, `picker::HINT`, `palette::foreground` (now unused).
- Kept `picker::TRIGGER`: `parse_split_command` still rejects `:firefox:v` lines.
- Kept the swallow-ending on Enter (builtins run in place via `DUNE_RUN`).

## Next step
Restart the dune session and check `:` types normally in a terminal tile.
