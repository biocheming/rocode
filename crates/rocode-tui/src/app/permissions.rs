use std::collections::HashSet;

use crate::components::{PermissionRequest, PermissionType};
use rocode_core::contracts::patch::keys as patch_keys;
use rocode_core::contracts::permission::keys as permission_keys;
use rocode_core::contracts::permission::PermissionTypeWire;
use rocode_core::contracts::tools::BuiltinToolName;

use super::App;

impl App {
    fn permission_type_from_name(name: &str) -> PermissionType {
        if let Some(permission) = PermissionTypeWire::parse(name) {
            return match permission {
                PermissionTypeWire::ExternalDirectory => PermissionType::ExternalDirectory,
                PermissionTypeWire::List => PermissionType::List,
                PermissionTypeWire::DoomLoop => PermissionType::ExecuteCommand,
            };
        }

        match BuiltinToolName::parse(name) {
            Some(BuiltinToolName::Read) => PermissionType::ReadFile,
            Some(BuiltinToolName::Write) => PermissionType::WriteFile,
            Some(
                BuiltinToolName::Edit
                | BuiltinToolName::MultiEdit
                | BuiltinToolName::ApplyPatch,
            ) => PermissionType::Edit,
            Some(BuiltinToolName::Bash | BuiltinToolName::ShellSession) => PermissionType::Bash,
            Some(BuiltinToolName::Glob) => PermissionType::Glob,
            Some(BuiltinToolName::Grep) => PermissionType::Grep,
            Some(BuiltinToolName::Task | BuiltinToolName::TaskFlow) => PermissionType::Task,
            Some(BuiltinToolName::WebFetch | BuiltinToolName::BrowserSession) => {
                PermissionType::WebFetch
            }
            Some(BuiltinToolName::WebSearch) => PermissionType::WebSearch,
            Some(BuiltinToolName::CodeSearch | BuiltinToolName::GitHubResearch) => {
                PermissionType::CodeSearch
            }
            _ => PermissionType::ExecuteCommand,
        }
    }

    fn permission_request_to_prompt(
        permission: &crate::api::PermissionRequestInfo,
    ) -> PermissionRequest {
        let input = permission.input.as_object().cloned().unwrap_or_default();
        let permission_name = input
            .get(permission_keys::REQUEST_PERMISSION)
            .and_then(|value| value.as_str())
            .unwrap_or(permission.tool.as_str());
        let resource = input
            .get(permission_keys::REQUEST_PATTERNS)
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|value| !value.is_empty())
            .or_else(|| {
                input.get(permission_keys::REQUEST_METADATA).and_then(|value| {
                    value
                        .get(permission_keys::COMMAND)
                        .and_then(|item| item.as_str())
                        .or_else(|| value.get(patch_keys::FILEPATH).and_then(|item| item.as_str()))
                        .or_else(|| value.get(patch_keys::LEGACY_PATH).and_then(|item| item.as_str()))
                        .map(str::to_string)
                })
            })
            .unwrap_or_else(|| permission.message.clone());

        PermissionRequest {
            id: permission.id.clone(),
            permission_type: Self::permission_type_from_name(permission_name),
            resource,
            tool_name: permission_name.to_string(),
        }
    }

    pub(super) fn sync_permission_requests(&mut self) -> bool {
        let Some(client) = self.context.get_api_client() else {
            self.context.set_pending_permissions(0);
            return false;
        };

        let active_session = self.current_session_id();
        let mut permissions = match client.list_permissions() {
            Ok(items) => items,
            Err(error) => {
                tracing::debug!(%error, "failed to sync permission requests");
                return false;
            }
        };

        if let Some(session_id) = active_session.as_deref() {
            permissions.retain(|permission| permission.session_id == session_id);
        }
        permissions.sort_by(|a, b| a.id.cmp(&b.id));

        let latest_ids = permissions
            .iter()
            .map(|permission| permission.id.clone())
            .collect::<HashSet<_>>();
        let mut changed = latest_ids != self.pending_permission_ids;

        for permission in permissions {
            let permission_id = permission.id.clone();
            if self.pending_permission_ids.insert(permission_id.clone()) {
                self.permission_prompt
                    .add_request(Self::permission_request_to_prompt(&permission));
                changed = true;
            }
            self.pending_permissions.insert(permission_id, permission);
        }

        self.pending_permission_ids
            .retain(|id| latest_ids.contains(id));
        self.pending_permissions
            .retain(|id, _| latest_ids.contains(id));
        self.permission_prompt
            .retain_requests(|request| latest_ids.contains(&request.id));

        changed
    }

    pub(super) fn resolve_permission_request(
        &mut self,
        permission_id: &str,
        reply: &str,
        message: Option<String>,
    ) {
        let Some(client) = self.context.get_api_client() else {
            self.alert_dialog
                .set_message("Cannot answer permission request: no API client");
            self.alert_dialog.open();
            return;
        };

        match client.reply_permission(permission_id, reply, message) {
            Ok(()) => {
                self.pending_permission_ids.remove(permission_id);
                self.pending_permissions.remove(permission_id);
                self.permission_prompt.remove_request(permission_id);
                self.toast.show(
                    crate::components::ToastVariant::Success,
                    "Permission updated",
                    2000,
                );
            }
            Err(error) => {
                self.alert_dialog
                    .set_message(&format!("Failed to submit permission response:\n{}", error));
                self.alert_dialog.open();
            }
        }
    }

    pub(super) fn enqueue_permission_request(
        &mut self,
        permission: crate::api::PermissionRequestInfo,
    ) {
        if self.pending_permission_ids.insert(permission.id.clone()) {
            self.permission_prompt
                .add_request(Self::permission_request_to_prompt(&permission));
        }
        self.pending_permissions
            .insert(permission.id.clone(), permission);
    }

    pub(super) fn clear_permission_request(&mut self, permission_id: &str) {
        self.pending_permission_ids.remove(permission_id);
        self.pending_permissions.remove(permission_id);
        self.permission_prompt.remove_request(permission_id);
    }
}
