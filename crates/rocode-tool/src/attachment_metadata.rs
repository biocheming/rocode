use crate::Metadata;
use rocode_core::contracts::attachments::keys as attachment_keys;

pub(crate) fn collect_attachments_from_metadata(metadata: &Metadata) -> Vec<serde_json::Value> {
    let mut attachments = Vec::new();

    let mut push_unique = |value: serde_json::Value| {
        if !attachments.iter().any(|existing| existing == &value) {
            attachments.push(value);
        }
    };

    if let Some(value) = metadata.get(attachment_keys::ATTACHMENTS) {
        match value {
            serde_json::Value::Array(array) => {
                for item in array {
                    push_unique(item.clone());
                }
            }
            other => push_unique(other.clone()),
        }
    }
    if let Some(value) = metadata.get(attachment_keys::ATTACHMENT) {
        push_unique(value.clone());
    }
    attachments
}

pub(crate) fn strip_attachments_from_metadata(metadata: &Metadata) -> Metadata {
    let mut sanitized = metadata.clone();
    sanitized.remove(attachment_keys::ATTACHMENTS);
    sanitized.remove(attachment_keys::ATTACHMENT);
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_attachments_reads_both_plural_and_singular_keys() {
        let mut metadata = Metadata::new();
        metadata.insert(
            attachment_keys::ATTACHMENTS.to_string(),
            serde_json::json!([{ "mime": "application/pdf", "url": "data:application/pdf;base64,AA==" }]),
        );
        metadata.insert(
            attachment_keys::ATTACHMENT.to_string(),
            serde_json::json!({ "mime": "image/png", "url": "data:image/png;base64,BB==" }),
        );

        let attachments = collect_attachments_from_metadata(&metadata);
        assert_eq!(attachments.len(), 2);
    }

    #[test]
    fn collect_attachments_deduplicates_identical_payloads() {
        let attachment = serde_json::json!({ "mime": "application/pdf", "url": "data:application/pdf;base64,AA==" });
        let mut metadata = Metadata::new();
        metadata.insert(
            attachment_keys::ATTACHMENTS.to_string(),
            serde_json::json!([attachment.clone()]),
        );
        metadata.insert(attachment_keys::ATTACHMENT.to_string(), attachment);

        let attachments = collect_attachments_from_metadata(&metadata);
        assert_eq!(attachments.len(), 1);
    }

    #[test]
    fn strip_attachments_removes_attachment_payload_keys() {
        let mut metadata = Metadata::new();
        metadata.insert("foo".to_string(), serde_json::json!("bar"));
        metadata.insert(
            attachment_keys::ATTACHMENTS.to_string(),
            serde_json::json!([{ "mime": "application/pdf", "url": "data:application/pdf;base64,AA==" }]),
        );
        metadata.insert(
            attachment_keys::ATTACHMENT.to_string(),
            serde_json::json!({ "mime": "image/png", "url": "data:image/png;base64,BB==" }),
        );

        let sanitized = strip_attachments_from_metadata(&metadata);
        assert_eq!(sanitized.get("foo").and_then(|v| v.as_str()), Some("bar"));
        assert!(!sanitized.contains_key(attachment_keys::ATTACHMENTS));
        assert!(!sanitized.contains_key(attachment_keys::ATTACHMENT));
    }
}
