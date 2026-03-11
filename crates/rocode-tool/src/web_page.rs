use regex::Regex;
use reqwest::Client;

use crate::ToolError;

pub(crate) const MAX_WEB_RESPONSE_SIZE: usize = 5 * 1024 * 1024;
pub(crate) const DEFAULT_WEB_TIMEOUT_SECS: u64 = 30;
pub(crate) const MAX_WEB_TIMEOUT_SECS: u64 = 120;
pub(crate) const DEFAULT_WEB_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";

pub(crate) fn build_web_client() -> Client {
    Client::builder()
        .user_agent(DEFAULT_WEB_USER_AGENT)
        .timeout(std::time::Duration::from_secs(MAX_WEB_TIMEOUT_SECS))
        .build()
        .expect("web client should build")
}

pub(crate) fn ensure_http_url(url: &str) -> Result<(), ToolError> {
    if url.starts_with("http://") || url.starts_with("https://") {
        return Ok(());
    }
    Err(ToolError::InvalidArguments(
        "URL must start with http:// or https://".to_string(),
    ))
}

pub(crate) fn convert_html_to_markdown(html: &str) -> String {
    html2md::parse_html(html)
}

pub(crate) fn strip_html(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let chars: Vec<char> = html.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        if c == '<' {
            if i + 7 <= len {
                let tag: String = chars[i..i + 7].iter().collect();
                let tag_lower = tag.to_lowercase();
                if tag_lower.starts_with("<script") {
                    in_script = true;
                } else if tag_lower.starts_with("<style") {
                    in_style = true;
                }
            }
            in_tag = true;
            i += 1;
            continue;
        }

        if c == '>' {
            if in_script {
                if i >= 8 {
                    let end_tag: String = chars[i - 8..=i].iter().collect();
                    if end_tag.to_lowercase() == "</script>" {
                        in_script = false;
                    }
                }
            } else if in_style {
                if i >= 7 {
                    let end_tag: String = chars[i - 7..=i].iter().collect();
                    if end_tag.to_lowercase() == "</style>" {
                        in_style = false;
                    }
                }
            }
            in_tag = false;
            i += 1;
            continue;
        }

        if !in_tag && !in_script && !in_style {
            if c == '&' {
                if i + 4 <= len {
                    let entity: String = chars[i..i + 4].iter().collect();
                    match entity.as_str() {
                        "&lt;" => {
                            result.push('<');
                            i += 4;
                            continue;
                        }
                        "&gt;" => {
                            result.push('>');
                            i += 4;
                            continue;
                        }
                        "&amp;" => {
                            result.push('&');
                            i += 5;
                            continue;
                        }
                        _ => {}
                    }
                }
                if i + 6 <= len {
                    let entity: String = chars[i..i + 6].iter().collect();
                    if entity == "&nbsp;" {
                        result.push(' ');
                        i += 6;
                        continue;
                    }
                }
            }
            result.push(c);
        }

        i += 1;
    }

    result
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn extract_title(html: &str) -> Option<String> {
    static TITLE_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = TITLE_RE.get_or_init(|| {
        Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("title regex should compile")
    });
    let captures = re.captures(html)?;
    let title = captures.get(1)?.as_str();
    let cleaned = strip_html(title).trim().to_string();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}
