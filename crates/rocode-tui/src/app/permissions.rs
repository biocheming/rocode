use std::collections::HashSet;

use crate::components::PermissionRequest;
use rocode_permission::PermissionReply;

use super::App;

impl App {
    fn permission_request_to_prompt(
        permission: &crate::api::PermissionRequestInfo,
    ) -> PermissionRequest {
        let metadata = permission.input.metadata_view();
        let resource = (!permission.input.patterns.is_empty())
            .then(|| permission.input.patterns.join(", "))
            .or_else(|| {
                metadata
                    .command
                    .or(metadata.filepath)
                    .or(metadata.path)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or_else(|| permission.message.clone());

        PermissionRequest {
            id: permission.id.clone(),
            permission: permission.tool.clone(),
            resource,
            tool_name: permission.tool.as_str().to_string(),
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
        reply: PermissionReply,
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
