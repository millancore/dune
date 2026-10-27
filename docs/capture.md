# Screenshots and recordings

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
