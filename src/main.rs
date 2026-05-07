use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use yazelix_zellij_popup::popup_contract::{
    resolve_transient_toggle_plan_by_identity, select_transient_pane_by_identity,
    ConfiguredPopupSpecs, PopupMessageRequestError, TransientPaneGeometry, TransientPaneSnapshot,
    TransientPopupAction, TransientPopupCommandHook, TransientPopupPipeRequest,
    TransientTogglePlan,
};
use zellij_tile::prelude::*;

const RESULT_CLOSED: &str = "closed";
const RESULT_CLOSED_FLOATING_CLEANUP_FAILED: &str = "closed_floating_cleanup_failed";
const RESULT_DENIED: &str = "permissions_denied";
const RESULT_FOCUSED: &str = "focused";
const RESULT_INVALID_CONFIG: &str = "invalid_config";
const RESULT_INVALID_PAYLOAD: &str = "invalid_payload";
const RESULT_MISSING: &str = "missing";
const RESULT_MISSING_CONFIG: &str = "missing_config";
const RESULT_NOT_READY: &str = "not_ready";
const RESULT_OPENED: &str = "opened";

#[derive(Default)]
struct State {
    active_tab_position: Option<usize>,
    terminal_panes_by_tab: HashMap<usize, Vec<TerminalPane>>,
    initial_cwd: PathBuf,
    permissions_granted: bool,
    popup_specs: ConfiguredPopupSpecs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalPane {
    pane_id: PaneId,
    title: String,
    terminal_command: Option<String>,
    is_focused: bool,
    is_floating: bool,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        set_selectable(false);
        self.initial_cwd = get_plugin_ids().initial_cwd;
        self.popup_specs = ConfiguredPopupSpecs::from_configuration(&configuration);
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::OpenTerminalsOrPlugins,
            PermissionType::RunCommands,
            PermissionType::ReadCliPipes,
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::PermissionRequestResult,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::TabUpdate(tabs) => {
                self.active_tab_position =
                    tabs.iter().find(|tab| tab.active).map(|tab| tab.position);
            }
            Event::PaneUpdate(pane_manifest) => {
                self.terminal_panes_by_tab = build_terminal_panes_by_tab(&pane_manifest);
            }
            Event::PermissionRequestResult(status) => {
                self.permissions_granted = status == PermissionStatus::Granted;
            }
            _ => {}
        }

        false
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        let request = match self
            .popup_specs
            .request_from_message(&pipe_message.name, pipe_message.payload.as_deref())
        {
            Ok(request) => request,
            Err(PopupMessageRequestError::UnknownAction) => return false,
            Err(PopupMessageRequestError::InvalidPayload) => {
                self.respond(&pipe_message, RESULT_INVALID_PAYLOAD);
                return false;
            }
            Err(PopupMessageRequestError::InvalidConfiguredSpec(_)) => {
                self.respond(&pipe_message, RESULT_INVALID_CONFIG);
                return false;
            }
            Err(PopupMessageRequestError::MissingConfiguredSpec(_)) => {
                self.respond(&pipe_message, RESULT_MISSING_CONFIG);
                return false;
            }
        };

        self.handle_transient_popup(&pipe_message, request);
        false
    }

    fn render(&mut self, _rows: usize, _cols: usize) {}
}

impl State {
    fn handle_transient_popup(
        &mut self,
        pipe_message: &PipeMessage,
        request: TransientPopupPipeRequest,
    ) {
        let Some(active_tab_position) = self.ensure_ready(pipe_message) else {
            return;
        };

        let terminal_panes = self
            .terminal_panes_by_tab
            .get(&active_tab_position)
            .cloned()
            .unwrap_or_default();
        let snapshots: Vec<TransientPaneSnapshot<'_, PaneId>> = terminal_panes
            .iter()
            .map(|pane| pane.transient_snapshot())
            .collect();

        let fallback_cwd = self.launch_fallback_cwd(active_tab_position);

        match request.action {
            TransientPopupAction::Toggle => {
                match resolve_transient_toggle_plan_by_identity(&snapshots, request.spec.identity())
                {
                    TransientTogglePlan::Open => {
                        self.open_popup(pipe_message, &request, &fallback_cwd)
                    }
                    TransientTogglePlan::Focus(pane_id) => self.focus_popup(pipe_message, pane_id),
                    TransientTogglePlan::CloseAndHideFloatingLayer(pane_id) => self.close_popup(
                        pipe_message,
                        pane_id,
                        request.spec.on_close.as_ref(),
                        &fallback_cwd,
                    ),
                }
            }
            TransientPopupAction::Open => self.open_popup(pipe_message, &request, &fallback_cwd),
            TransientPopupAction::Focus => {
                match select_transient_pane_by_identity(&snapshots, request.spec.identity()) {
                    Some(pane) => self.focus_popup(pipe_message, pane.pane_id),
                    None => self.respond(pipe_message, RESULT_MISSING),
                }
            }
            TransientPopupAction::Close => {
                match select_transient_pane_by_identity(&snapshots, request.spec.identity()) {
                    Some(pane) => self.close_popup(
                        pipe_message,
                        pane.pane_id,
                        request.spec.on_close.as_ref(),
                        &fallback_cwd,
                    ),
                    None => self.respond(pipe_message, RESULT_MISSING),
                }
            }
        }
    }

    fn ensure_ready(&self, pipe_message: &PipeMessage) -> Option<usize> {
        if !self.permissions_granted {
            self.respond(pipe_message, RESULT_DENIED);
            return None;
        }

        let Some(active_tab_position) = self.active_tab_position else {
            self.respond(pipe_message, RESULT_NOT_READY);
            return None;
        };

        Some(active_tab_position)
    }

    fn launch_fallback_cwd(&self, active_tab_position: usize) -> String {
        self.terminal_panes_by_tab
            .get(&active_tab_position)
            .and_then(|panes| panes.iter().find(|pane| pane.is_focused))
            .and_then(|pane| get_pane_cwd(pane.pane_id).ok())
            .unwrap_or_else(|| self.initial_cwd.clone())
            .display()
            .to_string()
    }

    fn open_popup(
        &self,
        pipe_message: &PipeMessage,
        request: &TransientPopupPipeRequest,
        fallback_cwd: &str,
    ) {
        let Some(launch_plan) = request.launch_plan(fallback_cwd) else {
            self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
            return;
        };
        let command_to_run = CommandToRun {
            path: PathBuf::from(launch_plan.command_path),
            args: launch_plan.args,
            cwd: Some(PathBuf::from(launch_plan.cwd)),
        };
        let pane_id = open_command_pane_floating(
            command_to_run,
            floating_coordinates(launch_plan.geometry),
            BTreeMap::new(),
        );

        if let Some(pane_id) = pane_id {
            rename_pane_with_id(pane_id, request.spec.pane_title.trim());
            self.respond(pipe_message, RESULT_OPENED);
        } else {
            self.respond(pipe_message, RESULT_MISSING);
        }
    }

    fn focus_popup(&self, pipe_message: &PipeMessage, pane_id: PaneId) {
        focus_pane_with_id(pane_id, true, false);
        self.respond(pipe_message, RESULT_FOCUSED);
    }

    fn close_popup(
        &self,
        pipe_message: &PipeMessage,
        pane_id: PaneId,
        on_close: Option<&TransientPopupCommandHook>,
        fallback_cwd: &str,
    ) {
        close_pane_with_id(pane_id);
        run_on_close_hook(on_close, fallback_cwd);
        match hide_floating_panes(None) {
            Ok(_) => self.respond(pipe_message, RESULT_CLOSED),
            Err(_) => self.respond(pipe_message, RESULT_CLOSED_FLOATING_CLEANUP_FAILED),
        }
    }

    fn respond(&self, pipe_message: &PipeMessage, result: &str) {
        if let PipeSource::Cli(pipe_id) = &pipe_message.source {
            cli_pipe_output(pipe_id, result);
        }
    }
}

impl TerminalPane {
    fn transient_snapshot(&self) -> TransientPaneSnapshot<'_, PaneId> {
        TransientPaneSnapshot {
            pane_id: self.pane_id,
            title: self.title.as_str(),
            terminal_command: self.terminal_command.as_deref(),
            is_plugin: false,
            exited: false,
            is_floating: self.is_floating,
            is_focused: self.is_focused,
        }
    }
}

fn build_terminal_panes_by_tab(pane_manifest: &PaneManifest) -> HashMap<usize, Vec<TerminalPane>> {
    pane_manifest
        .panes
        .iter()
        .map(|(tab_position, panes)| {
            let terminal_panes = panes
                .iter()
                .filter(|pane| !pane.is_plugin && !pane.exited)
                .map(|pane| TerminalPane {
                    pane_id: PaneId::Terminal(pane.id),
                    title: pane.title.clone(),
                    terminal_command: pane.terminal_command.clone(),
                    is_focused: pane.is_focused,
                    is_floating: pane.is_floating,
                })
                .collect();
            (*tab_position, terminal_panes)
        })
        .collect()
}

fn floating_coordinates(geometry: TransientPaneGeometry) -> Option<FloatingPaneCoordinates> {
    let width_arg = format!("{}%", geometry.width_percent);
    let height_arg = format!("{}%", geometry.height_percent);
    let x_offset = ((100 - geometry.width_percent) / 2).to_string() + "%";
    let y_offset = ((100 - geometry.height_percent) / 2).to_string() + "%";

    FloatingPaneCoordinates::new(
        Some(x_offset),
        Some(y_offset),
        Some(width_arg),
        Some(height_arg),
        None,
        None,
    )
}

fn run_on_close_hook(on_close: Option<&TransientPopupCommandHook>, fallback_cwd: &str) {
    let Some(hook_plan) = on_close.and_then(|hook| hook.launch_plan(fallback_cwd)) else {
        return;
    };

    let argv = hook_plan
        .command
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    run_command_with_env_variables_and_cwd(
        &argv,
        BTreeMap::new(),
        PathBuf::from(hook_plan.cwd),
        BTreeMap::new(),
    );
}
