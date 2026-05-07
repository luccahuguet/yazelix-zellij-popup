use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use kdl::{KdlDocument, KdlNode, KdlValue};
use serde::{Deserialize, Serialize};

const RAW_PIPE_NAME: &str = "transient_popup";
const DEFAULT_SPEC_ID: &str = "default";
const DEFAULT_POPUP_CONFIG_KEY: &str = "popup";
const NAMED_POPUPS_CONFIG_KEY: &str = "popups";
const DEFAULT_WIDTH_PERCENT: usize = 90;
const DEFAULT_HEIGHT_PERCENT: usize = 85;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransientPopupAction {
    Toggle,
    Open,
    Focus,
    Close,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransientPopupSpec {
    pub id: String,
    pub pane_title: String,
    #[serde(default)]
    pub command_marker: Option<String>,
    pub command: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    pub width_percent: usize,
    pub height_percent: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransientPopupPipeRequest {
    pub action: TransientPopupAction,
    pub spec: TransientPopupSpec,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfiguredPopupSpecs {
    specs: BTreeMap<String, TransientPopupSpec>,
    invalid_spec_ids: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PopupMessageRequestError {
    UnknownAction,
    MissingConfiguredSpec(String),
    InvalidConfiguredSpec(String),
    InvalidPayload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransientPaneIdentityView<'a> {
    pub pane_title: &'a str,
    pub command_marker: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransientPaneSnapshot<'a, Id> {
    pub pane_id: Id,
    pub title: &'a str,
    pub terminal_command: Option<&'a str>,
    pub is_plugin: bool,
    pub exited: bool,
    pub is_floating: bool,
    pub is_focused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransientPaneState<Id> {
    pub pane_id: Id,
    pub is_focused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransientTogglePlan<Id> {
    Open,
    Focus(Id),
    CloseAndHideFloatingLayer(Id),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransientPaneGeometry {
    pub width_percent: usize,
    pub height_percent: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransientPaneLaunchRequest {
    pub command_path: String,
    pub args: Vec<String>,
    pub requested_cwd: Option<String>,
    pub fallback_cwd: String,
    pub geometry: TransientPaneGeometry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransientPaneLaunchPlan {
    pub command_path: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub geometry: TransientPaneGeometry,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PopupSpecDraft {
    command: Option<String>,
    args: BTreeMap<usize, String>,
    pane_title: Option<String>,
    command_marker: Option<String>,
    cwd: Option<String>,
    width_percent: Option<String>,
    height_percent: Option<String>,
    invalid: bool,
}

impl TransientPopupAction {
    pub fn from_pipe_name(pipe_name: &str) -> Option<Self> {
        match pipe_name {
            "toggle" => Some(Self::Toggle),
            "open" => Some(Self::Open),
            "focus" => Some(Self::Focus),
            "close" => Some(Self::Close),
            _ => None,
        }
    }
}

impl ConfiguredPopupSpecs {
    pub fn from_configuration(configuration: &BTreeMap<String, String>) -> Self {
        let mut drafts = BTreeMap::<String, PopupSpecDraft>::new();

        if let Some(raw_default_popup) = configuration.get(DEFAULT_POPUP_CONFIG_KEY) {
            parse_popup_fields_into(
                DEFAULT_SPEC_ID,
                raw_default_popup,
                drafts.entry(DEFAULT_SPEC_ID.to_string()).or_default(),
            );
        }

        if let Some(raw_named_popups) = configuration.get(NAMED_POPUPS_CONFIG_KEY) {
            parse_named_popups_into(raw_named_popups, &mut drafts);
        }

        let mut specs = BTreeMap::new();
        let mut invalid_spec_ids = BTreeSet::new();

        for (id, draft) in drafts {
            match build_configured_spec(&id, draft) {
                Some(spec) if spec.is_launchable() => {
                    specs.insert(id, spec);
                }
                _ => {
                    invalid_spec_ids.insert(id);
                }
            }
        }

        Self {
            specs,
            invalid_spec_ids,
        }
    }

    pub fn request_from_message(
        &self,
        pipe_name: &str,
        payload: Option<&str>,
    ) -> Result<TransientPopupPipeRequest, PopupMessageRequestError> {
        if pipe_name == RAW_PIPE_NAME {
            return parse_raw_request(payload);
        }

        let Some(action) = TransientPopupAction::from_pipe_name(pipe_name) else {
            return Err(PopupMessageRequestError::UnknownAction);
        };
        let spec_id = self.resolve_requested_spec_id(payload)?;

        if self.invalid_spec_ids.contains(&spec_id) {
            return Err(PopupMessageRequestError::InvalidConfiguredSpec(spec_id));
        }
        let Some(spec) = self.specs.get(&spec_id).cloned() else {
            return Err(PopupMessageRequestError::MissingConfiguredSpec(spec_id));
        };

        Ok(TransientPopupPipeRequest {
            action,
            spec,
            args: vec![],
            cwd: None,
        })
    }

    fn resolve_requested_spec_id(
        &self,
        payload: Option<&str>,
    ) -> Result<String, PopupMessageRequestError> {
        if let Some(spec_id) = payload.map(str::trim).filter(|value| !value.is_empty()) {
            return Ok(spec_id.to_string());
        }

        if self.specs.contains_key(DEFAULT_SPEC_ID)
            || self.invalid_spec_ids.contains(DEFAULT_SPEC_ID)
        {
            return Ok(DEFAULT_SPEC_ID.to_string());
        }

        if self.specs.len() == 1 && self.invalid_spec_ids.is_empty() {
            return Ok(self.specs.keys().next().cloned().unwrap_or_default());
        }

        Err(PopupMessageRequestError::InvalidPayload)
    }
}

impl TransientPopupSpec {
    pub fn identity(&self) -> TransientPaneIdentityView<'_> {
        TransientPaneIdentityView {
            pane_title: self.pane_title.as_str(),
            command_marker: self
                .command_marker
                .as_deref()
                .map(str::trim)
                .filter(|marker| !marker.is_empty()),
        }
    }

    pub fn geometry(&self) -> Option<TransientPaneGeometry> {
        if !(1..=100).contains(&self.width_percent) || !(1..=100).contains(&self.height_percent) {
            return None;
        }

        Some(TransientPaneGeometry {
            width_percent: self.width_percent,
            height_percent: self.height_percent,
        })
    }

    fn is_launchable(&self) -> bool {
        if self.id.trim().is_empty()
            || self.pane_title.trim().is_empty()
            || self.geometry().is_none()
        {
            return false;
        }

        if self
            .command_marker
            .as_deref()
            .is_some_and(|marker| marker.trim().is_empty())
        {
            return false;
        }

        self.command
            .first()
            .is_some_and(|command_path| !command_path.trim().is_empty())
            && self.command.iter().all(|arg| !arg.trim().is_empty())
            && self
                .cwd
                .as_deref()
                .map(str::trim)
                .is_none_or(|cwd| !cwd.is_empty())
    }
}

impl TransientPopupPipeRequest {
    pub fn is_launchable_spec(&self) -> bool {
        self.spec.is_launchable()
    }

    pub fn launch_plan(&self, fallback_cwd: &str) -> Option<TransientPaneLaunchPlan> {
        if self.spec.id.trim().is_empty() || self.spec.pane_title.trim().is_empty() {
            return None;
        }

        let command_path = self.spec.command.first()?.clone();
        let mut args = self
            .spec
            .command
            .iter()
            .skip(1)
            .cloned()
            .collect::<Vec<_>>();
        args.extend(self.args.iter().cloned());

        resolve_transient_launch_plan(TransientPaneLaunchRequest {
            command_path,
            args,
            requested_cwd: self.cwd.clone().or_else(|| self.spec.cwd.clone()),
            fallback_cwd: fallback_cwd.to_string(),
            geometry: self.spec.geometry()?,
        })
    }
}

pub fn resolve_transient_launch_plan(
    request: TransientPaneLaunchRequest,
) -> Option<TransientPaneLaunchPlan> {
    let command_path = request.command_path.trim();
    if command_path.is_empty() {
        return None;
    }
    let requested_cwd = request
        .requested_cwd
        .as_deref()
        .map(str::trim)
        .filter(|cwd| !cwd.is_empty());
    let cwd = resolve_launch_cwd(requested_cwd, request.fallback_cwd.trim())?;
    if cwd.is_empty() {
        return None;
    }

    Some(TransientPaneLaunchPlan {
        command_path: command_path.to_string(),
        args: request.args,
        cwd,
        geometry: request.geometry,
    })
}

fn resolve_launch_cwd(requested_cwd: Option<&str>, fallback_cwd: &str) -> Option<String> {
    match requested_cwd {
        Some(cwd) if Path::new(cwd).is_absolute() => Some(cwd.to_string()),
        Some(cwd) => {
            if fallback_cwd.is_empty() {
                return None;
            }

            Some(Path::new(fallback_cwd).join(cwd).display().to_string())
        }
        None => (!fallback_cwd.is_empty()).then(|| fallback_cwd.to_string()),
    }
}

pub fn select_transient_pane_by_identity<Id: Copy>(
    panes: &[TransientPaneSnapshot<'_, Id>],
    identity: TransientPaneIdentityView<'_>,
) -> Option<TransientPaneState<Id>> {
    panes
        .iter()
        .filter(|pane| {
            !pane.is_plugin
                && !pane.exited
                && pane.is_floating
                && (pane.title.trim() == identity.pane_title
                    || identity.command_marker.is_some_and(|command_marker| {
                        pane.terminal_command
                            .map(|command| command.contains(command_marker))
                            .unwrap_or(false)
                    }))
        })
        .max_by_key(|pane| pane.is_focused)
        .map(|pane| TransientPaneState {
            pane_id: pane.pane_id,
            is_focused: pane.is_focused,
        })
}

pub fn resolve_transient_toggle_plan_by_identity<Id: Copy>(
    panes: &[TransientPaneSnapshot<'_, Id>],
    identity: TransientPaneIdentityView<'_>,
) -> TransientTogglePlan<Id> {
    match select_transient_pane_by_identity(panes, identity) {
        Some(pane) if pane.is_focused => {
            TransientTogglePlan::CloseAndHideFloatingLayer(pane.pane_id)
        }
        Some(pane) => TransientTogglePlan::Focus(pane.pane_id),
        None => TransientTogglePlan::Open,
    }
}

fn parse_raw_request(
    payload: Option<&str>,
) -> Result<TransientPopupPipeRequest, PopupMessageRequestError> {
    let Some(payload) = payload else {
        return Err(PopupMessageRequestError::InvalidPayload);
    };
    match serde_json::from_str::<TransientPopupPipeRequest>(payload) {
        Ok(request) if request.is_launchable_spec() => Ok(request),
        _ => Err(PopupMessageRequestError::InvalidPayload),
    }
}

fn build_configured_spec(id: &str, draft: PopupSpecDraft) -> Option<TransientPopupSpec> {
    if id.trim().is_empty() || draft.invalid {
        return None;
    }

    let command_path = trim_required(draft.command)?;
    let mut command = vec![command_path.clone()];
    for arg in draft.args.into_values() {
        command.push(trim_required(Some(arg))?);
    }

    Some(TransientPopupSpec {
        id: id.trim().to_string(),
        pane_title: trim_optional(draft.pane_title).unwrap_or_else(|| format!("{id}_popup")),
        command_marker: trim_optional(draft.command_marker).or(Some(command_path)),
        command,
        cwd: trim_optional(draft.cwd),
        width_percent: parse_percent(draft.width_percent, DEFAULT_WIDTH_PERCENT)?,
        height_percent: parse_percent(draft.height_percent, DEFAULT_HEIGHT_PERCENT)?,
    })
}

fn parse_named_popups_into(raw: &str, drafts: &mut BTreeMap<String, PopupSpecDraft>) {
    let Ok(document) = raw.parse::<KdlDocument>() else {
        drafts
            .entry(NAMED_POPUPS_CONFIG_KEY.to_string())
            .or_default()
            .invalid = true;
        return;
    };

    for popup_node in document.nodes() {
        let id = popup_node.name().value().trim();
        let draft = drafts.entry(id.to_string()).or_default();
        if id.is_empty() {
            draft.invalid = true;
            continue;
        }

        let Some(children) = popup_node.children() else {
            draft.invalid = true;
            continue;
        };

        parse_popup_fields_document_into(children, draft);
    }
}

fn parse_popup_fields_into(id: &str, raw: &str, draft: &mut PopupSpecDraft) {
    if id.trim().is_empty() {
        draft.invalid = true;
        return;
    }

    let Ok(document) = raw.parse::<KdlDocument>() else {
        draft.invalid = true;
        return;
    };

    parse_popup_fields_document_into(&document, draft);
}

fn parse_popup_fields_document_into(document: &KdlDocument, draft: &mut PopupSpecDraft) {
    for field_node in document.nodes() {
        let field_name = field_node.name().value();
        let Some(field) = popup_config_field(field_name) else {
            draft.invalid = true;
            continue;
        };
        let Some(value) = popup_field_value(field_node) else {
            draft.invalid = true;
            continue;
        };

        apply_config_field(draft, field, value);
    }
}

fn popup_field_value(field_node: &KdlNode) -> Option<String> {
    field_node
        .entries()
        .iter()
        .find(|entry| entry.name().is_none())
        .and_then(|entry| kdl_value_to_string(entry.value()))
}

fn kdl_value_to_string(value: &KdlValue) -> Option<String> {
    value
        .as_string()
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_bool().map(|value| value.to_string()))
}

fn apply_config_field(draft: &mut PopupSpecDraft, field: PopupConfigField, value: String) {
    match field {
        PopupConfigField::Command => draft.command = Some(value),
        PopupConfigField::PaneTitle => draft.pane_title = Some(value),
        PopupConfigField::CommandMarker => draft.command_marker = Some(value),
        PopupConfigField::Cwd => draft.cwd = Some(value),
        PopupConfigField::WidthPercent => draft.width_percent = Some(value),
        PopupConfigField::HeightPercent => draft.height_percent = Some(value),
        PopupConfigField::Arg(index) => {
            if index == 0 {
                draft.invalid = true;
            } else {
                draft.args.insert(index, value);
            }
        }
    }
}

fn trim_required(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_percent(value: Option<String>, default: usize) -> Option<usize> {
    match value {
        Some(value) => {
            let parsed = value.trim().parse::<usize>().ok()?;
            (1..=100).contains(&parsed).then_some(parsed)
        }
        None => Some(default),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PopupConfigField {
    Command,
    PaneTitle,
    CommandMarker,
    Cwd,
    WidthPercent,
    HeightPercent,
    Arg(usize),
}

fn popup_config_field(key: &str) -> Option<PopupConfigField> {
    if let Some(index) = key.strip_prefix("arg_") {
        index.parse::<usize>().ok().map(PopupConfigField::Arg)
    } else if key == "command" {
        Some(PopupConfigField::Command)
    } else if key == "pane_title" {
        Some(PopupConfigField::PaneTitle)
    } else if key == "command_marker" {
        Some(PopupConfigField::CommandMarker)
    } else if key == "cwd" {
        Some(PopupConfigField::Cwd)
    } else if key == "width_percent" {
        Some(PopupConfigField::WidthPercent)
    } else if key == "height_percent" {
        Some(PopupConfigField::HeightPercent)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_transient_toggle_plan_by_identity, ConfiguredPopupSpecs, PopupMessageRequestError,
        TransientPaneSnapshot, TransientPaneState, TransientPopupAction, TransientTogglePlan,
    };
    use std::collections::BTreeMap;

    fn config(values: &[(&str, &str)]) -> BTreeMap<String, String> {
        values
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn transient_pane<'a>(
        pane_id: i32,
        title: &'a str,
        terminal_command: Option<&'a str>,
        is_focused: bool,
    ) -> TransientPaneSnapshot<'a, i32> {
        TransientPaneSnapshot {
            pane_id,
            title,
            terminal_command,
            is_plugin: false,
            exited: false,
            is_floating: true,
            is_focused,
        }
    }

    #[test]
    fn configured_spec_builds_kdl_native_request() {
        let specs = ConfiguredPopupSpecs::from_configuration(&config(&[(
            "popup",
            r#"
                    command "gitui"
                    arg_1 "--watch"
                    pane_title "gitui_popup"
                    cwd "."
                    width_percent 90
                    height_percent 85
                "#,
        )]));

        let request = specs
            .request_from_message("toggle", None)
            .expect("configured request");

        assert_eq!(request.action, TransientPopupAction::Toggle);
        assert_eq!(request.spec.id, "default");
        assert_eq!(request.spec.command, vec!["gitui", "--watch"]);
        assert_eq!(request.spec.command_marker.as_deref(), Some("gitui"));
        assert_eq!(
            request.launch_plan("/fallback").expect("launch plan").cwd,
            "/fallback/."
        );
    }

    #[test]
    fn relative_configured_cwd_resolves_against_focused_fallback() {
        let specs = ConfiguredPopupSpecs::from_configuration(&config(&[(
            "popup",
            r#"
                    command "gitui"
                    cwd "tools"
                "#,
        )]));

        let request = specs
            .request_from_message("toggle", None)
            .expect("configured request");

        assert_eq!(
            request.launch_plan("/repo").expect("launch plan").cwd,
            "/repo/tools"
        );
    }

    #[test]
    fn configured_spec_without_cwd_uses_focused_fallback() {
        let specs = ConfiguredPopupSpecs::from_configuration(&config(&[(
            "popup",
            r#"
                    command "gitui"
                "#,
        )]));

        let request = specs
            .request_from_message("toggle", None)
            .expect("configured request");

        assert_eq!(
            request.launch_plan("/repo").expect("launch plan").cwd,
            "/repo"
        );
    }

    #[test]
    fn absolute_configured_cwd_is_preserved() {
        let specs = ConfiguredPopupSpecs::from_configuration(&config(&[(
            "popup",
            r#"
                    command "gitui"
                    cwd "/tmp/repo"
                "#,
        )]));

        let request = specs
            .request_from_message("toggle", None)
            .expect("configured request");

        assert_eq!(
            request.launch_plan("/repo").expect("launch plan").cwd,
            "/tmp/repo"
        );
    }

    #[test]
    fn nested_popups_support_multiple_named_popups() {
        let specs = ConfiguredPopupSpecs::from_configuration(&config(&[(
            "popups",
            r#"
                gitui {
                    command "gitui"
                    pane_title "gitui_popup"
                }
                lazygit {
                    command "lazygit"
                    pane_title "lazygit_popup"
                }
            "#,
        )]));

        let request = specs
            .request_from_message("toggle", Some("lazygit"))
            .expect("named configured request");

        assert_eq!(request.spec.id, "lazygit");
        assert_eq!(request.spec.command, vec!["lazygit"]);
        assert_eq!(request.spec.pane_title, "lazygit_popup");
    }

    #[test]
    fn configured_spec_defaults_to_single_spec_without_payload() {
        let specs = ConfiguredPopupSpecs::from_configuration(&config(&[(
            "popups",
            r#"
                lazygit {
                    command "lazygit"
                }
            "#,
        )]));

        let request = specs
            .request_from_message("open", None)
            .expect("single configured request");

        assert_eq!(request.action, TransientPopupAction::Open);
        assert_eq!(request.spec.id, "lazygit");
        assert_eq!(request.spec.pane_title, "lazygit_popup");
        assert_eq!(request.spec.width_percent, 90);
        assert_eq!(request.spec.height_percent, 85);
    }

    #[test]
    fn configured_spec_returns_invalid_config_for_bad_percent() {
        let specs = ConfiguredPopupSpecs::from_configuration(&config(&[(
            "popups",
            r#"
                gitui {
                    command "gitui"
                    width_percent 101
                }
            "#,
        )]));

        assert_eq!(
            specs.request_from_message("toggle", Some("gitui")),
            Err(PopupMessageRequestError::InvalidConfiguredSpec(
                "gitui".into()
            ))
        );
    }

    #[test]
    fn raw_json_request_still_works_for_generated_callers() {
        let specs = ConfiguredPopupSpecs::default();
        let payload = r#"{
            "action": "close",
            "spec": {
                "id": "gitui",
                "pane_title": "gitui_popup",
                "command_marker": "gitui",
                "command": ["gitui"],
                "cwd": ".",
                "width_percent": 90,
                "height_percent": 85
            }
        }"#;

        let request = specs
            .request_from_message("transient_popup", Some(payload))
            .expect("raw request");

        assert_eq!(request.action, TransientPopupAction::Close);
        assert_eq!(request.spec.id, "gitui");
    }

    #[test]
    fn toggle_plan_uses_title_or_command_marker_and_closes_focused_popup() {
        let specs = ConfiguredPopupSpecs::from_configuration(&config(&[(
            "popups",
            r#"
                gitui {
                    command "gitui"
                }
            "#,
        )]));
        let request = specs
            .request_from_message("toggle", Some("gitui"))
            .expect("request");
        let focused = [transient_pane(11, "other", Some("gitui"), true)];

        assert_eq!(
            resolve_transient_toggle_plan_by_identity(&focused, request.spec.identity()),
            TransientTogglePlan::CloseAndHideFloatingLayer(11)
        );
        assert_eq!(
            super::select_transient_pane_by_identity(&focused, request.spec.identity()),
            Some(TransientPaneState {
                pane_id: 11,
                is_focused: true,
            })
        );
    }
}
