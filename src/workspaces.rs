//! Desktops for bars (waybar's `ext/workspaces`): the `ext-workspace-v1` protocol.
//!
//! Every monitor is a workspace group holding desktops 1-9. The desktop it shows is
//! `active`; an empty one it does not show is `hidden`, which bars leave out. A bar
//! that activates a desktop and commits makes its monitor show it.

use std::{collections::HashMap, sync::Mutex};

use smithay::{
    output::Output,
    reexports::{
        wayland_protocols::ext::workspace::v1::server::{
            ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
            ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
            ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, backend::ClientId,
        },
    },
};

use crate::{Dune, desktops::DESKS};

/// A desktop: monitor name and number.
type Key = (String, u8);

/// What a bound manager was told, so only changes are sent.
struct Instance {
    manager: ExtWorkspaceManagerV1,
    /// Per monitor: its group and whether it was told which `wl_output` it is.
    groups: HashMap<String, (ExtWorkspaceGroupHandleV1, bool)>,
    /// Per desktop: its handle and the last state sent.
    desks: HashMap<Key, (ExtWorkspaceHandleV1, u32)>,
}

#[derive(Default)]
pub struct WorkspaceState {
    instances: Vec<Instance>,
}

/// Data of a manager: the desktop a bar asked for, applied on commit.
#[derive(Default)]
pub struct ManagerData {
    pending: Mutex<Option<Key>>,
}

/// Data of a workspace handle: its desktop and the manager it came from.
pub struct DeskData {
    key: Key,
    manager: ExtWorkspaceManagerV1,
}

const ACTIVE: u32 = 1;
const HIDDEN: u32 = 4;

impl WorkspaceState {
    pub fn new(dh: &DisplayHandle) -> Self {
        dh.create_global::<Dune, ExtWorkspaceManagerV1, ()>(1, ());
        WorkspaceState::default()
    }
}

impl Instance {
    /// Tell the bar about monitors and desktops that changed; `desks` is every desktop
    /// of `outputs` with its state.
    fn sync(&mut self, dh: &DisplayHandle, outputs: &[Output], desks: &[(Key, u32)]) {
        let Ok(client) = dh.get_client(self.manager.id()) else { return };
        let version = self.manager.version();
        let mut changed = false;

        // Monitors that went away take their desktops with them.
        let gone: Vec<String> = self.groups.keys().filter(|n| !outputs.iter().any(|o| o.name() == **n)).cloned().collect();
        for name in gone {
            let (group, _) = self.groups.remove(&name).unwrap();
            for key in (1..=DESKS).map(|n| (name.clone(), n)) {
                if let Some((desk, _)) = self.desks.remove(&key) {
                    group.workspace_leave(&desk);
                    desk.removed();
                }
            }
            group.removed();
            changed = true;
        }

        for output in outputs {
            let name = output.name();
            if !self.groups.contains_key(&name) {
                let Ok(group) = client.create_resource::<ExtWorkspaceGroupHandleV1, _, Dune>(dh, version, name.clone()) else {
                    continue;
                };
                self.manager.workspace_group(&group);
                group.capabilities(ext_workspace_group_handle_v1::GroupCapabilities::empty());
                self.groups.insert(name.clone(), (group, false));
                changed = true;
            }
            let (group, entered) = self.groups.get_mut(&name).unwrap();
            // The bar may bind the monitor's wl_output after the manager.
            if !*entered {
                for wl_output in output.client_outputs(&client) {
                    group.output_enter(&wl_output);
                    *entered = true;
                    changed = true;
                }
            }
        }

        for (key, state) in desks {
            let Some((group, _)) = self.groups.get(&key.0) else { continue };
            match self.desks.get_mut(key) {
                Some((desk, sent)) => {
                    if sent != state {
                        desk.state(ext_workspace_handle_v1::State::from_bits_truncate(*state));
                        *sent = *state;
                        changed = true;
                    }
                }
                None => {
                    let data = DeskData { key: key.clone(), manager: self.manager.clone() };
                    let Ok(desk) = client.create_resource::<ExtWorkspaceHandleV1, _, Dune>(dh, version, data) else {
                        continue;
                    };
                    self.manager.workspace(&desk);
                    desk.id(format!("{}-{}", key.0, key.1));
                    desk.name(key.1.to_string());
                    desk.coordinates(u32::from(key.1 - 1).to_ne_bytes().to_vec());
                    desk.capabilities(ext_workspace_handle_v1::WorkspaceCapabilities::Activate);
                    desk.state(ext_workspace_handle_v1::State::from_bits_truncate(*state));
                    group.workspace_enter(&desk);
                    self.desks.insert(key.clone(), (desk, *state));
                    changed = true;
                }
            }
        }

        if changed {
            self.manager.done();
        }
    }
}

impl Dune {
    /// Send bars the desktops as they are now. Called after every `arrange`.
    pub fn refresh_workspaces(&mut self) {
        if self.workspaces.instances.is_empty() {
            return;
        }
        let outputs = self.numbered_outputs();
        let mut desks = Vec::new();
        for output in &outputs {
            let name = output.name();
            let shown = self.desk_of(&name);
            for n in 1..=DESKS {
                let state = if n == shown {
                    ACTIVE
                } else if self.hidden.contains_key(&(name.clone(), n)) {
                    0
                } else {
                    HIDDEN
                };
                desks.push(((name.clone(), n), state));
            }
        }
        let dh = self.display_handle.clone();
        for instance in &mut self.workspaces.instances {
            instance.sync(&dh, &outputs, &desks);
        }
    }
}

impl GlobalDispatch<ExtWorkspaceManagerV1, ()> for Dune {
    fn bind(
        state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ExtWorkspaceManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let manager = data_init.init(resource, ManagerData::default());
        state.workspaces.instances.push(Instance { manager, groups: HashMap::new(), desks: HashMap::new() });
        state.refresh_workspaces();
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ManagerData> for Dune {
    fn request(
        state: &mut Self,
        _client: &Client,
        manager: &ExtWorkspaceManagerV1,
        request: ext_workspace_manager_v1::Request,
        data: &ManagerData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            ext_workspace_manager_v1::Request::Commit => {
                if let Some((output, n)) = data.pending.lock().unwrap().take() {
                    state.show_desk_on(&output, n);
                }
            }
            ext_workspace_manager_v1::Request::Stop => {
                manager.finished();
                state.workspaces.instances.retain(|i| i.manager != *manager);
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _client: ClientId, manager: &ExtWorkspaceManagerV1, _data: &ManagerData) {
        state.workspaces.instances.retain(|i| i.manager != *manager);
    }
}

impl Dispatch<ExtWorkspaceGroupHandleV1, String> for Dune {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _group: &ExtWorkspaceGroupHandleV1,
        _request: ext_workspace_group_handle_v1::Request,
        _data: &String,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        // Desktops are fixed: nothing to create; destroy needs no cleanup.
    }
}

impl Dispatch<ExtWorkspaceHandleV1, DeskData> for Dune {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _desk: &ExtWorkspaceHandleV1,
        request: ext_workspace_handle_v1::Request,
        data: &DeskData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        if let ext_workspace_handle_v1::Request::Activate = request
            && let Some(manager) = data.manager.data::<ManagerData>()
        {
            *manager.pending.lock().unwrap() = Some(data.key.clone());
        }
    }
}
