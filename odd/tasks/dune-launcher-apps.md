# dune — launcher with icons and a fuzzel/rofi-style app list

## Objective
User request 2026-10-20: the launcher (Mod+d) should show icons and list apps the way fuzzel / rofi do;
today it shows a mix of desktop entries.

## Problem
- Rows read `Name  (id)`, so ids like `appimagekit_2336…-OpenPLC_Editor` or `chrome-aghbi…-Profile_1` show.
- Entries meant for other desktops (`OnlyShowIn` / `NotShowIn`) and with a missing `TryExec` are listed.
- Duplicates (several AppImage copies of Tinkerwell / Cursor) show as separate rows.
- Matching is prefix-only on id / name words / program; no fuzzy, no keywords, no frequency.
- No icons.

## Decisions / constraints
- Rows show the app's icon and its Name only (like fuzzel); the id stays the internal key.
- Filtering like fuzzel: Type=Application, not NoDisplay/Hidden, OnlyShowIn/NotShowIn checked against
  `XDG_CURRENT_DESKTOP`, TryExec must exist; same Name + same Exec shown once.
- Matching: name prefix > name word prefix > name substring > fuzzy subsequence of the name > match in
  GenericName / Keywords / program. Ties: launches (most used first), then name.
- Launch counts kept in `$XDG_CACHE_HOME/dune/launcher` (`id count` lines), like fuzzel's cache. Empty
  query: most used first, then alphabetical.
- Icons: freedesktop icon theme lookup (theme from config `icon_theme`, else GTK settings.ini, else
  hicolor; Inherits followed; `/usr/share/pixmaps`; absolute `Icon=` paths). SVG and PNG drawn with
  `resvg` (new dependency of the dune crate only). Decoded lazily per visible row, cached per session.
- TDD: off (no project config, no user choice). Runner: `cargo test --workspace`.
- Checks: cargo build without warnings, cargo test --workspace, nested smoke run if possible.

## Tasks
- [x] T1 `dune-common/src/desktop.rs`: App gets `icon`, `generic_name`, `keywords`; OnlyShowIn/NotShowIn,
      TryExec filters; dedupe by Name+Exec; fuzzy `rank`. Tests.
- [x] T2 `dune-common/src/picker.rs`: candidates labelled by Name, carry the icon; ordering with launch
      counts; launch-count cache read/write. Tests.
- [x] T3 `src/icons.rs`: icon theme lookup + resvg rasterizing to ARGB at a given size. Tests.
- [x] T4 `src/launcher.rs`: draw icons in the rows, record launches. Tests.
- [x] T5 checks: build, tests, panel rendered with real apps/icons. Nested smoke run: not done.

## Acceptance criteria
- Launcher rows show icon + app name, no ids.
- Apps for other desktops and broken TryExec are gone; duplicates show once.
- Typing `ff` finds Firefox (fuzzy), apps used more often come first.

## Progress
- 2026-10-20 doc created after exploring desktop.rs, picker.rs, launcher.rs.

- 2026-10-20 T1-T5 done. `App` gains icon / generic_name / keywords; parse filters OnlyShowIn/NotShowIn
  (XDG_CURRENT_DESKTOP), TryExec, absolute Exec paths that no longer exist (removed AppImages); dedupe
  Name+Exec. `rank` 0 name prefix, 1 word prefix, 2 substring, 3 subsequence, 4 id/generic/keywords/program.
  `desktop::launches` / `record_launch` (`count id` lines). Picker: label = Name, `Candidate.icon`,
  `browser(.., uses)`, ties by uses, `chosen()`. `src/icons.rs`: theme chain (config `icon_theme`, else
  gsettings, else GTK settings.ini, else hicolor; Inherits BFS, hicolor last), best-size dir, pixmaps,
  absolute paths; resvg draws SVG/PNG; cached by (name, size). Launcher: icon column per row,
  `Step::Open.app` recorded by `Dune::launcher_key`; reload recreates the launcher when font/icon theme change.
  Rationale: tests must not write the user's cache, so recording lives in Dune, not Launcher::feed.
- Checks: cargo build 0 warnings; clippy clean on touched files (pre-existing warnings elsewhere);
  cargo test --workspace 67 + 32 + 12 pass. Panel rendered to PNG with this machine's 194 apps: icons
  from Papirus-Dark-Maia chain show; only "Yazi File explorer" has no Icon. Typing `fire` → Firefox first.
- Known behaviour: literal substrings outrank fuzzy matches (`ff` lists KDiff3/LibreOffice before Firefox),
  as fuzzel does; launches lift an app within its match class.
- 2026-10-20 installed with `cargo install --path . --locked` (with key repeat change). Not verified: real session smoke.

## Next step
User restarts the dune session and checks the launcher.
