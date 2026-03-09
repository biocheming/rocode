fn structured_section(title: &str, body: &str) -> String {
    format!(
        "**{title}**
{}",
        body.trim()
    )
}

fn first_meaningful_line(content: &str) -> &str {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("No summary provided.")
}

pub fn normalize_sisyphus_final_output(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }
    if trimmed.contains("## Delivery Summary")
        && trimmed.contains("**Execution Outcome**")
        && trimmed.contains("**Verification**")
    {
        return trimmed.to_string();
    }

    let summary = first_meaningful_line(trimmed);
    [
        format!("## Delivery Summary
{summary}"),
        structured_section("Execution Outcome", trimmed),
        structured_section(
            "Verification",
            "- Preserve only evidence-backed completion claims from the Sisyphus single-pass execution.",
        ),
        structured_section("Remaining Risks", "- None noted in the final Sisyphus output."),
    ]
    .join("

")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sisyphus_final_output_normalization_wraps_unstructured_delivery() {
        let output = normalize_sisyphus_final_output(
            "Shipped the change and verified the targeted behavior.",
        );
        assert!(output.contains("## Delivery Summary"));
        assert!(output.contains("**Execution Outcome**"));
        assert!(output.contains("**Verification**"));
        assert!(output.contains("Shipped the change"));
    }

    #[test]
    fn sisyphus_final_output_normalization_preserves_structured_delivery() {
        let structured = "## Delivery Summary
Done.

**Execution Outcome**
- A

**Verification**
- B";
        assert_eq!(normalize_sisyphus_final_output(structured), structured);
    }
}
