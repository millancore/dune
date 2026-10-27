# Terminals

Terminals open where you work, and console programs get one of their own:

- `Mod+v` / `Mod+s` / the `terminal_tab` key open a terminal in the focused terminal's folder.
- A console program chosen in dune-finder, or `dune open COMMAND`, gets a new terminal
  with the command typed in once your shell has started (its rc files run first). dune's
  shell layer does the typing; terminals that ignore `$SHELL` (`wezterm start`) are given it
  as their program.
- The folders terminals work in feed dune-finder's jump list.

```text
  ┌──────────────────────┐            ┌──────────────────────┬──────────────────────┐
  │ ~/projects/dune $ _  │   Mod+v    │ ~/projects/dune $    │ ~/projects/dune $ _  │
  │                      │  ───────►  │                      │                      │
  └──────────────────────┘            └──────────────────────┴──────────────────────┘
```

**`dune-open COMMAND`** runs a program in the terminal's own tile: its windows cover the
terminal, which shows again when they close. Outside a terminal of dune it just runs
the command.

```text
  ┌───────────────┐  $ dune-open mpv video.mkv  ┌───────────────┐   mpv exits   ┌───────────────┐
  │ $ _           │  ─────────────────────────► │   mpv window  │  ──────────►  │ $ _           │
  └───────────────┘                             └───────────────┘               └───────────────┘
```

A shell alias makes it a habit: `alias o=dune-open`, then `o mpv video.mkv`.
