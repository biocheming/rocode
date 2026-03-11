use rocode_agent::AgentExecutor;
use rocode_command::agent_presenter::{present_agent_outcome, AgentPresenterConfig};
use rocode_command::output_blocks::OutputBlock;
use tokio_util::sync::CancellationToken;

pub(crate) struct StreamRenderStats {
    pub(crate) prompt_tokens: u64,
    pub(crate) completion_tokens: u64,
}

pub(crate) async fn stream_prompt_to_blocks_with_cancel<F>(
    executor: &mut AgentExecutor,
    prompt: &str,
    cancel_token: CancellationToken,
    mut emit: F,
) -> anyhow::Result<StreamRenderStats>
where
    F: FnMut(OutputBlock) -> anyhow::Result<()>,
{
    let outcome = executor
        .execute_rendered_with_cancel_token(prompt.to_string(), cancel_token)
        .await?;
    present_outcome_to_blocks(outcome, &mut emit)
}

fn present_outcome_to_blocks<F>(
    outcome: rocode_agent::AgentRenderOutcome,
    emit: &mut F,
) -> anyhow::Result<StreamRenderStats>
where
    F: FnMut(OutputBlock) -> anyhow::Result<()>,
{
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
