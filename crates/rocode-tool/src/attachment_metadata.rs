use crate::Metadata;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
struct AttachmentMetadataWire {
    #[serde(default)]
    attachments: Option<AttachmentMetadataValueWire>,
    #[serde(default)]
    attachment: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AttachmentMetadataValueWire {
    Array(Vec<serde_json::Value>),
    Single(serde_json::Value),
}

fn attachment_metadata_wire(metadata: &Metadata) -> AttachmentMetadataWire {
    serde_json::from_value::<AttachmentMetadataWire>(serde_json::Value::Object(
        metadata.clone().into_iter().collect(),
    ))
    .unwrap_or_default()
}

pub(crate) fn collect_attachments_from_metadata(metadata: &Metadata) -> Vec<serde_json::Value> {
    let mut attachments = Vec::new();

    let mut push_unique = |value: serde_json::Value| {
        if !attachments.iter().any(|existing| existing == &value) {
            attachments.push(value);
        }
    };

    let metadata_wire = attachment_metadata_wire(metadata);
    if let Some(values) = metadata_wire.attachments {
        match values {
            AttachmentMetadataValueWire::Array(values) => {
                for value in values {
                    push_unique(value);
                }
            }
            AttachmentMetadataValueWire::Single(value) => {
                push_unique(value);
            }
        }
    }
    if let Some(value) = metadata_wire.attachment {
        push_unique(value);
    }
    attachments
}

pub(crate) fn strip_attachments_from_metadata(metadata: &Metadata) -> Metadata {
    let mut sanitized = metadata.clone();
    sanitized.remove("attachments");
    sanitized.remove("attachment");
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_attachments_reads_both_plural_and_singular_keys() {
        let mut metadata = Metadata::new();
        metadata.insert(
            "attachments".to_string(),
            serde_json::json!([{ "mime": "application/pdf", "url": "data:application/pdf;base64,AA==" }]),
        );
        metadata.insert(
            "attachment".to_string(),
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
            "attachments".to_string(),
            serde_json::json!([attachment.clone()]),
        );
        metadata.insert("attachment".to_string(), attachment);

        let attachments = collect_attachments_from_metadata(&metadata);
        assert_eq!(attachments.len(), 1);
    }

    #[test]
    fn strip_attachments_removes_attachment_payload_keys() {
        let mut metadata = Metadata::new();
        metadata.insert("foo".to_string(), serde_json::json!("bar"));
        metadata.insert(
            "attachments".to_string(),
            serde_json::json!([{ "mime": "application/pdf", "url": "data:application/pdf;base64,AA==" }]),
        );
        metadata.insert(
            "attachment".to_string(),
            serde_json::json!({ "mime": "image/png", "url": "data:image/png;base64,BB==" }),
        );

        let sanitized = strip_attachments_from_metadata(&metadata);
        assert_eq!(sanitized.get("foo").and_then(|v| v.as_str()), Some("bar"));
        assert!(!sanitized.contains_key("attachments"));
        assert!(!sanitized.contains_key("attachment"));
    }
}
