```text
              ╔══════════════════════════════════╗
        ┌ ─ ─ ║ ┌──────┐┌──────┐┌──────┐┌──────┐ ║ ─ ─ ┐
        │     ║ │ █▀▀▄ ││ █  █ ││ █▄ █ ││ █▀▀▀ │ ║     │
         ‹2   ║ │ █  █ ││ █  █ ││ █▀▄█ ││ █▀▀  │ ║   3›
        │     ║ │ █▄▄▀ ││ ▀▄▄▀ ││ █ ▀█ ││ █▄▄▄ │ ║     │
        └ ─ ─ ║ └──────┘└──────┘└──────┘└──────┘ ║ ─ ─ ┘
              ╚══════════════════════════════════╝

    ◄  an endless strip of columns, one monitor at a time  ►
```

**dune** is a tiling Wayland compositor built on [Smithay](https://github.com/Smithay/smithay).
Its windows live on an endless horizontal strip of columns, as in [niri](https://github.com/YaLTeR/niri),
with tabs, folding, floats, a scratchpad and nine desktops per monitor on top. It comes with
**dune-finder**, one panel that is the launcher, the window switcher, a file browser, a
folder jumper, the tile menu and the power menu.

---

## Contents

- [Install](#install)
- [Running](#running)
- [The strip](#the-strip)
- [Columns, rows and tiles](#columns-rows-and-tiles)
- [Tabs](#tabs)
- [Folding (the accordion)](#folding-the-accordion)
- [Fullscreen](#fullscreen)
- [Floating tiles and the scratchpad](#floating-tiles-and-the-scratchpad)
- [Desktops and monitors](#desktops-and-monitors)
- [Terminals](#terminals)
- [dune-finder](#dune-finder)
- [Mouse](#mouse)
- [Privacy: private tiles and locked desktops](#privacy-private-tiles-and-locked-desktops)
- [Screenshots and recordings](#screenshots-and-recordings)
- [Session: lock, idle, night light, X11](#session-lock-idle-night-light-x11)
- [Bars (waybar)](#bars-waybar)
- [Command line](#command-line)
- [Configuration](#configuration)
- [Default keys](#default-keys)
- [Development](#development)

---

## Install

```sh
make install          # dune, dune-open, dune-finder and dune-session into ~/.cargo/bin,
                      # plus the portal settings into ~/.config (no root needed)
make install-session  # the "Dune" entry of the login screen (asks for sudo)
```

Run `make` as your own user, not with sudo: the paths come from `$HOME`.

Optional tools dune works with:

| Tool | For |
|---|---|
| `wezterm`, `foot`, `alacritty`, ... | terminal tiles (`terminal` in `dune.toml`) |
| `grim`, `slurp`, `wf-recorder`, `wl-clipboard`, `libnotify` | screenshots and recordings |
| `swaylock`, `swayidle` | screen lock and idle timeouts |
| `xwayland-satellite` | X11 apps |
| `xdg-desktop-portal-wlr` | screen sharing in browsers and video calls |
| `waybar`, `mako`, `wlsunset` | bar, notifications, night light |
| `pactl`, `playerctl` | the speaker mark on tiles that play sound |
| `yazi` | the terminal file manager dune-finder opens (`file_manager` in `finder.toml`) |

A running dune keeps the old binary until the session is restarted.

## Running

```sh
dune            # nested in a window inside another session, or on a TTY when there is none
dune --tty      # a real session: DRM/GBM, libinput, libseat
dune --nested   # always nested (winit)
dune-session    # what the login screen runs: a login shell first, log in
                # ~/.local/state/dune/session.log
```

---

## The strip

Every monitor shows a window onto a strip of columns that never ends. Columns open to
the right of the focused one and the view slides to show whatever takes focus, whole.

```text
              ┌───────────────── monitor ─────────────────┐
   ┌────────┐ │┌──────────────┐┌─────────────┐┌──────────┐│┌──────────┐ ┌────────┐
   │        │ ││              ││             ││          │││          │ │        │
   │   A    │ ││      B       ││   C (focus) ││    D     │││    E     │ │   F    │
   │        │ ││              ││             ││          │││          │ │        │
   └────────┘ │└──────────────┘└─────────────┘└──────────┘│└──────────┘ └────────┘
              └───────────────────────────────────────────┘
        ‹1                                                   2›
     ◄── off the view                         off the view ──►
```

- **Focus** `Mod+h/j/k/l` (or the arrows) moves focus; the view follows.
- **Swap** `Mod+Shift+direction` swaps the tile with its neighbour. With nothing that way
  on this monitor it moves to the next monitor.
- **Edge marks** `‹1` and `2›` at the middle of the screen edges count the tiles past them.
- **Edge push**: pushing the pointer against the left or right screen edge slides the
  strip that way, past a little resistance. The view stays there until focus moves, or
  until you click the focused tile that was slid off. When the whole strip fits on the
  monitor, pushing does nothing.

```text
  pointer pushes ►|            the strip slides left
  ┌───────────────┤          ┌───────────────┐
  │ B  │ C  │ D  ░│ E   ──►  │ C  │ D  │ E   │
  └───────────────┤          └───────────────┘
```

- **Width** `Mod+r` cycles the column through ⅓, ½, ⅔ and full width; `Mod+=` / `Mod+-`
  resize it by hand, `Mod+f` toggles full width.
- **Center** `Mod+Space` keeps the focused column in the middle of the monitor, even when
  that leaves empty space at an end, until focus goes to another column.

```text
   without centering             Mod+Space
  ┌─────────────────────┐      ┌─────────────────────┐
  │┌────────┐           │      │     ┌────────┐      │
  ││   A    │           │      │     │   A    │      │
  │└────────┘           │      │     └────────┘      │
  └─────────────────────┘      └─────────────────────┘
```

## Columns, rows and tiles

A column holds rows stacked on top of each other; a row holds tiles side by side.
Rows share the height of the monitor, tiles in a row share the width of the column.

```text
   column 1           column 2
  ┌──────────────┐ ┌──────────────────────┐
  │              │ │  row 1: one tile     │
  │              │ ├───────────┬──────────┤
  │   one tile   │ │  row 2:   │  two     │
  │              │ │  tile     │  tiles   │
  │              │ ├───────────┴──────────┤
  │              │ │  row 3               │
  └──────────────┘ └──────────────────────┘
```

| Keys | Does |
|---|---|
| `Mod+Shift+=` / `Mod+Shift+-` | the tile's height in its column |
| `Mod+Ctrl+=` / `Mod+Ctrl+-` | the tile's width inside its row |
| `Mod+Return` | an empty container, with dune-finder to fill it |
| `Mod+v` / `Mod+s` | a terminal right of / below the tile, in its folder |
| `Mod+q` | close the tile |

**Edge splits**: with Mod held, the pointer near the edge of a tile lights a band along that
edge. A Mod+click there opens an empty container on that side, with a compact dune-finder
that shows no list until you type.

```text
  Mod held, pointer near the right edge     Mod+click
  ┌──────────────────┐                       ┌─────────┬────────┐
  │                 ▐│                       │         │ ▸ _    │
  │      tile       ▐│  ──►                  │  tile   │ (empty │
  │                 ▐│                       │         │  slot) │
  └──────────────────┘                       └─────────┴────────┘
```

## Tabs

Tabs belong to columns, and each tab keeps its own rows and splits. The bar sits at the
top left of the column; a tab grows up to `[tabs] max_width` and shrinks to `min_width`,
then the bar scrolls.

```text
  ┌─[ vim ]─[ logs ]─[ htop ]─ + ────────────┐
  │                                          │
  │          the shown tab ("vim")           │
  │          with its own splits             │
  │                                          │
  └──────────────────────────────────────────┘
```

| Keys / mouse | Does |
|---|---|
| `Mod+Tab` / `Mod+Shift+Tab` | next / previous tab |
| `Mod+Ctrl+direction` | join the column that way as a tab, or leave a tabbed one |
| `Mod+t` | the tile out of its split into a tab of its own |
| `Mod+Shift+t` | an empty tab next to the focused one |
| click on a tab | show it |
| click on `+` | a new empty tab, with dune-finder |
| Mod+drag onto a tab bar | the tile joins that container as a tab |
| Mod+drag a tab along its bar | reorder it |
| `Ctrl+T` or `:t` in dune-finder | open the app as a tab |

## Folding (the accordion)

A tile can fold into a thin bar that shows its title, at the smallest level with open
neighbours: inside its row, otherwise its row in the column, otherwise its column in the strip.
A lone tile folds its column. A tile squeezed too small by a neighbour folds on its own.
Hovering a folded bar focuses it; clicking it (or Enter while it is focused) opens it again.

```text
  row folded into a bar                  column folded upright
  ┌──────────────────────────┐          ┌─┐┌──────────────────┐
  │ ▸ htop — folded row      │          │3││                  │
  ├──────────────────────────┤          │ ││                  │
  │                          │          │♪││     editor       │
  │        editor            │          │ ││                  │
  │                          │          │ ││                  │
  └──────────────────────────┘          └─┘└──────────────────┘
```

A folded tabbed column shows its tab count at the top. Folded tiles that play sound show
a speaker 🔊 (followed through `pactl`; for browsers, matched by the MPRIS title).

Fold with the `fold` key (unbound by default, set it in `[keys]`), or with `[mouse] fold_click`
(for example `"mod+right"`) on the tile under the pointer.

## Fullscreen

A tile goes fullscreen when its app asks (a video's `f`, a page's fullscreen button, a
game) or with `Mod+Shift+f`. It covers its whole monitor, bar included, with no gaps or
borders; only Overlay layers (dune-finder, notifications set to `layer=overlay` in mako)
and the pointer show over it.

```text
   Mod+Shift+f (or the app's own key)
  ┌─ waybar ────────────────────┐      ┌─────────────────────────────┐
  │┌──────┐┌────────┐┌──────┐   │      │                             │
  ││  A   ││ video  ││  C   │   │ ──►  │           video             │
  │└──────┘└────────┘└──────┘   │      │                             │
  └─────────────────────────────┘      └─────────────────────────────┘
```

It keeps its place in the strip (or as a float): leaving fullscreen puts it back. It
ends with the same key, the app's own key (Esc in a browser), closing it, or focus
going to another tile of that monitor (`Mod+h/l`, a new window, a notification). Going
to another desktop and back keeps it.

## Floating tiles and the scratchpad

`Mod+Shift+Space` (or a Mod+click without moving) floats the tile above the strip; the
same again puts it back. Mod+left-drag moves a float, Mod+right-drag resizes it.

The **scratchpad** works like sway's: `` Mod+Shift+` `` puts the tile away, off every desktop.
`` Mod+` `` calls the next one back as a float on the active monitor; on a shown, focused one
it puts it away again at the end of the queue, so repeated presses cycle through them.

```text
   desktop                          scratchpad queue
  ┌──────────────────────┐         ┌───┬───┬───┐
  │  ┌──────────┐        │ ◄─Mod+`─│ 1 │ 2 │ 3 │
  │  │ float 1  │        │         └───┴───┴───┘
  │  └──────────┘        │ ─Mod+`─►  (back to the end)
  └──────────────────────┘
```

## Desktops and monitors

Every monitor has nine desktops (1-9) and shows one of them.

| Keys | Does |
|---|---|
| `Mod+1` .. `Mod+9` | show that desktop |
| `Mod+Shift+1` .. `Mod+Shift+9` | send the focused tile there |
| `[keys] focus_monitor_*` | focus the monitor that way (unbound by default) |

```text
   HDMI-A-1                      DP-1
  ┌──────────────────┐          ┌──────────────────┐
  │  desktop 2 of 9  │          │  desktop 5 of 9  │
  │  [1] 2 [3] ...   │          │  ... [5] ...     │
  └──────────────────┘          └──────────────────┘
```

Apps can be launched onto a desktop (`firefox :d3`); with `follow = true` dune goes there too.
Clicking a notification (mako, xdg-activation) goes to the window, showing its desktop and
its tab.

## Terminals

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

## dune-finder

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
| **windows** | `/` in apps, or the `switcher` key | every window on every monitor, desktop, tab and folded row, most recent first; Enter right away goes back to the previous window. `:d2` / `:m1` filter by desktop / monitor. `:l` lists the windows of locked desktops. |
| **browser tabs** | in windows | a Chromium browser started with `--remote-debugging-port=9222` lists its tabs; Enter brings the tab forward in its window. |
| **files** | `#` in apps | browse from home: Enter goes into a folder or opens a file (`xdg-open`); `~`, `/` and `name/` work as in a shell; `Mod+Enter` opens `file_manager` (yazi, set in `finder.toml`), `Ctrl+Enter` a terminal there. |
| **jumps** | `>` in apps | folders by frecency, like zoxide's `z` (parent folders in home included). Enter opens a terminal there, `Mod+Enter` the file manager, `/` goes into the folder in files mode. |
| **tile options** | `Mod+/` | private, float, center, full width, to tab, send to desktop #, close, each with its key. |
| **system** | `Mod+Tab` from the last mode | power off, reboot, log out. |
| **empty container** | `Mod+Return` | Enter right away puts a terminal there; anything chosen or typed goes there instead (`/` then a window moves it in). |

A click chooses a row, the wheel scrolls, a click outside closes the panel.

## Mouse

| Mouse | Does |
|---|---|
| click | focus the tile (a focused tile slid half off comes back whole) |
| hover | focus, with `focus_follows_mouse = true` (only tiles fully on screen) |
| Mod+left drag | move the tile; drop it on another column, monitor, or a tab bar |
| Mod+left click | float / unfloat; near an edge: an empty split there |
| Mod+right drag | resize the tile (or a float) |
| `[mouse] fold_click` | fold / open the tile under the pointer |
| push a screen edge | slide the strip |

```text
   Mod+left drag of B onto D's column
  ┌──────┐┌──────┐┌──────┐          ┌──────┐┌──────┐
  │  A   ││  B ✋ ││  D   │   ──►    │  A   ││  D   │
  │      ││      ││      │          │      │├──────┤
  └──────┘└──────┘└──────┘          └──────┘│  B   │
                                            └──────┘
```

## Privacy: private tiles and locked desktops

Made for screen sharing: what is shared is what you see.

- A **private tile** (`Mod+/` → private, or the `private_tile` key) is blocked on screen and
  in every capture. Tab bars and dune-finder show it as `🔒 <app>`, never its title.
- A **locked desktop**: a right click on it in waybar locks it (`🔒 n`). It is never shown;
  `Mod+N`, the bar, dune-finder and notifications leave it alone. Locking the shown one
  switches to another desktop. Windows can still be sent there. Another right click, or
  choosing a window from `:l` in dune-finder, unlocks it.

```text
  ┌──────────────────────────────────────┐
  │┌──────────────┐┌────────────────────┐│
  ││              ││░░░░░░░░░░░░░░░░░░░░││
  ││   editor     ││░░░░ 🔒 slack ░░░░░░░││  ← private tile: blocked on
  ││              ││░░░░░░░░░░░░░░░░░░░░││    screen and in the stream
  │└──────────────┘└────────────────────┘│
  └──────────────────────────────────────┘
```

## Screenshots and recordings

| Keys | Does |
|---|---|
| `Print` | screenshot of a region chosen with the mouse (Esc cancels) |
| `Ctrl+Print` | screenshot of the focused tile |
| `Shift+Print` | screenshot of the active monitor |
| `Mod+Shift+r` | record a region (any record key again stops) |
| `Mod+Ctrl+r` | record the focused tile |
| `Mod+Ctrl+Shift+r` | record the active monitor |

Screenshots are saved as `dune-YYYY-MM-DD_HH-MM-SS.png` in `[capture] screenshot_dir`, copied to the
clipboard and announced by notification. Recordings are `.mp4` (no sound) in
`[capture] recording_dir`. Private tiles never show up in either.

dune implements `ext-image-copy-capture` and `wlr-screencopy`, so grim, wf-recorder and
xdg-desktop-portal-wlr (screen sharing in browsers and calls) work. It also implements
ext/wlr data control, so wl-copy and wl-paste work.

## Session: lock, idle, night light, X11

- **Lock**: `Mod+Escape` runs `lock_command` (`swaylock -f`), through `ext-session-lock`.
  While locked only the lock surface is shown and compositor keys are off.
- **Idle**: `ext-idle-notify` for swayidle's timers; idle-inhibit lets videos and games hold it off.
- **Night light**: `wlr-gamma-control` for wlsunset (real session only).
- **X11 apps**: through `xwayland-satellite`, started when the first X11 client connects
  and restarted if it crashes (`xwayland = "off"` disables it).
- **Games**: pointer constraints (locked and confined pointers) and relative motion.
- **Layers**: wlr-layer-shell for bars, notifications, launchers and wallpapers.

## Bars (waybar)

| Module | Setup |
|---|---|
| desktops | `ext/workspaces`, with `"on-click-right": "deactivate"` to lock / unlock |
| scratchpad | `custom`, `exec: dune scratchpad watch`, `return-type: json`, `on-click: dune scratchpad show` |
| recording | `custom`, `exec: dune recording watch`, `return-type: json`, `on-click: dune recording stop` |
| night light | waybar's own module, which runs wlsunset |

## Command line

```sh
dune open [--split l|r|u|d] [--from TILE] [--cwd DIR] [--] COMMAND...   # a terminal tile
dune-open COMMAND...                     # in a terminal: the program takes its tile
dune scratchpad show | watch
dune recording stop | watch
```

The control socket is `$DUNE_SOCKET`; the protocol lives in `dune-common/src/protocol.rs`.

---

## Configuration

Two TOML files in `~/.config/dune/` (or `$XDG_CONFIG_HOME/dune/`): `dune.toml` for the
window manager and `finder.toml` for dune-finder. Every setting is optional. Saving
`dune.toml` applies it live, except monitor modes, `off`, `xwayland` and `exec`;
dune-finder reads `finder.toml` each time it opens.

```toml
# ~/.config/dune/dune.toml
terminal = "wezterm start --always-new-process"
modifier = "alt"               # alt (default), super or ctrl+alt: `mod` below
lock_command = "swaylock -f"
follow = true                  # go to the desktop an app is launched on
focus_follows_mouse = true
xwayland = "satellite"         # or "off"
font = "JetBrains Mono"        # tab titles
border_color = "none"          # "#RRGGBB", "#RRGGBBAA" or "none"
exec = ["waybar", "mako"]      # programs started with dune, in order

[keyboard]
layout = "us"
variant = "dvorak"
repeat_delay = 500             # ms before a held key repeats
repeat_rate = 25               # repeats per second

[tabs]
min_width = 200
max_width = 220

[capture]
screenshot_dir = "~/shots"
recording_dir = "~/videos"

[mouse]
fold_click = "mod+right"

[input]                        # mice and touchpads (real session)
left_handed = true
natural_scroll = false
tap = true
accel_speed = 0.0              # -1.0 .. 1.0

[output.HDMI-A-1]              # monitors, by connector name
mode = "2560x1080@60"
position = [0, 0]

[output.DP-1]
off = true

[keys]
close = ["super+q", "super+'"]
launcher = "super+e"
switcher = "none"
```

```toml
# ~/.config/dune/finder.toml
font = "JetBrains Mono"
icon_theme = "Papirus-Dark"
file_manager = "yazi"
file_manager_terminal = "foot"
```

Every key can be rebound in `[keys]`, with one key or a list; a value replaces that key's
defaults and `"none"` (or `[]`) turns it off. The names and defaults are listed at the top
of `src/config.rs`. A setting dune does not know, or a value it cannot use, is warned about in
the log; a file that is not TOML is reported with its line, and the settings in use stay.
Environment variables win over the file: `XKB_DEFAULT_*`, `DUNE_TERMINAL`, `DUNE_MOD`,
`DUNE_XWAYLAND`.

## Default keys

`Mod` is `modifier` (Alt by default). The first column is the name to rebind it with in
`[keys]`.

| Name | Default | Does |
|---|---|---|
| `focus_left` / `_down` / `_up` / `_right` | `Mod+h/j/k/l`, arrows | focus |
| `swap_left` / `_down` / `_up` / `_right` | `Mod+Shift+h/j/k/l`, arrows | swap |
| `move_tab_left` / `_down` / `_up` / `_right` | `Mod+Ctrl+h/j/k/l`, arrows | join / leave tabs |
| `focus_monitor_left` / `_down` / `_up` / `_right` | none | focus the monitor that way |
| `desk_1` .. `desk_9` | `Mod+1..9` | show desktop |
| `send_to_desk_1` .. `send_to_desk_9` | `Mod+Shift+1..9` | send to desktop |
| `empty_container` | `Mod+Return` | empty container |
| `empty_tab` | `Mod+Shift+t` | empty tab |
| `next_tab` / `prev_tab` | `Mod+Tab` / `Mod+Shift+Tab` | next / previous tab |
| `wider` / `narrower` | `Mod+=` / `Mod+-` | column width |
| `taller` / `shorter` | `Mod+Shift+=` / `Mod+Shift+-` | tile height |
| `row_wider` / `row_narrower` | `Mod+Ctrl+=` / `Mod+Ctrl+-` | width in row |
| `cycle_width` | `Mod+r` | cycle width |
| `full_width` | `Mod+f` | full width |
| `fullscreen` | `Mod+Shift+f` | fullscreen |
| `center` | `Mod+Space` | center |
| `float` | `Mod+Shift+Space` | float |
| `send_to_scratchpad` / `scratchpad` | `` Mod+Shift+` `` / `` Mod+` `` | to / from the scratchpad |
| `close` | `Mod+q` | close |
| `launcher` | `Mod+d` | dune-finder: apps |
| `tile_options` | `Mod+/` | dune-finder: tile options |
| `switcher` | none | dune-finder: windows |
| `private_tile` | none | private tile |
| `fold` | none | fold / open the tile |
| `terminal_vertical` / `terminal_horizontal` | `Mod+v` / `Mod+s` | terminal right / below |
| `terminal_tab` | none | terminal as a tab |
| `stack_to_tabs` | `Mod+t` | stack to tabs |
| `lock` | `Mod+Escape` | lock |
| `quit` | `Mod+Shift+e` | quit |
| `screenshot` / `_tile` / `_screen` | `Print` / `Ctrl+Print` / `Shift+Print` | screenshot region / tile / monitor |
| `record` / `_tile` / `_screen` | `Mod+Shift+r` / `Mod+Ctrl+r` / `Mod+Ctrl+Shift+r` | record region / tile / monitor |

`Ctrl+Alt+F1..F12` always switch VTs on a real session.

---

## Development

```sh
make check    # tests and clippy for the whole workspace
make build    # release build
```

| Crate | What |
|---|---|
| `dune` (`src/`) | the compositor; `layout.rs` is the pure, unit-tested tiling layout |
| `dune-finder/` | the launcher / switcher panel (wlr-layer-shell client) |
| `dune-common/` | shared code: protocol, fonts, desktop entries, jumps, the shell layer |
