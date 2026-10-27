# Development

```sh
make check    # tests and clippy for the whole workspace
make build    # release build
```

| Crate | What |
|---|---|
| `dune` (`src/`) | the compositor; `layout.rs` is the pure, unit-tested tiling layout |
| `dune-finder/` | the launcher / switcher panel (wlr-layer-shell client) |
| `dune-common/` | shared code: protocol, fonts, desktop entries, jumps, the shell layer |
