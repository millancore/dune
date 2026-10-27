# The dock

Every desktop of every monitor has a dock: tiles put out of the way, off the strip.
`Mod+Shift+d` (the `dock` key), `[mouse] dock_click` (for example `"mod+right"`) on the
tile under the pointer, **Send to dock** in the tile options, or the window's own minimize
button (`-`) puts a tile there. A tile
of a tabbed column takes its whole column, tabs and all.

Two ways open the dock of the desktop a monitor shows, as a bar down the left edge (along
the bottom, just above the bar, when a monitor's bars are only at the bottom), over
everything, with the icon of each entry's app (from the icon theme, as dune-finder finds
them; the app's first letter without one). A pinned entry has
an orange dot, a tabbed column a dot per tab, a window playing sound a speaker. The title
of the entry under the pointer (or chosen by keys) shows beside the bar (above it, along
the bottom).

`Mod+Shift+e` (the `dock_show` key), for the active monitor (the one desktops switch on); or a click on
the dock module of a bar (`dune dock show`, see [Waybar](session.md#waybar)), for the bar's monitor.
It stays until Esc or a click away, and keys drive it: the arrows or `h`/`j`/`k`/`l` along
the bar choose, Enter takes the entry out, `p` or Space pins it. Other keys do nothing meanwhile; the
compositor's own bindings hide it and do their thing.

```text
  Mod+Shift+e, or a click on the bar's dock module
  ┌──────┬───────────────────────────────
  │ [*]··│                                  [*] the scratchpad, two windows waiting
  │ [S] •│                                  [S] Spotify, pinned (•)
  ▌ [h]  │ htop     the strip, as it was    [h] under the pointer: its title beside
  │ [v]  │                                  [v] vim, a column of three tabs (···)
  │  ··· │
  └──────┘
```

- **Click** an entry: it goes back where it was in the strip (or where it floated), focused.
- **Right click**: pin it, or unpin it. A pinned entry (📌) is in the dock of every desktop,
  and comes out on the desktop shown: a tile floating, a tabbed column right of the
  focused one. Unpinned, it belongs to the dock it was unpinned in.
- **A tabbed column** chosen (by keys or under the pointer) fans out its tabs beside the bar
  (above it, along the bottom): a row per tab, its icon and title. A click on one, or Enter
  on it, brings the whole column back with that tab shown. The arrow away from the bar
  (right, or up along the bottom) goes into the fan, up/down go through its tabs, the arrow
  back leaves it.
- `(+2)` tells a column's other tabs; a speaker 🔊 shows on entries that play sound
  (followed through `pactl`; for browsers, matched by the MPRIS title).
- **The scratchpad** is the first cell of every dock (a dot per window waiting there).
  Chosen, under the pointer or by keys (the dock opens on it), it fans out its windows
  like a column's tabs; a click on one (or Enter) floats it on the desktop shown, a click
  on the cell takes out the first.
- dune-finder's windows mode lists docked windows (`dock`, `dock 📌`); choosing one takes
  it out of the dock, going to its desktop.

Tiles in a column or row never get smaller than a tenth of it: resizing stops there.
