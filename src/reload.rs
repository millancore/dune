//! Applies `~/.config/dune/config` again when it is saved, like niri: its modification
//! time is polled, so editors that replace the file instead of writing it also work.

use std::{path::Path, time::{Duration, SystemTime}};

use smithay::reexports::calloop::{
    LoopHandle,
    timer::{TimeoutAction, Timer},
};

use crate::{Dune, config::Config, state::Backend};

const POLL_INTERVAL: Duration = Duration::from_millis(500);

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

pub fn watch(loop_handle: &LoopHandle<'static, Dune>) {
    let Some(path) = crate::config::path() else { return };
    let mut last = modified(&path);
    let watched = loop_handle.insert_source(Timer::from_duration(POLL_INTERVAL), move |_, _, state| {
        let now = modified(&path);
        if now != last {
            last = now;
            state.reload_config();
        }
        TimeoutAction::ToDuration(POLL_INTERVAL)
    });
    if let Err(err) = watched {
        tracing::warn!("the config will not reload when saved: {err}");
    }
}

impl Dune {
    pub fn reload_config(&mut self) {
        let new = crate::config::load();
        if new == self.config {
            return;
        }
        tracing::info!("config changed, applying it");
        for setting in new.restart_needed(&self.config) {
            tracing::warn!("`{setting}` changed: restart dune to apply it");
        }
        let old = std::mem::replace(&mut self.config, new);

        let keyboard = |c: &Config| {
            [&c.keyboard_rules, &c.keyboard_model, &c.keyboard_layout, &c.keyboard_variant, &c.keyboard_options].map(String::clone)
        };
        let config = self.config.clone();
        if keyboard(&config) != keyboard(&old)
            && let Some(handle) = self.seat.get_keyboard()
            && handle.set_xkb_config(self, config.xkb()).is_err()
        {
            tracing::error!("invalid keyboard layout in the dune config, keeping the current one");
        }
        if self.config.font_family != old.font_family {
            self.tab_titles = crate::tab_title::TabTitles::new(&self.config.font_family);
        }
        if self.config.input != old.input {
            self.configure_input_devices();
        }
        let positions = |c: &Config| c.outputs.iter().map(|(n, o)| (n.clone(), o.position)).collect::<Vec<_>>();
        if positions(&self.config) != positions(&old) && matches!(self.backend, Backend::Udev(_)) {
            self.place_outputs();
        }
        self.arrange();
    }
}
