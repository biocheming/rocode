use rocode_agent::AgentExecutor;
use rocode_command::agent_presenter::{present_agent_outcome, AgentPresenterConfig};
use rocode_command::output_blocks::OutputBlock;

pub(crate) struct StreamRenderStats {
    pub(crate) prompt_tokens: u64,
    pub(crate) completion_tokens: u64,
}

pub(crate) async fn stream_prompt_to_blocks<F>(
    executor: &mut AgentExecutor,
    prompt: &str,
    mut emit: F,
) -> anyhow::Result<StreamRenderStats>
where
    F: FnMut(OutputBlock) -> anyhow::Result<()>,
{
    let outcome = executor.execute_rendered(prompt.to_string()).await?;
    let presented = present_agent_outcome(outcome, AgentPresenterConfig::default());

    for block in presented.blocks {
        emit(block)?;
    }

    if let Some(error) = presented.stream_error {
        return Err(anyhow::anyhow!("Agent stream failure: {}", error));
    }

    Ok(StreamRenderStats {
        prompt_tokens: presented.prompt_tokens,
        completion_tokens: presented.completion_tokens,
    })
}

pub(crate) async fn stream_prompt_to_text(
    executor: &mut AgentExecutor,
    prompt: &str,
) -> anyhow::Result<String> {
    executor
        .execute_text_response(prompt.to_string())
        .await
        .map_err(|err| anyhow::anyhow!("{}", err))
}
