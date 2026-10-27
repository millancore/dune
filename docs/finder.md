# dune-finder

One panel, several modes. Type a prefix to switch modes; Backspace with nothing typed
goes back; `Mod+Tab` / `Mod+Shift+Tab` cycle through every mode.

```text
  ┌──────────────────────────────────────────────┐
  │ ▸ fire_                                      │
  ├──────────────────────────────────────────────┤
  │ [ic] Firefox                                 │
  │ [ic] Firewall Configuration                  │
  │ ...                                          │
  ├──────────────────────────────────────────────┤
  │ Enter open · Ctrl+T tab · :d2 desktop 2      │
  │ /  windows · #  files · >  jump              │
  └──────────────────────────────────────────────┘
```

```text
                         type first in apps
                 ┌──────────────┼───────────────┐
                "/"            "#"             ">"
                 ▼              ▼               ▼
  ┌──────┐   ┌─────────┐   ┌─────────┐   "/"  ┌─────────┐
  │ apps │   │ windows │   │  files  │ ◄───── │  jumps  │
  └──────┘   └─────────┘   └─────────┘        └─────────┘
      ▲            │             │                 │
      └────────────┴─────────────┴─────────────────┘
              Backspace with nothing typed

  Mod+Tab:  apps → windows → files → jumps → system → apps
```

| Mode | Open with | What it does |
|---|---|---|
| **apps** | `Mod+d` | fuzzy-search desktop apps. Pick where it opens before Enter: a side of the focused tile, floating, as a tab (`Ctrl+T` / `:t`), another monitor or desktop (`firefox :d2`). Console programs get a terminal. |
| **windows** | `/` in apps, or the `switcher` key | every window on every monitor, desktop, tab and dock, most recent first; Enter right away goes back to the previous window. `:d2` / `:m1` filter by desktop / monitor. `:l` lists the windows of locked desktops. |
| **browser tabs** | in windows | a Chromium browser started with `--remote-debugging-port=9222` lists its tabs; Enter brings the tab forward in its window. |
| **files** | `#` in apps | browse from home: Enter goes into a folder or opens a file (`xdg-open`); `~`, `/` and `name/` work as in a shell; `Mod+Enter` opens `file_manager` (yazi, set in `finder.toml`), `Ctrl+Enter` a terminal there. |
| **jumps** | `>` in apps | folders by frecency, like zoxide's `z` (parent folders in home included). Enter opens a terminal there, `Mod+Enter` the file manager, `/` goes into the folder in files mode. |
| **tile options** | `Mod+/` | private, float, center, full width, to tab, send to desktop #, send to dock, close, each with its key. |
| **system** | `@` in apps, or `Mod+Tab` from the last mode | power off, reboot, log out; **Stay awake** holds off idle (no lock, screen off nor suspend from swayidle) until chosen again, its row says `on` meanwhile. |
| **empty container** | `Mod+Return` | Enter right away puts a terminal there; anything chosen or typed goes there instead (`/` then a window moves it in). |

A click chooses a row, the wheel scrolls, a click outside closes the panel.
