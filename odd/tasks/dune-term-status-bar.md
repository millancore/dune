# dune-term: status bar (path + git branch) and mouse selection to primary

## Objective
dune-term shows a one-line bar at the bottom of the window with the working
directory of the program in the terminal and, inside a git repository, its branch.
Selecting text with the mouse copies it to the primary selection; middle click pastes it.

## Scope
- `dune-term/src/status.rs` (new): read the cwd of the PTY's foreground process
  (`/proc/<pgrp>/cwd`), find the branch from `.git/HEAD` (plain repos, worktrees and
  submodules via `gitdir:`, detached HEAD as a short hash). No `git` process.
- `dune-term/src/render.rs`: draw the bar; grid leaves room for it.
- `dune-term/src/app.rs`: refresh on the 200ms tick, redraw only on change;
  primary selection source/device, set on left-button release, pasted on middle click
  (clipboard fallback when the compositor lacks the protocol).
- `dune-term/src/config.rs`: `status_bar = true|false` (default true).
- `src/handlers/mod.rs`, `src/state.rs`: compositor serves `zwp_primary_selection_v1`.

## Checks
TDD: off (no project/session config). Runner: `cargo test --workspace`, `cargo build --workspace`.

## Tasks
- [x] T1 `status.rs`: cwd + branch reading, path shortening (+ tests)
- [x] T2 Bar drawing, grid sizing and `status_bar` setting (+ tests)
- [x] T3 Wire into app.rs, build and test the workspace
- [x] T4 Primary selection in the compositor (added mid-feature by user request)
- [x] T5 dune-term: selection → primary, middle click pastes primary

## Progress
Started 2026-10-20. Done 2026-10-20.

## Verification
- `cargo build --workspace`: clean, no warnings.
- `cargo test --workspace`: 68 + 32 + 18 passed, 0 failed.
- New tests: status.rs (branch from subdir, detached HEAD, worktree, no repo, `~`, cut),
  render.rs `status_bar_leaves_room_and_is_drawn_at_the_bottom`, config `status_bar`.
- Not checked live: bar and primary paste in a real session (needs install + session restart).

## Notes
- Drag/double/triple-click selection and Shift+drag in mouse-reporting programs already existed;
  what was missing was copy-on-select and middle-click paste of the selection.
- Branch icon: Powerline U+E0A0 if a font has it, else `⎇`, else none.

## Next step
Install and restart the dune session; check the bar follows `cd` and git checkouts,
and select + middle click pastes.
