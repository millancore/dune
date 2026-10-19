//! dune-term: the terminal emulator of the dune compositor.
//!
//! `dune-term [COMMAND [ARGS...]]` runs COMMAND, or `$SHELL`, in a new window.

mod app;
mod config;
use dune_common::font;
mod keys;
mod palette;
mod render;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // Accept the common `-e COMMAND` spelling other terminals use.
    if args.first().is_some_and(|a| a == "-e") {
        args.remove(0);
    }
    if let Err(err) = app::run(args) {
        eprintln!("dune-term: {err}");
        std::process::exit(1);
    }
}
