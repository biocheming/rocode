use crate::output_blocks::SchedulerStageBlock;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct SchedulerStageGovernanceFixture {
    pub block: SchedulerStageBlock,
    pub payload: Value,
    pub metadata: HashMap<String, Value>,
    pub message_text: String,
}

pub fn canonical_scheduler_stage_fixture() -> SchedulerStageGovernanceFixture {
    serde_json::from_str(include_str!("../governance/scheduler_stage_fixture.json"))
        .expect("valid canonical scheduler stage governance fixture")
}
