use rocode_provider::{Protocol, ProviderConfig};

#[test]
fn test_deepseek_uses_openai_protocol() {
    let protocol = Protocol::from_npm("@ai-sdk/openai-compatible");
    assert_eq!(protocol, Protocol::OpenAI);
}

#[test]
fn test_custom_anthropic_endpoint() {
    let protocol = Protocol::from_npm("@ai-sdk/anthropic");
    assert_eq!(protocol, Protocol::Anthropic);

    let config = ProviderConfig::new(
        "bailian",
        "https://coding.dashscope.aliyuncs.com/api/v1/messages",
        "sk-sp-xxx",
    );

    assert_eq!(
        config.base_url,
        "https://coding.dashscope.aliyuncs.com/api/v1/messages"
    );
}

#[test]
fn test_openrouter_custom_headers() {
    let protocol = Protocol::from_npm("@openrouter/ai-sdk-provider");
    assert_eq!(protocol, Protocol::OpenAI);

    let config = ProviderConfig::new(
        "openrouter",
        "https://openrouter.ai/api/v1/chat/completions",
        "sk-or-xxx",
    )
    .with_header("HTTP-Referer", "https://opencode.ai/")
    .with_header("X-Title", "opencode");

    assert_eq!(
        config.headers.get("HTTP-Referer").expect("referer header"),
        "https://opencode.ai/"
    );
}
