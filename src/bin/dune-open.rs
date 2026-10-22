//! `dune-open COMMAND...`: run a program from a terminal tile so that its windows open in
//! that tile, on top of the terminal, which shows again when they close.
//!
//! It asks dune (a `swallow` request) to put the windows of its own process, and of the
//! children of it, in the tile of its terminal (`DUNE_TILE`), then becomes the command
//! (`exec`, same pid). Outside a terminal of dune it just runs the command.

use std::{os::unix::process::CommandExt, process::Command, time::Duration};

use dune_common::protocol::{Request, ask};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((program, rest)) = args.split_first() else {
        eprintln!("usage: dune-open COMMAND [ARGS...]");
        std::process::exit(2);
    };
    let tile = std::env::var("DUNE_TILE").ok().and_then(|t| t.parse().ok());
    if let Some(tile) = tile
        && std::env::var_os("DUNE_SOCKET").is_some()
    {
        // The answer comes once dune knows, so the windows cannot get there first.
        let _ = ask(&Request::Swallow { tile, pid: std::process::id() }, Duration::from_secs(1));
    }
    let err = Command::new(program).args(rest).exec();
    eprintln!("dune-open: {program}: {err}");
    std::process::exit(127);
}
