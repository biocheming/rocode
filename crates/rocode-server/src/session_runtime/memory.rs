use std::sync::Arc;

use anyhow::Result;
use rocode_command::stage_protocol::StageSummary;
use rocode_memory::{
    MemoryAuthority, ResolvedMemoryContext, SkillWriteObservation, ToolMemoryObservation,
};
use rocode_types::{MemoryRetrievalPacket, MemoryRetrievalQuery, Session, SkillGuardReport};

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct RuntimeMemoryAuthority {
    memory: Arc<MemoryAuthority>,
}

#[allow(dead_code)]
impl RuntimeMemoryAuthority {
    pub(crate) fn new(memory: Arc<MemoryAuthority>) -> Self {
        Self { memory }
    }

    pub(crate) fn memory(&self) -> Arc<MemoryAuthority> {
        self.memory.clone()
    }

    pub(crate) async fn resolve_context(&self) -> Result<ResolvedMemoryContext> {
        self.memory.resolve_context().await
    }

    pub(crate) async fn build_frozen_snapshot(&self) -> Result<MemoryRetrievalPacket> {
        self.memory.build_frozen_snapshot().await
    }

    pub(crate) async fn build_prefetch_packet(
        &self,
        query: &MemoryRetrievalQuery,
    ) -> Result<MemoryRetrievalPacket> {
        self.memory.build_prefetch_packet(query).await
    }

    pub(crate) async fn ingest_session_record(&self, session: &Session) -> Result<()> {
        let _ = self.memory.ingest_session_record(session).await?;
        Ok(())
    }

    pub(crate) async fn ingest_stage_summaries(
        &self,
        session_id: &str,
        summaries: &[StageSummary],
    ) -> Result<()> {
        for summary in summaries {
            let _ = self
                .memory
                .ingest_stage_summary_observation(session_id, summary)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn ingest_tool_result(
        &self,
        session_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        stage_id: Option<&str>,
        output: &str,
        is_error: bool,
    ) -> Result<()> {
        let _ = self
            .memory
            .ingest_tool_result_observation(&ToolMemoryObservation {
                session_id,
                tool_call_id,
                tool_name,
                stage_id,
                output,
                is_error,
            })
            .await?;
        Ok(())
    }

    pub(crate) async fn ingest_skill_manage_result(
        &self,
        session_id: &str,
        tool_call_id: &str,
        metadata: Option<&serde_json::Value>,
    ) -> Result<()> {
        let Some(metadata) = metadata else {
            return Ok(());
        };
        let Some(skill_name) = metadata.get("name").and_then(|value| value.as_str()) else {
            return Ok(());
        };
        let action = metadata
            .get("action")
            .and_then(|value| value.as_str())
            .unwrap_or("update");
        let location = metadata.get("location").and_then(|value| value.as_str());
        let supporting_file = metadata.get("file_path").and_then(|value| value.as_str());
        let guard_report = metadata
            .get("guard_report")
            .cloned()
            .and_then(|value| serde_json::from_value::<SkillGuardReport>(value).ok());

        let _ = self
            .memory
            .ingest_skill_write_observation(&SkillWriteObservation {
                session_id,
                tool_call_id: Some(tool_call_id),
                skill_name,
                action,
                location,
                supporting_file,
                guard_report: guard_report.as_ref(),
            })
            .await?;
        Ok(())
    }
}
