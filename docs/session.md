# Session and bar

Lock, idle, night light, X11, Waybar and the `dune` command line.

- [Session: lock, idle, night light, X11](#session-lock-idle-night-light-x11)
- [Waybar](#waybar)
- [Command line](#command-line)

## Session: lock, idle, night light, X11

- **Lock**: `Mod+Escape` runs `lock_command` (`swaylock -f`), through `ext-session-lock`.
  While locked only the lock surface is shown and compositor keys are off.
- **Idle**: `ext-idle-notify` for swayidle's timers; idle-inhibit lets videos and games hold it off, and so does the finder's Stay awake.
- **Night light**: `wlr-gamma-control` for wlsunset (real session only).
- **X11 apps**: through `xwayland-satellite`, started when the first X11 client connects
  and restarted if it crashes (`xwayland = "off"` disables it).
- **Games**: pointer constraints (locked and confined pointers) and relative motion.
- **Layers**: wlr-layer-shell for bars, notifications, launchers and wallpapers.

## Waybar

| Module | Setup |
|---|---|
| desktops | `ext/workspaces`, with `"on-click-right": "deactivate"` to lock / unlock |
| dock | `custom`, `exec: dune dock watch`, `return-type: json`, `on-click: dune dock show`; first in `modules-left` (each bar follows the dock of its own monitor, from `$WAYBAR_OUTPUT_NAME`; the text is the count, class `empty` or `filled`; the tooltip lists the scratchpad's windows too, it being the dock's first cell) |
| scratchpad | optional, the dock has it: `custom`, `exec: dune scratchpad watch`, `return-type: json`, `on-click: dune scratchpad show` |
| recording | `custom`, `exec: dune recording watch`, `return-type: json`, `on-click: dune recording stop` |
| night light | waybar's own module, which runs wlsunset |

## Command line

```sh
dune open [--split l|r|u|d] [--from TILE] [--cwd DIR] [--] COMMAND...   # a terminal tile
dune-open COMMAND...                     # in a terminal: the program takes its tile
dune scratchpad show | watch
dune dock show | watch                   # for the monitor of $WAYBAR_OUTPUT_NAME, else the active one
dune recording stop | watch
```

The control socket is `$DUNE_SOCKET`; the protocol lives in `dune-common/src/protocol.rs`.
