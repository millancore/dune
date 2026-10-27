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
with tabs, a dock per desktop, floats, a scratchpad and nine desktops per monitor on top. It comes with
**dune-finder**, one panel that is the launcher, the window switcher, a file browser, a
folder jumper, the tile menu and the power menu.

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

## First steps

`Mod` is Alt by default (`modifier` in `dune.toml`).

| Keys | Does |
|---|---|
| `Mod+d` | dune-finder: open an app |
| `Mod+h/j/k/l` (or the arrows) | move focus along the strip |
| `Mod+Shift+h/j/k/l` | swap the tile with its neighbour |
| `Mod+v` / `Mod+s` | a terminal right of / below the tile |
| `Mod+r` | cycle the column width |
| `Mod+Shift+f` | fullscreen |
| `Mod+Shift+d` / `Mod+Shift+e` | the tile into the dock / show the dock |
| `Mod+1` .. `Mod+9` | show a desktop |
| `Mod+q` | close the tile |

Every key is listed, and can be rebound, in [Configuration](docs/configuration.md#default-keys).

## Documentation

| Page | About |
|---|---|
| [Windows](docs/windows.md) | the strip, columns, rows and tabs, fullscreen, floats and the scratchpad, desktops and monitors, the mouse |
| [The dock](docs/dock.md) | tiles put out of the way, pinned entries, the scratchpad cell |
| [Terminals](docs/terminals.md) | terminals in the folder you work in, `dune-open` |
| [dune-finder](docs/finder.md) | launcher, window switcher, files, jumps, tile options, system |
| [Privacy](docs/privacy.md) | private tiles and locked desktops for screen sharing |
| [Screenshots and recordings](docs/capture.md) | capture keys, where files go, supported protocols |
| [Session and bar](docs/session.md) | lock, idle, night light, X11, Waybar modules, the `dune` command line |
| [Configuration](docs/configuration.md) | `dune.toml`, `finder.toml` and the default keys |
| [Development](docs/development.md) | building, checks and the crates |
