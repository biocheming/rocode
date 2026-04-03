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
  const executionJump = executionJumpTarget(activity.selectedExecution);
  const selectedEventJump = eventJumpTarget(activity.selectedEvent);
  const selectedEventChildSessionId = eventChildSessionId(activity.selectedEvent);
  const canCancelSelectedExecution =
    Boolean(activity.selectedExecution) &&
    activity.selectedExecution?.status !== "done" &&
    activity.executionCancellingId !== activity.selectedExecution?.id;

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
          onClick={() => void activity.refreshExecutionActivity()}
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
        </>
      ) : (
        <p className="text-center text-muted-foreground py-4">No scheduler topology loaded yet.</p>
      )}

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
                    onClick={() => onNavigateStage(activity.selectedExecution?.stage_id || "")}
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
                    onClick={() => onNavigateStage(activity.selectedEvent?.stage_id || "")}
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
