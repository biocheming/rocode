import { useEffect, useState } from "react";
import type { ConversationJumpTarget } from "../hooks/useConversationJump";
import type { useExecutionActivity } from "../hooks/useExecutionActivity";
import { cn } from "@/lib/utils";
import { StructuredDataView } from "./StructuredDataView";

type ExecutionActivityState = ReturnType<typeof useExecutionActivity>;

interface ExecutionActivityPanelProps {
  activity: ExecutionActivityState;
  activeStageId: string | null;
  previewStageId?: string | null;
  onJumpToConversation: (target: ConversationJumpTarget) => void;
  onNavigateStage: (stageId: string) => void;
  onNavigateChildSession: (
    sessionId: string,
    context?: { stageId?: string | null; toolCallId?: string | null; label?: string | null },
  ) => void;
  onNavigateToolCall: (
    toolCallId: string,
    context?: { executionId?: string | null; stageId?: string | null },
  ) => void;
}

function formatTs(ts?: number | null) {
  if (!ts) return "--";
  return new Date(ts).toLocaleTimeString();
}

function formatMoney(value?: number | null) {
  if (typeof value !== "number" || Number.isNaN(value)) return "--";
  return `$${value.toFixed(4)}`;
}

function eventWindowLabel(page: number, count: number, pageSize: number) {
  if (count === 0) return `page ${page} · items 0`;
  const start = (page - 1) * pageSize + 1;
  const end = start + count - 1;
  return `page ${page} · items ${start}-${end}`;
}

function stageStatusTone(status: ExecutionActivityState["stageSummaries"][number]["status"]) {
  switch (status) {
    case "running":
      return "bg-blue-500/10 text-blue-700 dark:text-blue-300";
    case "waiting":
    case "blocked":
    case "retrying":
      return "bg-amber-500/10 text-amber-700 dark:text-amber-300";
    case "done":
      return "bg-green-500/10 text-green-700 dark:text-green-300";
    case "cancelled":
    case "cancelling":
      return "bg-rose-500/10 text-rose-700 dark:text-rose-300";
    default:
      return "bg-muted text-muted-foreground";
  }
}

function stageSummaryMeta(stage: ExecutionActivityState["stageSummaries"][number]) {
  const parts: string[] = [];
  if (typeof stage.index === "number" && typeof stage.total === "number") {
    parts.push(`${stage.index}/${stage.total}`);
  }
  if (typeof stage.step === "number" && typeof stage.step_total === "number") {
    parts.push(`step ${stage.step}/${stage.step_total}`);
  }
  if (stage.waiting_on) {
    parts.push(`waiting ${stage.waiting_on}`);
  }
  if (typeof stage.retry_attempt === "number") {
    parts.push(`retry ${stage.retry_attempt}`);
  }
  if (stage.active_agent_count > 0) {
    parts.push(`agents ${stage.active_agent_count}`);
  }
  if (stage.active_tool_count > 0) {
    parts.push(`tools ${stage.active_tool_count}`);
  }
  if (stage.child_session_count > 0) {
    parts.push(`child ${stage.child_session_count}`);
  }
  if (typeof stage.skill_tree_budget === "number") {
    parts.push(
      `budget ${stage.skill_tree_budget}${stage.skill_tree_truncated ? " truncated" : ""}`,
    );
  }
  if (typeof stage.estimated_context_tokens === "number") {
    parts.push(`ctx ${stage.estimated_context_tokens}`);
  }
  return parts;
}

function metadataValue(record: Record<string, unknown> | null | undefined, key: string) {
  const value = record?.[key];
  return typeof value === "string" && value.trim() ? value : null;
}

function executionJumpTarget(node: ExecutionActivityState["selectedExecution"]) {
  if (!node) return null;
  const toolCallId = metadataValue(node.metadata, "tool_call_id");
  if (toolCallId) {
    return {
      toolCallId,
      executionId: node.id,
      stageId: node.stage_id,
      label: node.label || toolCallId,
    };
  }
  if (node.stage_id) {
    return {
      stageId: node.stage_id,
      executionId: node.id,
      label: node.label || node.stage_id,
    };
  }
  return null;
}

function eventJumpTarget(event: ExecutionActivityState["selectedEvent"]) {
  if (!event) return null;
  const payload = event.payload ?? {};
  const toolCallId =
    (typeof payload.tool_call_id === "string" && payload.tool_call_id) ||
    (typeof payload.id === "string" && payload.id.startsWith("call_") ? payload.id : null);
  return {
    toolCallId,
    executionId: event.execution_id ?? null,
    stageId: event.stage_id ?? null,
    label: event.event_type || "event",
  };
}

function eventChildSessionId(event: ExecutionActivityState["selectedEvent"]) {
  if (!event) return null;
  const payload = event.payload ?? {};
  return typeof payload.child_session_id === "string" && payload.child_session_id
    ? payload.child_session_id
    : null;
}

function ExecutionNodeTree({
  node,
  selectedExecutionId,
  activeStageId,
  previewStageId = null,
  onSelectExecution,
  onJumpToConversation,
}: {
  node: ExecutionActivityState["executionNodes"][number];
  selectedExecutionId: string | null;
  activeStageId: string | null;
  previewStageId?: string | null;
  onSelectExecution: (id: string) => void;
  onJumpToConversation: (target: ConversationJumpTarget) => void;
}) {
  const jumpTarget = executionJumpTarget(node);
  const stageClass =
    selectedExecutionId === node.id
      ? "active"
      : previewStageId && node.stage_id === previewStageId
        ? "stage-preview"
        : activeStageId && node.stage_id === activeStageId
          ? "stage-active"
          : "";

  return (
    <div className="pl-3 border-l-2 border-border/50">
      <div className="flex items-center gap-2">
        <button
          className={cn("flex items-center gap-2 px-2 py-1.5 rounded-lg border-0 bg-transparent text-foreground cursor-pointer text-sm w-full text-left hover:bg-accent", stageClass === "active" && "bg-primary/15 font-semibold", stageClass === "stage-preview" && "bg-amber-100/40 dark:bg-amber-900/20", stageClass === "stage-active" && "bg-primary/8")}
          type="button"
          onClick={() => onSelectExecution(node.id)}
        >
          <span className={cn("w-2.5 h-2.5 rounded-full shrink-0", node.status === "done" ? "bg-green-500" : node.status === "running" ? "bg-blue-500 animate-pulse" : node.status === "waiting" ? "bg-amber-400" : "bg-muted-foreground/40")} />
          <span className="text-xs text-muted-foreground font-mono">{node.kind}</span>
          <strong>{node.label || node.id}</strong>
        </button>
        {jumpTarget ? (
          <button
            className="text-xs text-primary underline underline-offset-2 cursor-pointer hover:text-primary/80 border-0 bg-transparent"
            type="button"
            onClick={() => onJumpToConversation(jumpTarget)}
          >
            Jump
          </button>
        ) : null}
      </div>
      {node.recent_event || node.waiting_on ? (
        <div className="text-xs text-muted-foreground pl-7 leading-relaxed">{node.recent_event || node.waiting_on}</div>
      ) : null}
      {node.children?.length ? (
        <div className="ml-3">
          {node.children.map((child) => (
            <ExecutionNodeTree
              key={child.id}
              node={child}
              selectedExecutionId={selectedExecutionId}
              activeStageId={activeStageId}
              previewStageId={previewStageId}
              onSelectExecution={onSelectExecution}
              onJumpToConversation={onJumpToConversation}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

export function ExecutionActivityPanel({
  activity,
  activeStageId,
  previewStageId = null,
  onJumpToConversation,
  onNavigateStage,
  onNavigateChildSession,
  onNavigateToolCall,
}: ExecutionActivityPanelProps) {
  const [pageDraft, setPageDraft] = useState(String(activity.activityPage));
  const executionJump = executionJumpTarget(activity.selectedExecution);
  const selectedEventJump = eventJumpTarget(activity.selectedEvent);
  const selectedEventChildSessionId = eventChildSessionId(activity.selectedEvent);
  const canCancelSelectedExecution =
    Boolean(activity.selectedExecution) &&
    activity.selectedExecution?.status !== "done" &&
    activity.executionCancellingId !== activity.selectedExecution?.id;

  useEffect(() => {
    setPageDraft(String(activity.activityPage));
  }, [activity.activityPage]);

  return (
    <div className="rounded-2xl border border-border bg-card/75 backdrop-blur-sm shadow-lg p-5 grid gap-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Scheduler</p>
          <h3>Execution + Activity</h3>
        </div>
        <button
          className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
          type="button"
          onClick={() =>
            void activity.refreshExecutionActivity(
              undefined,
              activity.activityFilters,
              activity.activityPage,
            )
          }
          disabled={activity.activityLoading}
        >
          {activity.activityLoading ? "Refreshing..." : "Refresh"}
        </button>
      </div>

      {activity.executionTopology ? (
        <>
          <div className="flex flex-wrap gap-2">
            <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">active {activity.executionTopology.active_count}</span>
            <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">running {activity.executionTopology.running_count}</span>
            <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">waiting {activity.executionTopology.waiting_count}</span>
            <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">retry {activity.executionTopology.retry_count ?? 0}</span>
            <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">cancelling {activity.executionTopology.cancelling_count ?? 0}</span>
            <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">done {activity.executionTopology.done_count}</span>
          </div>
          <p className="text-sm text-muted-foreground leading-relaxed">
            Updated {formatTs(activity.executionTopology.updated_at ?? undefined)}
          </p>
          {activity.sessionUsage ? (
            <div className="grid gap-3 md:grid-cols-2">
              <div className="rounded-2xl border border-border bg-background/70 p-4 grid gap-2">
                <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Session Usage</p>
                <div className="flex flex-wrap gap-2">
                  <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">input {activity.sessionUsage.input_tokens}</span>
                  <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">output {activity.sessionUsage.output_tokens}</span>
                  <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">reasoning {activity.sessionUsage.reasoning_tokens}</span>
                  <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">cache read {activity.sessionUsage.cache_read_tokens}</span>
                  <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">cache write {activity.sessionUsage.cache_write_tokens}</span>
                </div>
                <p className="text-sm text-muted-foreground leading-relaxed">Total cost {formatMoney(activity.sessionUsage.total_cost)}</p>
              </div>
              <div className="rounded-2xl border border-border bg-background/70 p-4 grid gap-2">
                <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Active Stage</p>
                {activity.activeStageSummary ? (
                  <>
                    <div className="flex flex-wrap items-center gap-2">
                      <strong>{activity.activeStageSummary.stage_name}</strong>
                      <span className="rounded-full border border-border bg-muted px-3 py-1 text-xs text-muted-foreground font-medium">{activity.activeStageSummary.status}</span>
                      {activity.sessionRuntime?.active_stage_count ? (
                        <span className="rounded-full border border-border bg-muted px-3 py-1 text-xs text-muted-foreground font-medium">active {activity.sessionRuntime.active_stage_count}</span>
                      ) : null}
                    </div>
                    <div className="flex flex-wrap gap-2">
                      {typeof activity.activeStageSummary.prompt_tokens === "number" ? (
                        <span className="rounded-full border border-border bg-muted px-3 py-1 text-xs text-muted-foreground font-medium">in {activity.activeStageSummary.prompt_tokens}</span>
                      ) : null}
                      {typeof activity.activeStageSummary.completion_tokens === "number" ? (
                        <span className="rounded-full border border-border bg-muted px-3 py-1 text-xs text-muted-foreground font-medium">out {activity.activeStageSummary.completion_tokens}</span>
                      ) : null}
                      {typeof activity.activeStageSummary.reasoning_tokens === "number" ? (
                        <span className="rounded-full border border-border bg-muted px-3 py-1 text-xs text-muted-foreground font-medium">reasoning {activity.activeStageSummary.reasoning_tokens}</span>
                      ) : null}
                      {typeof activity.activeStageSummary.skill_tree_budget === "number" ? (
                        <span className="rounded-full border border-border bg-muted px-3 py-1 text-xs text-muted-foreground font-medium">budget {activity.activeStageSummary.skill_tree_budget}</span>
                      ) : null}
                    </div>
                    <p className="text-sm text-muted-foreground leading-relaxed">
                      {activity.activeStageSummary.waiting_on
                        ? `Waiting on ${activity.activeStageSummary.waiting_on}`
                        : activity.activeStageSummary.last_event || "No active wait signal"}
                    </p>
                    {activity.activeStageSummary.skill_tree_truncated ? (
                      <p className="text-sm text-amber-700 dark:text-amber-300 leading-relaxed">
                        Skill tree truncated{activity.activeStageSummary.skill_tree_truncation_strategy
                          ? ` via ${activity.activeStageSummary.skill_tree_truncation_strategy}`
                          : ""}
                      </p>
                    ) : null}
                  </>
                ) : (
                  <p className="text-sm text-muted-foreground leading-relaxed">No active stage summary in telemetry.</p>
                )}
              </div>
            </div>
          ) : null}
        </>
      ) : (
        <p className="text-center text-muted-foreground py-4">No scheduler topology loaded yet.</p>
      )}

      {activity.stageSummaries.length ? (
        <div className="rounded-2xl border border-border bg-background/70 p-4 grid gap-3">
          <div className="flex items-center justify-between gap-3">
            <div>
              <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Stage Summaries</p>
              <h4>{activity.stageSummaries.length} stages</h4>
            </div>
            <p className="text-xs text-muted-foreground">
              Authority-backed telemetry snapshot
            </p>
          </div>
          <div className="grid gap-3 xl:grid-cols-2">
            {activity.stageSummaries.map((stage) => {
              const meta = stageSummaryMeta(stage);
              const isHighlighted =
                stage.stage_id === activity.sessionRuntime?.active_stage_id ||
                stage.stage_id === previewStageId;
              return (
                <div
                  key={stage.stage_id}
                  className={cn(
                    "rounded-xl border border-border bg-card/60 p-4 grid gap-3",
                    isHighlighted && "border-primary/40 bg-primary/5",
                  )}
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <strong className="truncate">{stage.stage_name}</strong>
                        <span
                          className={cn(
                            "rounded-full px-2.5 py-1 text-xs font-medium",
                            stageStatusTone(stage.status),
                          )}
                        >
                          {stage.status}
                        </span>
                      </div>
                      <p className="text-xs text-muted-foreground font-mono mt-1">
                        {stage.stage_id}
                      </p>
                    </div>
                    <div className="flex flex-wrap gap-2 shrink-0">
                      <button
                        className="min-h-[32px] rounded-full px-3 border border-border bg-card/70 text-foreground text-xs inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                        type="button"
                        onClick={() => onNavigateStage(stage.stage_id)}
                      >
                        Open
                      </button>
                      <button
                        className="min-h-[32px] rounded-full px-3 border border-border bg-card/70 text-foreground text-xs inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                        type="button"
                        onClick={() => activity.patchActivityFilters({ stageId: stage.stage_id })}
                      >
                        Filter Events
                      </button>
                    </div>
                  </div>
                  {meta.length ? (
                    <div className="flex flex-wrap gap-2">
                      {meta.map((item) => (
                        <span
                          key={`${stage.stage_id}:${item}`}
                          className="rounded-full border border-border bg-muted px-2.5 py-1 text-xs text-muted-foreground"
                        >
                          {item}
                        </span>
                      ))}
                    </div>
                  ) : null}
                  <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
                    {typeof stage.prompt_tokens === "number" ? <span>in {stage.prompt_tokens}</span> : null}
                    {typeof stage.completion_tokens === "number" ? <span>out {stage.completion_tokens}</span> : null}
                    {typeof stage.reasoning_tokens === "number" ? <span>reasoning {stage.reasoning_tokens}</span> : null}
                    {typeof stage.cache_read_tokens === "number" ? <span>cache read {stage.cache_read_tokens}</span> : null}
                    {typeof stage.cache_write_tokens === "number" ? <span>cache write {stage.cache_write_tokens}</span> : null}
                  </div>
                  {stage.last_event || stage.focus ? (
                    <div className="grid gap-1 text-xs text-muted-foreground">
                      {stage.last_event ? <p>Last event: {stage.last_event}</p> : null}
                      {stage.focus ? <p>Focus: {stage.focus}</p> : null}
                    </div>
                  ) : null}
                </div>
              );
            })}
          </div>
        </div>
      ) : null}

      <div className="grid grid-cols-[1fr_1fr_1fr_auto] gap-3 items-end">
        <label>
          <span>Stage</span>
          <select
            value={activity.activityFilters.stageId}
            onChange={(event) => activity.patchActivityFilters({ stageId: event.target.value })}
          >
            <option value="">all stages</option>
            {activity.stageOptions.map((stageId) => (
              <option key={stageId} value={stageId}>
                {stageId}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>Execution</span>
          <select
            value={activity.activityFilters.executionId}
            onChange={(event) => activity.patchActivityFilters({ executionId: event.target.value })}
          >
            <option value="">all executions</option>
            {activity.executionNodes.map((node) => (
              <option key={node.id} value={node.id}>
                {node.label || node.id}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>Event Type</span>
          <select
            value={activity.activityFilters.eventType}
            onChange={(event) => activity.patchActivityFilters({ eventType: event.target.value })}
          >
            <option value="">all events</option>
            {activity.knownEventTypes.map((eventType) => (
              <option key={eventType} value={eventType}>
                {eventType}
              </option>
            ))}
          </select>
        </label>
        <button className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent" type="button" onClick={activity.clearActivityFilters}>
          Clear
        </button>
      </div>

      <div className="max-h-64 overflow-auto flex flex-col gap-1">
        {activity.executionTopology?.roots.length ? (
          activity.executionTopology.roots.map((node) => (
            <ExecutionNodeTree
              key={node.id}
              node={node}
              selectedExecutionId={activity.selectedExecutionId}
              activeStageId={activeStageId}
              previewStageId={previewStageId}
              onSelectExecution={activity.setSelectedExecutionId}
              onJumpToConversation={onJumpToConversation}
            />
          ))
        ) : (
          <p className="text-center text-muted-foreground py-4">No active execution topology for this session.</p>
        )}
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <div className="rounded-xl border border-border bg-card/60 p-4 grid gap-3">
          <div className="flex items-start justify-between gap-3">
            <div>
              <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Execution</p>
              <h4>{activity.selectedExecution?.label || "Select an execution node"}</h4>
            </div>
            <div className="flex flex-wrap gap-2">
              {executionJump ? (
                <button
                  className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                  type="button"
                  onClick={() => onJumpToConversation(executionJump)}
                >
                  Jump to Message
                </button>
              ) : null}
              {activity.selectedExecution ? (
                <button
                  className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                  type="button"
                  disabled={!canCancelSelectedExecution}
                  onClick={() => void activity.cancelExecution(activity.selectedExecution?.id || undefined)}
                >
                  {activity.executionCancellingId === activity.selectedExecution.id
                    ? "Cancelling..."
                    : "Cancel"}
                </button>
              ) : null}
            </div>
          </div>
          {activity.selectedExecution ? (
            <>
              <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
                <div>
                  <dt>ID</dt>
                  <dd>{activity.selectedExecution.id}</dd>
                </div>
                <div>
                  <dt>Status</dt>
                  <dd>{activity.selectedExecution.status}</dd>
                </div>
                <div>
                  <dt>Stage</dt>
                  <dd>
                    {activity.selectedExecution.stage_id ? (
                      <button
                        className="text-xs text-primary underline underline-offset-2 cursor-pointer hover:text-primary/80 border-0 bg-transparent p-0"
                        type="button"
                        onClick={() => onNavigateStage(activity.selectedExecution.stage_id || "")}
                      >
                        {activity.selectedExecution.stage_id}
                      </button>
                    ) : (
                      "--"
                    )}
                  </dd>
                </div>
                <div>
                  <dt>Updated</dt>
                  <dd>{formatTs(activity.selectedExecution.updated_at)}</dd>
                </div>
              </dl>
              <div className="flex flex-wrap gap-2">
                <button
                  className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                  type="button"
                  onClick={() => activity.patchActivityFilters({ executionId: activity.selectedExecution?.id || "" })}
                >
                  Filter Events to Execution
                </button>
                {activity.selectedExecution.stage_id ? (
                <button
                  className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                  type="button"
                  onClick={() =>
                    activity.patchActivityFilters({
                      stageId: activity.selectedExecution?.stage_id || "",
                    })
                  }
                >
                    Filter Events to Stage
                  </button>
                ) : null}
              </div>
              <StructuredDataView
                value={activity.selectedExecution.metadata}
                emptyLabel="No execution metadata for this node."
              />
            </>
          ) : (
            <p className="text-center text-muted-foreground py-4">Choose a node to inspect its metadata and provenance.</p>
          )}
        </div>

        <div className="rounded-xl border border-border bg-card/60 p-4 grid gap-3">
          <div className="flex items-start justify-between gap-3">
            <div>
              <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Activity</p>
              <h4>{activity.selectedEvent?.event_type || "Recent events"}</h4>
            </div>
            {selectedEventJump ? (
              <button
                className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                type="button"
                onClick={() => onJumpToConversation(selectedEventJump)}
              >
                Jump to Provenance
              </button>
            ) : null}
          </div>
          {activity.selectedEvent ? (
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
              {activity.selectedEvent.stage_id ? (
                <div>
                  <dt>Stage</dt>
                  <dd>
                    <button
                      className="text-xs text-primary underline underline-offset-2 cursor-pointer hover:text-primary/80 border-0 bg-transparent p-0"
                      type="button"
                      onClick={() => onNavigateStage(activity.selectedEvent?.stage_id || "")}
                    >
                      {activity.selectedEvent.stage_id}
                    </button>
                  </dd>
                </div>
              ) : null}
              {selectedEventChildSessionId ? (
                <div>
                  <dt>Child Session</dt>
                  <dd>
                    <button
                      className="text-xs text-primary underline underline-offset-2 cursor-pointer hover:text-primary/80 border-0 bg-transparent p-0"
                      type="button"
                      onClick={() =>
                        onNavigateChildSession(selectedEventChildSessionId, {
                          stageId: activity.selectedEvent?.stage_id ?? null,
                          toolCallId: selectedEventJump?.toolCallId ?? null,
                          label: activity.selectedEvent?.event_type || selectedEventChildSessionId,
                        })
                      }
                    >
                      {selectedEventChildSessionId}
                    </button>
                  </dd>
                </div>
              ) : null}
              {selectedEventJump?.toolCallId ? (
                <div>
                  <dt>Tool Call</dt>
                  <dd>
                    <button
                      className="text-xs text-primary underline underline-offset-2 cursor-pointer hover:text-primary/80 border-0 bg-transparent p-0"
                      type="button"
                      onClick={() =>
                        onNavigateToolCall(selectedEventJump.toolCallId!, {
                          executionId: selectedEventJump.executionId,
                          stageId: selectedEventJump.stageId,
                        })
                      }
                    >
                      {selectedEventJump.toolCallId}
                    </button>
                  </dd>
                </div>
              ) : null}
            </dl>
          ) : null}
          <div className="max-h-64 overflow-auto flex flex-col gap-1">
            {activity.activityEvents.length ? (
              activity.activityEvents.map((event, index) => (
                <button
                  key={event.event_id || `${event.ts || "event"}:${event.event_type || index}`}
                  className={cn("flex flex-col gap-1 px-3 py-2 rounded-lg border-0 bg-transparent text-foreground cursor-pointer text-sm text-left w-full hover:bg-accent", activity.selectedEventId === event.event_id ? "bg-primary/15 font-semibold" : previewStageId && event.stage_id === previewStageId ? "bg-amber-100/40 dark:bg-amber-900/20" : "")}
                  type="button"
                  onClick={() => activity.setSelectedEventId(event.event_id || null)}
                >
                  <div className="flex items-center justify-between gap-2">
                    <strong>{event.event_type || "event"}</strong>
                    <span>{formatTs(event.ts)}</span>
                  </div>
                  {event.summary ? <p>{event.summary}</p> : null}
                  {event.stage_id || event.execution_id ? (
                    <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
                      {event.stage_id ? <span>stage {event.stage_id}</span> : null}
                      {event.execution_id ? <span>exec {event.execution_id}</span> : null}
                    </div>
                  ) : null}
                </button>
              ))
            ) : (
              <p className="text-center text-muted-foreground py-4">No recent activity events for this filter.</p>
            )}
          </div>
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-border bg-background/60 px-3 py-2">
            <p className="text-xs text-muted-foreground">
              {eventWindowLabel(
                activity.activityPage,
                activity.activityEvents.length,
                activity.activityPageSize,
              )}{" "}
              · limit {activity.activityPageSize}
            </p>
            <div className="flex flex-wrap items-center gap-2">
              <button
                className="min-h-[32px] rounded-full px-3 border border-border bg-card/70 text-foreground text-xs inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
                type="button"
                disabled={!activity.activityHasPreviousPage}
                onClick={activity.firstActivityPage}
              >
                First
              </button>
              <button
                className="min-h-[32px] rounded-full px-3 border border-border bg-card/70 text-foreground text-xs inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
                type="button"
                disabled={!activity.activityHasPreviousPage}
                onClick={activity.previousActivityPage}
              >
                Prev
              </button>
              <label className="flex items-center gap-2 text-xs text-muted-foreground">
                <span>Page</span>
                <input
                  className="h-8 w-16 rounded-md border border-input bg-transparent px-2 py-1 text-sm text-foreground"
                  type="number"
                  min={1}
                  step={1}
                  value={pageDraft}
                  onChange={(event) => setPageDraft(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      const page = Number.parseInt(pageDraft, 10);
                      activity.goToActivityPage(Number.isFinite(page) ? page : 1);
                    }
                  }}
                />
              </label>
              <button
                className="min-h-[32px] rounded-full px-3 border border-border bg-card/70 text-foreground text-xs inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                type="button"
                onClick={() => {
                  const page = Number.parseInt(pageDraft, 10);
                  activity.goToActivityPage(Number.isFinite(page) ? page : 1);
                }}
              >
                Go
              </button>
              <button
                className="min-h-[32px] rounded-full px-3 border border-border bg-card/70 text-foreground text-xs inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
                type="button"
                disabled={!activity.activityHasNextPage}
                onClick={activity.nextActivityPage}
              >
                Next
              </button>
            </div>
          </div>
          {activity.selectedEvent ? (
            <>
              <div className="flex flex-wrap gap-2">
                {activity.selectedEvent.execution_id ? (
                  <button
                    className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                    type="button"
                    onClick={() =>
                      activity.patchActivityFilters({ executionId: activity.selectedEvent?.execution_id || "" })
                    }
                  >
                    Filter to Execution
                  </button>
                ) : null}
                {activity.selectedEvent.stage_id ? (
                  <button
                    className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                    type="button"
                    onClick={() =>
                      activity.patchActivityFilters({ stageId: activity.selectedEvent?.stage_id || "" })
                    }
                  >
                    Filter to Stage
                  </button>
                ) : null}
                {selectedEventChildSessionId ? (
                  <button
                  className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                  type="button"
                  onClick={() =>
                    onNavigateChildSession(selectedEventChildSessionId, {
                      stageId: activity.selectedEvent?.stage_id ?? null,
                      toolCallId: selectedEventJump?.toolCallId ?? null,
                      label: activity.selectedEvent?.event_type || selectedEventChildSessionId,
                    })
                  }
                >
                  Open Child Session
                </button>
                ) : null}
                {selectedEventJump?.toolCallId ? (
                  <button
                    className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                    type="button"
                    onClick={() =>
                      onNavigateToolCall(selectedEventJump.toolCallId!, {
                        executionId: selectedEventJump.executionId,
                        stageId: selectedEventJump.stageId,
                      })
                    }
                  >
                    Open Tool Call
                  </button>
                ) : null}
              </div>
              <StructuredDataView
                value={{
                  scope: activity.selectedEvent.scope,
                  stage_id: activity.selectedEvent.stage_id,
                  child_session_id: selectedEventChildSessionId,
                  execution_id: activity.selectedEvent.execution_id,
                  tool_call_id: selectedEventJump?.toolCallId ?? null,
                  payload: activity.selectedEvent.payload,
                }}
                emptyLabel="No structured payload for this event."
                onNavigateKeyValue={(key, value) => {
                  if (key === "stage_id") onNavigateStage(value);
                  if (key === "child_session_id") {
                    onNavigateChildSession(value, {
                      stageId: activity.selectedEvent?.stage_id ?? null,
                      toolCallId: selectedEventJump?.toolCallId ?? null,
                      label: activity.selectedEvent?.event_type || value,
                    });
                  }
                  if (key === "tool_call_id") {
                    onNavigateToolCall(value, {
                      executionId: selectedEventJump?.executionId,
                      stageId: selectedEventJump?.stageId,
                    });
                  }
                }}
              />
            </>
          ) : null}
        </div>
      </div>
    </div>
  );
}
