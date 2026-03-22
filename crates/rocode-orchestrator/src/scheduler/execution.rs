use crate::iterative_workflow_runtime::WorkflowController;
use crate::ExecutionContext;
use crate::{OrchestratorContext, OrchestratorError, OrchestratorOutput};
use std::future::Future;

use super::execution_adapter::SchedulerExecutionCapabilityAdapter;
use super::execution_contracts::normalize_retry_focus;
use super::profile_state::SchedulerProfileState;
use super::{
    SchedulerExecutionGateDecision, SchedulerExecutionGateStatus,
    SchedulerExecutionVerificationMode, SchedulerInternalStageSpec, SchedulerProfileOrchestrator,
    SchedulerProfilePlan, SchedulerVerificationFailurePolicy, SchedulerVerificationFallback,
};

pub(super) struct SchedulerExecutionService<'a> {
    orchestrator: &'a SchedulerProfileOrchestrator,
    original_input: &'a str,
    state: &'a mut SchedulerProfileState,
    plan: &'a SchedulerProfilePlan,
    ctx: &'a OrchestratorContext,
}

impl<'a> SchedulerExecutionService<'a> {
    pub(super) fn new(
        orchestrator: &'a SchedulerProfileOrchestrator,
        original_input: &'a str,
        state: &'a mut SchedulerProfileState,
        plan: &'a SchedulerProfilePlan,
        ctx: &'a OrchestratorContext,
    ) -> Self {
        Self {
            orchestrator,
            original_input,
            state,
            plan,
            ctx,
        }
    }

    pub(super) async fn run_direct(&mut self) -> Result<(), OrchestratorError> {
        let workflow = self.plan.execution_workflow_policy();
        let adapter =
            SchedulerExecutionCapabilityAdapter::new(self.orchestrator, self.plan, self.ctx);
        let execution_input = self.orchestrator.compose_execution_orchestration_input(
            self.original_input,
            self.state,
            self.plan,
        );
        let output = adapter
            .execute_execution_path(
                &execution_input,
                workflow.child_mode,
                true,
                Some(Self::stage_context("execution", 1)),
                Some(super::SchedulerStageKind::ExecutionOrchestration),
            )
            .await?;
        SchedulerProfileOrchestrator::record_output(self.state, &output);
        self.state.execution.delegated = Some(output);
        Ok(())
    }

    pub(super) async fn run_single_pass(&mut self) -> Result<(), OrchestratorError> {
        let stage = self.plan.single_pass_executor_stage();
        let execution_input = self.orchestrator.compose_execution_orchestration_input(
            self.original_input,
            self.state,
            self.plan,
        );
        let prompt = self.resolve_single_pass_prompt(stage);
        let output = super::execute_stage_agent(
            &execution_input,
            self.ctx,
            super::stage_agent_unbounded(stage.agent_name, prompt),
            stage.tool_policy,
            Some(Self::stage_context("execution", 1)),
        )
        .await?;
        SchedulerProfileOrchestrator::record_output(self.state, &output);
        self.state.execution.delegated = Some(output);
        Ok(())
    }

    pub(super) async fn run_coordination_loop(&mut self) -> Result<(), OrchestratorError> {
        let workflow = self.plan.stage_execution_semantics().ok_or_else(|| {
            OrchestratorError::Other(self.plan.coordination_semantics_error().to_string())
        })?;
        let max_rounds = workflow.max_rounds.max(1) as usize;
        SchedulerProfileOrchestrator::sync_preset_runtime_authority(
            self.plan, self.state, self.ctx,
        );
        let mut execution_input = self.orchestrator.compose_execution_orchestration_input(
            self.original_input,
            self.state,
            self.plan,
        );

        for round in 1..=max_rounds {
            let execution_output = self
                .execute_execution_round(&execution_input, workflow.child_mode, true, None)
                .await?;
            SchedulerProfileOrchestrator::record_output(self.state, &execution_output);
            if self.state.is_cancelled {
                self.state.execution.delegated = Some(execution_output);
                break;
            }
            self.state.execution.delegated = Some(execution_output.clone());
            SchedulerProfileOrchestrator::sync_preset_runtime_authority(
                self.plan, self.state, self.ctx,
            );

            let verification_output = self
                .execute_coordination_verification(
                    round,
                    max_rounds,
                    &execution_output,
                    workflow.verification_mode,
                )
                .await?;

            let gate_input = self.orchestrator.compose_coordination_gate_input(
                self.original_input,
                self.state,
                self.plan,
                round,
                &execution_output,
                verification_output.as_ref(),
            );
            let (gate_output, decision) = self
                .execute_coordination_gate(round, max_rounds, &gate_input)
                .await?;
            SchedulerProfileOrchestrator::record_output(self.state, &gate_output);
            if self.state.is_cancelled {
                break;
            }

            let Some(decision) = decision else {
                tracing::warn!(
                    round,
                    "coordination gate returned no parseable decision; stopping after current round"
                );
                break;
            };

            match decision.status {
                SchedulerExecutionGateStatus::Done => {
                    if let Some(output) = SchedulerProfileOrchestrator::gate_terminal_output(
                        self.plan,
                        SchedulerExecutionGateStatus::Done,
                        &decision,
                        &execution_output,
                    ) {
                        self.state.place_execution_output(
                            self.plan.coordination_terminal_placement(),
                            output,
                        );
                    }
                    break;
                }
                SchedulerExecutionGateStatus::Blocked => {
                    if let Some(output) = SchedulerProfileOrchestrator::gate_terminal_output(
                        self.plan,
                        SchedulerExecutionGateStatus::Blocked,
                        &decision,
                        &execution_output,
                    ) {
                        self.state.place_execution_output(
                            self.plan.coordination_terminal_placement(),
                            output,
                        );
                    }
                    break;
                }
                SchedulerExecutionGateStatus::Continue if round < max_rounds => {
                    self.emit_retry_stage(
                        self.plan.coordination_retry_event(),
                        round,
                        max_rounds,
                        &decision,
                        verification_output.as_ref(),
                    )
                    .await;
                    SchedulerProfileOrchestrator::sync_preset_runtime_authority(
                        self.plan, self.state, self.ctx,
                    );
                    execution_input = self.orchestrator.compose_retry_input(
                        super::profile::RetryComposeRequest {
                            original_input: self.original_input,
                            state: self.state,
                            plan: self.plan,
                            round,
                            decision: &decision,
                            previous_output: &execution_output,
                            review_output: verification_output.as_ref(),
                        },
                    );
                }
                SchedulerExecutionGateStatus::Continue => {
                    self.state.place_execution_output(
                        self.plan.retry_exhausted_placement(),
                        SchedulerProfileOrchestrator::retry_budget_exhausted_output(
                            self.plan,
                            round,
                            max_rounds,
                            &decision,
                            &execution_output,
                            verification_output.as_ref(),
                        ),
                    );
                    break;
                }
            }
        }

        Ok(())
    }

    pub(super) async fn run_autonomous_loop(&mut self) -> Result<(), OrchestratorError> {
        let workflow = self.plan.stage_execution_semantics().ok_or_else(|| {
            OrchestratorError::Other(self.plan.autonomous_semantics_error().to_string())
        })?;
        let max_rounds = workflow.max_rounds.max(1) as usize;
        let mut workflow_controller = match self.plan.workflow.clone() {
            Some(workflow_config) => WorkflowController::from_config(
                workflow_config,
                self.orchestrator.tool_runner(),
                self.ctx.exec_ctx.clone(),
            )?,
            None => None,
        };
        if let Some(controller) = workflow_controller.as_mut() {
            controller.capture_baseline().await?;
        }
        let mut execution_input = self.orchestrator.compose_execution_orchestration_input(
            self.original_input,
            self.state,
            self.plan,
        );

        for round in 1..=max_rounds {
            let previous_output_for_retry = self
                .state
                .execution
                .delegated
                .as_ref()
                .cloned()
                .unwrap_or_else(OrchestratorOutput::empty);
            if let Some(controller) = workflow_controller.as_mut() {
                controller.begin_iteration(round as u32)?;
            }
            let workflow_exec_ctx = workflow_controller
                .as_ref()
                .and_then(|controller| controller.execution_context_override(&self.ctx.exec_ctx));
            let execution_output = match self
                .execute_execution_round(
                    &execution_input,
                    workflow.child_mode,
                    workflow.allow_execution_fallback,
                    workflow_exec_ctx.as_ref(),
                )
                .await
            {
                Ok(output) => output,
                Err(err) => {
                    if let Some(controller) = workflow_controller.as_mut() {
                        let result = controller.handle_execution_error(round as u32, &err);
                        let gate_output = result.output;
                        let decision = result.gate_decision;
                        SchedulerProfileOrchestrator::record_output(self.state, &gate_output);
                        if self.state.is_cancelled {
                            controller.orphan_active_checkpoint()?;
                            break;
                        }

                        match decision.status {
                            SchedulerExecutionGateStatus::Done => {
                                if let Some(output) =
                                    SchedulerProfileOrchestrator::gate_terminal_output(
                                        self.plan,
                                        SchedulerExecutionGateStatus::Done,
                                        &decision,
                                        &previous_output_for_retry,
                                    )
                                {
                                    self.state.place_execution_output(
                                        self.plan.autonomous_terminal_placement(),
                                        output,
                                    );
                                }
                                break;
                            }
                            SchedulerExecutionGateStatus::Blocked => {
                                if let Some(output) =
                                    SchedulerProfileOrchestrator::gate_terminal_output(
                                        self.plan,
                                        SchedulerExecutionGateStatus::Blocked,
                                        &decision,
                                        &previous_output_for_retry,
                                    )
                                {
                                    self.state.place_execution_output(
                                        self.plan.autonomous_terminal_placement(),
                                        output,
                                    );
                                }
                                break;
                            }
                            SchedulerExecutionGateStatus::Continue if round < max_rounds => {
                                self.emit_retry_stage(
                                    self.plan.autonomous_retry_event(),
                                    round,
                                    max_rounds,
                                    &decision,
                                    None,
                                )
                                .await;
                                execution_input = self.orchestrator.compose_retry_input(
                                    super::profile::RetryComposeRequest {
                                        original_input: self.original_input,
                                        state: self.state,
                                        plan: self.plan,
                                        round,
                                        decision: &decision,
                                        previous_output: &previous_output_for_retry,
                                        review_output: None,
                                    },
                                );
                                continue;
                            }
                            SchedulerExecutionGateStatus::Continue => {
                                self.state.place_execution_output(
                                    self.plan.retry_exhausted_placement(),
                                    SchedulerProfileOrchestrator::retry_budget_exhausted_output(
                                        self.plan,
                                        round,
                                        max_rounds,
                                        &decision,
                                        &previous_output_for_retry,
                                        None,
                                    ),
                                );
                                break;
                            }
                        }
                    }
                    return Err(err);
                }
            };
            SchedulerProfileOrchestrator::record_output(self.state, &execution_output);
            if self.state.is_cancelled {
                if let Some(controller) = workflow_controller.as_mut() {
                    controller.orphan_active_checkpoint()?;
                }
                self.state.execution.delegated = Some(execution_output);
                break;
            }
            self.state.execution.delegated = Some(execution_output.clone());

            let verification_result = self
                .execute_autonomous_verification(
                    round,
                    max_rounds,
                    &execution_output,
                    workflow_exec_ctx.as_ref(),
                )
                .await;
            let verification_output = match verification_result {
                Ok(output) => {
                    SchedulerProfileOrchestrator::record_output(self.state, &output);
                    if self.state.is_cancelled {
                        if let Some(controller) = workflow_controller.as_mut() {
                            controller.orphan_active_checkpoint()?;
                        }
                        break;
                    }
                    self.state.place_execution_output(
                        self.plan.autonomous_verification_placement(),
                        output.clone(),
                    );
                    Some(output)
                }
                Err(err)
                    if matches!(
                        self.plan.autonomous_verification_failure_policy(),
                        SchedulerVerificationFailurePolicy::Error
                    ) && Self::verification_required(self.plan) =>
                {
                    return Err(OrchestratorError::Other(format!(
                        "{} verification is required before finish gate: {err}",
                        self.plan.orchestrator.as_deref().unwrap_or("executor")
                    )));
                }
                Err(err) => {
                    tracing::warn!(error = %err, round, "autonomous verification stage failed; continuing to finish gate");
                    None
                }
            };

            let (gate_output, decision) = match workflow_controller.as_mut() {
                Some(controller) => {
                    self.execute_autonomous_workflow_gate(round, max_rounds, controller)
                        .await?
                }
                None => {
                    let gate_input = self.orchestrator.compose_autonomous_gate_input(
                        self.original_input,
                        self.state,
                        self.plan,
                        round,
                        &execution_output,
                        verification_output.as_ref(),
                    );
                    self.execute_autonomous_gate(round, max_rounds, &gate_input)
                        .await?
                }
            };
            SchedulerProfileOrchestrator::record_output(self.state, &gate_output);
            if self.state.is_cancelled {
                if let Some(controller) = workflow_controller.as_mut() {
                    controller.orphan_active_checkpoint()?;
                }
                break;
            }

            let Some(decision) = decision else {
                tracing::warn!(
                    round,
                    "autonomous gate returned no parseable decision; stopping after current round"
                );
                break;
            };

            match decision.status {
                SchedulerExecutionGateStatus::Done => {
                    if let Some(output) = SchedulerProfileOrchestrator::gate_terminal_output(
                        self.plan,
                        SchedulerExecutionGateStatus::Done,
                        &decision,
                        &execution_output,
                    ) {
                        self.state.place_execution_output(
                            self.plan.autonomous_terminal_placement(),
                            output,
                        );
                    }
                    break;
                }
                SchedulerExecutionGateStatus::Blocked => {
                    if let Some(output) = SchedulerProfileOrchestrator::gate_terminal_output(
                        self.plan,
                        SchedulerExecutionGateStatus::Blocked,
                        &decision,
                        &execution_output,
                    ) {
                        self.state.place_execution_output(
                            self.plan.autonomous_terminal_placement(),
                            output,
                        );
                    }
                    break;
                }
                SchedulerExecutionGateStatus::Continue if round < max_rounds => {
                    self.emit_retry_stage(
                        self.plan.autonomous_retry_event(),
                        round,
                        max_rounds,
                        &decision,
                        verification_output.as_ref(),
                    )
                    .await;
                    execution_input = self.orchestrator.compose_retry_input(
                        super::profile::RetryComposeRequest {
                            original_input: self.original_input,
                            state: self.state,
                            plan: self.plan,
                            round,
                            decision: &decision,
                            previous_output: &execution_output,
                            review_output: verification_output.as_ref(),
                        },
                    );
                }
                SchedulerExecutionGateStatus::Continue => {
                    self.state.place_execution_output(
                        self.plan.retry_exhausted_placement(),
                        SchedulerProfileOrchestrator::retry_budget_exhausted_output(
                            self.plan,
                            round,
                            max_rounds,
                            &decision,
                            &execution_output,
                            verification_output.as_ref(),
                        ),
                    );
                    break;
                }
            }
        }

        Ok(())
    }

    async fn execute_execution_round(
        &self,
        execution_input: &str,
        child_mode: super::SchedulerExecutionChildMode,
        allow_execution_fallback: bool,
        exec_ctx_override: Option<&ExecutionContext>,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let adapter = if let Some(exec_ctx) = exec_ctx_override.cloned() {
            SchedulerExecutionCapabilityAdapter::new(self.orchestrator, self.plan, self.ctx)
                .with_exec_ctx(exec_ctx)
        } else {
            SchedulerExecutionCapabilityAdapter::new(self.orchestrator, self.plan, self.ctx)
        };
        adapter
            .execute_execution_path(
                execution_input,
                child_mode,
                allow_execution_fallback,
                None,
                Some(super::SchedulerStageKind::ExecutionOrchestration),
            )
            .await
    }

    async fn execute_coordination_verification(
        &mut self,
        round: usize,
        total_rounds: usize,
        execution_output: &OrchestratorOutput,
        verification_mode: SchedulerExecutionVerificationMode,
    ) -> Result<Option<OrchestratorOutput>, OrchestratorError> {
        let stage = self.plan.coordination_verification_stage();
        let graph_verification_available =
            self.plan.agent_tree.is_some() && self.plan.skill_graph.is_some();
        let adapter =
            SchedulerExecutionCapabilityAdapter::new(self.orchestrator, self.plan, self.ctx);
        let stage_name = stage.event_name;
        let verification_output = if graph_verification_available {
            let verification_input = self.orchestrator.compose_coordination_verification_input(
                self.original_input,
                self.state,
                self.plan,
                round,
                execution_output,
            );
            Some(
                self.execute_internal_stage(
                    stage_name,
                    round,
                    total_rounds,
                    adapter.execute_skill_graph(
                        self.plan
                            .skill_graph
                            .as_ref()
                            .expect("graph verifier should exist"),
                        &verification_input,
                        Some(Self::stage_context(stage_name, round)),
                    ),
                )
                .await?,
            )
        } else if matches!(
            verification_mode,
            SchedulerExecutionVerificationMode::Required
        ) && matches!(
            self.plan.coordination_verification_fallback(),
            SchedulerVerificationFallback::ReviewStage
        ) {
            let verification_input = self.orchestrator.compose_coordination_verification_input(
                self.original_input,
                self.state,
                self.plan,
                round,
                execution_output,
            );
            Some(
                self.execute_internal_stage(
                    stage_name,
                    round,
                    total_rounds,
                    adapter.execute_review_stage(
                        &verification_input,
                        Some(Self::stage_context(stage_name, round)),
                    ),
                )
                .await?,
            )
        } else {
            None
        };

        if let Some(output) = &verification_output {
            SchedulerProfileOrchestrator::record_output(self.state, output);
            self.state.place_execution_output(
                self.plan.coordination_verification_placement(),
                output.clone(),
            );
        }

        Ok(verification_output)
    }

    async fn execute_coordination_gate(
        &self,
        round: usize,
        total_rounds: usize,
        gate_input: &str,
    ) -> Result<(OrchestratorOutput, Option<SchedulerExecutionGateDecision>), OrchestratorError>
    {
        let stage = self.plan.coordination_gate_stage();
        let stage_name = stage.event_name;
        let prompt = self.resolve_coordination_gate_prompt(stage);
        let output = self
            .execute_internal_stage(
                stage_name,
                round,
                total_rounds,
                super::execute_stage_agent(
                    gate_input,
                    self.ctx,
                    super::stage_agent_unbounded(stage.agent_name, prompt),
                    stage.tool_policy,
                    Some(Self::stage_context(stage_name, round)),
                ),
            )
            .await?;
        let decision = super::parse_execution_gate_decision(&output.content);
        Ok((output, decision))
    }

    async fn execute_autonomous_verification(
        &self,
        round: usize,
        total_rounds: usize,
        execution_output: &OrchestratorOutput,
        exec_ctx_override: Option<&ExecutionContext>,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let stage = self.plan.autonomous_verification_stage();
        let stage_name = stage.event_name;
        let verification_input = self.orchestrator.compose_autonomous_verification_input(
            self.original_input,
            self.state,
            self.plan,
            round,
            execution_output,
        );
        let prompt = self.resolve_autonomous_verification_prompt(stage);
        self.execute_internal_stage(
            stage_name,
            round,
            total_rounds,
            super::execute_stage_agent_with_exec_ctx(
                &verification_input,
                self.ctx,
                super::stage_agent_unbounded(stage.agent_name, prompt),
                stage.tool_policy,
                Some(Self::stage_context(stage_name, round)),
                exec_ctx_override.cloned(),
            ),
        )
        .await
    }

    async fn execute_autonomous_gate(
        &self,
        round: usize,
        total_rounds: usize,
        gate_input: &str,
    ) -> Result<(OrchestratorOutput, Option<SchedulerExecutionGateDecision>), OrchestratorError>
    {
        let stage = self.plan.autonomous_gate_stage();
        let stage_name = stage.event_name;
        let prompt = self.resolve_autonomous_gate_prompt(stage);
        let output = self
            .execute_internal_stage(
                stage_name,
                round,
                total_rounds,
                super::execute_stage_agent(
                    gate_input,
                    self.ctx,
                    super::stage_agent_unbounded(stage.agent_name, prompt),
                    stage.tool_policy,
                    Some(Self::stage_context(stage_name, round)),
                ),
            )
            .await?;
        let decision = super::parse_execution_gate_decision(&output.content);
        Ok((output, decision))
    }

    async fn execute_autonomous_workflow_gate(
        &self,
        round: usize,
        total_rounds: usize,
        controller: &mut WorkflowController,
    ) -> Result<(OrchestratorOutput, Option<SchedulerExecutionGateDecision>), OrchestratorError>
    {
        let stage = self.plan.autonomous_gate_stage();
        let stage_name = stage.event_name;
        self.emit_internal_stage_start(stage_name, round).await;
        match controller.evaluate_round(round as u32).await {
            Ok(result) => {
                self.emit_internal_stage_end(
                    stage_name,
                    round,
                    total_rounds,
                    &result.output.content,
                )
                .await;
                Ok((result.output, Some(result.gate_decision)))
            }
            Err(err) => {
                self.emit_internal_stage_end(
                    stage_name,
                    round,
                    total_rounds,
                    &format!("Stage error: {err}"),
                )
                .await;
                Err(err)
            }
        }
    }

    fn verification_required(plan: &SchedulerProfilePlan) -> bool {
        matches!(
            plan.stage_execution_semantics()
                .map(|workflow| workflow.verification_mode),
            Some(SchedulerExecutionVerificationMode::Required)
        )
    }

    fn stage_context(stage_name: &str, round: usize) -> (String, u32) {
        (stage_name.to_string(), round as u32)
    }

    fn resolve_single_pass_prompt(&self, stage: SchedulerInternalStageSpec) -> String {
        let profile_suffix = super::profile_prompt_suffix(
            self.plan,
            Some(super::SchedulerStageKind::ExecutionOrchestration),
        );
        self.plan
            .execution_orchestration_charter(&profile_suffix)
            .unwrap_or_else(|| (stage.fallback_prompt)(&profile_suffix))
    }

    fn resolve_coordination_gate_prompt(&self, stage: SchedulerInternalStageSpec) -> String {
        let profile_suffix = super::profile_prompt_suffix(
            self.plan,
            Some(super::SchedulerStageKind::ExecutionOrchestration),
        );
        self.plan
            .coordination_gate_prompt()
            .map(str::to_string)
            .unwrap_or_else(|| (stage.fallback_prompt)(&profile_suffix))
    }

    fn resolve_autonomous_verification_prompt(&self, stage: SchedulerInternalStageSpec) -> String {
        let profile_suffix = super::profile_prompt_suffix(
            self.plan,
            Some(super::SchedulerStageKind::ExecutionOrchestration),
        );
        self.plan
            .autonomous_verification_charter()
            .map(str::to_string)
            .unwrap_or_else(|| (stage.fallback_prompt)(&profile_suffix))
    }

    fn resolve_autonomous_gate_prompt(&self, stage: SchedulerInternalStageSpec) -> String {
        let profile_suffix = super::profile_prompt_suffix(
            self.plan,
            Some(super::SchedulerStageKind::ExecutionOrchestration),
        );
        self.plan
            .autonomous_gate_prompt()
            .map(str::to_string)
            .unwrap_or_else(|| (stage.fallback_prompt)(&profile_suffix))
    }

    async fn execute_internal_stage<F>(
        &self,
        stage_name: &'static str,
        round: usize,
        total_rounds: usize,
        future: F,
    ) -> Result<OrchestratorOutput, OrchestratorError>
    where
        F: Future<Output = Result<OrchestratorOutput, OrchestratorError>>,
    {
        self.emit_internal_stage_start(stage_name, round).await;
        match future.await {
            Ok(output) => {
                self.emit_internal_stage_end(stage_name, round, total_rounds, &output.content)
                    .await;
                Ok(output)
            }
            Err(err) => {
                self.emit_internal_stage_end(
                    stage_name,
                    round,
                    total_rounds,
                    &format!("Stage error: {err}"),
                )
                .await;
                Err(err)
            }
        }
    }

    async fn emit_internal_stage_start(&self, stage_name: &str, round: usize) {
        self.ctx
            .lifecycle_hook
            .on_scheduler_stage_start(
                &self.ctx.exec_ctx.agent_name,
                stage_name,
                round as u32,
                None,
                &self.ctx.exec_ctx,
            )
            .await;
    }

    async fn emit_internal_stage_end(
        &self,
        stage_name: &str,
        round: usize,
        total_rounds: usize,
        content: &str,
    ) {
        self.ctx
            .lifecycle_hook
            .on_scheduler_stage_end(
                &self.ctx.exec_ctx.agent_name,
                stage_name,
                round as u32,
                total_rounds as u32,
                content,
                &self.ctx.exec_ctx,
            )
            .await;
    }

    async fn emit_retry_stage(
        &self,
        stage_name: &'static str,
        round: usize,
        total_rounds: usize,
        decision: &SchedulerExecutionGateDecision,
        verification_output: Option<&OrchestratorOutput>,
    ) {
        self.emit_internal_stage_start(stage_name, round).await;
        self.emit_internal_stage_end(
            stage_name,
            round,
            total_rounds,
            &Self::format_retry_stage_content(decision, verification_output),
        )
        .await;
    }

    fn format_retry_stage_content(
        decision: &SchedulerExecutionGateDecision,
        verification_output: Option<&OrchestratorOutput>,
    ) -> String {
        let summary = decision.summary.trim();
        let focus = normalize_retry_focus(summary, decision.next_input.as_deref());
        let verification_note = verification_output
            .and_then(|output| Self::first_non_empty_line(&output.content))
            .filter(|line| {
                !line.eq_ignore_ascii_case(summary) && !line.eq_ignore_ascii_case(&focus)
            });

        let mut sections = Vec::new();
        sections.push(format!(
            "## Retry Focus\n{}",
            if focus.is_empty() {
                "Continue the bounded retry on the unresolved gap."
            } else {
                focus.as_str()
            }
        ));
        if !summary.is_empty() && summary != focus {
            sections.push(format!("## Gate Summary\n{summary}"));
        }
        if let Some(note) = verification_note {
            sections.push(format!("## Verification Note\n{note}"));
        }
        sections.join("\n\n")
    }

    fn first_non_empty_line(content: &str) -> Option<String> {
        content
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('-'))
            .map(str::to_string)
    }
}
