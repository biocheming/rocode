fn structured_section(title: &str, body: &str) -> String {
    format!("**{title}**\n{}", body.trim())
}

fn first_meaningful_line(content: &str) -> &str {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("No summary provided.")
}

pub fn normalize_atlas_final_output(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }
    if trimmed.contains("## Delivery Summary")
        && trimmed.contains("**Task Status**")
        && trimmed.contains("**Verification**")
    {
        return trimmed.to_string();
    }

    let summary = first_meaningful_line(trimmed);
    [
        format!("## Delivery Summary\n{summary}"),
        structured_section("Task Status", trimmed),
        structured_section(
            "Verification",
            "- Preserve only evidence-backed completion claims from Atlas coordination and verification stages.",
        ),
        structured_section("Blockers or Risks", "- None noted in the final Atlas output."),
        structured_section("Next Actions", "- None."),
    ]
    .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_final_output_normalization_wraps_unstructured_delivery() {
        let output = normalize_atlas_final_output("Task A done. Task B verified.");
        assert!(output.contains("## Delivery Summary"));
        assert!(output.contains("**Task Status**"));
        assert!(output.contains("**Verification**"));
        assert!(output.contains("Task A done. Task B verified."));
    }

    #[test]
    fn atlas_final_output_normalization_preserves_structured_delivery() {
        let structured =
            "## Delivery Summary\nDone.\n\n**Task Status**\n- A\n\n**Verification**\n- B";
        assert_eq!(normalize_atlas_final_output(structured), structured);
    }
}
