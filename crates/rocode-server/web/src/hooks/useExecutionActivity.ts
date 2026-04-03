import { useCallback, useEffect, useMemo, useRef, useState } from "react";

export interface ExecutionNodeRecord {
  id: string;
  kind: string;
  status: string;
  label?: string;
  parent_id?: string;
  stage_id?: string;
  waiting_on?: string;
  recent_event?: string;
  started_at?: number;
  updated_at?: number;
  metadata?: Record<string, unknown> | null;
  children?: ExecutionNodeRecord[];
}

export interface SessionExecutionTopologyRecord {
  active_count: number;
  running_count: number;
  waiting_count: number;
  cancelling_count?: number;
  retry_count?: number;
  done_count: number;
  updated_at?: number | null;
  roots: ExecutionNodeRecord[];
}

export interface ActivityEventRecord {
  event_id?: string;
  scope?: string;
  ts?: number;
  event_type?: string;
  stage_id?: string | null;
  execution_id?: string | null;
  summary?: string | null;
  payload?: Record<string, unknown> | null;
}

export interface ActivityFilters {
  stageId: string;
  executionId: string;
  eventType: string;
}

interface UseExecutionActivityOptions {
  selectedSessionId: string | null;
  apiJson: <T>(path: string, options?: RequestInit) => Promise<T>;
  onError: (message: string) => void;
  onInfo: (message: string) => void;
}

const DEFAULT_FILTERS: ActivityFilters = {
  stageId: "",
  executionId: "",
  eventType: "",
};

function formatError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return "Unknown error";
}

function executionActivityQuery(filters: ActivityFilters) {
  const search = new URLSearchParams();
  search.set("limit", "24");
  if (filters.stageId) search.set("stage_id", filters.stageId);
  if (filters.executionId) search.set("execution_id", filters.executionId);
  if (filters.eventType) search.set("event_type", filters.eventType);
  return search.toString();
}

async function loadExecutionActivityData(
  selectedSessionId: string,
  filters: ActivityFilters,
  apiJson: <T>(path: string, options?: RequestInit) => Promise<T>,
) {
  const query = executionActivityQuery(filters);
  const [topology, events] = await Promise.all([
    apiJson<SessionExecutionTopologyRecord>(`/session/${selectedSessionId}/executions`),
    apiJson<ActivityEventRecord[]>(`/session/${selectedSessionId}/events?${query}`),
  ]);
  return { topology, events };
}

function flattenExecutionNodes(nodes: ExecutionNodeRecord[]): ExecutionNodeRecord[] {
  return nodes.flatMap((node) => [node, ...flattenExecutionNodes(node.children ?? [])]);
}

function uniqStrings(values: Array<string | null | undefined>) {
  return Array.from(new Set(values.filter((value): value is string => Boolean(value && value.trim()))));
}

function sameActivityFilters(left: ActivityFilters, right: ActivityFilters) {
  return (
    left.stageId === right.stageId &&
    left.executionId === right.executionId &&
    left.eventType === right.eventType
  );
}

export function useExecutionActivity({
  selectedSessionId,
  apiJson,
  onError,
  onInfo,
}: UseExecutionActivityOptions) {
  const [executionTopology, setExecutionTopology] = useState<SessionExecutionTopologyRecord | null>(null);
  const [activityEvents, setActivityEvents] = useState<ActivityEventRecord[]>([]);
  const [activityLoading, setActivityLoading] = useState(false);
  const [activityFilters, setActivityFilters] = useState<ActivityFilters>(DEFAULT_FILTERS);
  const [selectedExecutionId, setSelectedExecutionId] = useState<string | null>(null);
  const [selectedEventId, setSelectedEventId] = useState<string | null>(null);
  const [knownEventTypes, setKnownEventTypes] = useState<string[]>([]);
  const [executionCancellingId, setExecutionCancellingId] = useState<string | null>(null);
  const sessionRef = useRef<string | null>(selectedSessionId);
  const previousSessionRef = useRef<string | null>(selectedSessionId);
  const filtersRef = useRef<ActivityFilters>(DEFAULT_FILTERS);

  useEffect(() => {
    sessionRef.current = selectedSessionId;
  }, [selectedSessionId]);

  useEffect(() => {
    if (previousSessionRef.current === selectedSessionId) return;
    previousSessionRef.current = selectedSessionId;
    setActivityFilters(DEFAULT_FILTERS);
    setSelectedExecutionId(null);
    setSelectedEventId(null);
    setKnownEventTypes([]);
  }, [selectedSessionId]);

  useEffect(() => {
    filtersRef.current = activityFilters;
  }, [activityFilters]);

  const resetExecutionActivity = useCallback(() => {
    setExecutionTopology(null);
    setActivityEvents([]);
    setActivityLoading(false);
    setActivityFilters(DEFAULT_FILTERS);
    setSelectedExecutionId(null);
    setSelectedEventId(null);
    setKnownEventTypes([]);
    setExecutionCancellingId(null);
  }, []);

  const refreshExecutionActivity = useCallback(
    async (sessionId = sessionRef.current, filters = filtersRef.current) => {
      if (!sessionId) {
        resetExecutionActivity();
        return;
      }

      setActivityLoading(true);
      try {
        const { topology, events } = await loadExecutionActivityData(sessionId, filters, apiJson);
        if (sessionRef.current !== sessionId) return;
        setExecutionTopology(topology);
        setActivityEvents(events);
        setKnownEventTypes((current) =>
          uniqStrings([...current, ...events.map((event) => event.event_type)]).sort(),
        );
      } catch (error) {
        if (sessionRef.current === sessionId) {
          onError(`Failed to load execution activity: ${formatError(error)}`);
        }
      } finally {
        if (sessionRef.current === sessionId) {
          setActivityLoading(false);
        }
      }
    },
    [apiJson, onError, resetExecutionActivity],
  );

  useEffect(() => {
    if (!selectedSessionId) {
      resetExecutionActivity();
      return;
    }
    void refreshExecutionActivity(selectedSessionId, activityFilters);
  }, [activityFilters, refreshExecutionActivity, resetExecutionActivity, selectedSessionId]);

  const executionNodes = useMemo(
    () => flattenExecutionNodes(executionTopology?.roots ?? []),
    [executionTopology?.roots],
  );

  const selectedExecution = useMemo(
    () => executionNodes.find((node) => node.id === selectedExecutionId) ?? null,
    [executionNodes, selectedExecutionId],
  );

  const selectedEvent = useMemo(
    () => activityEvents.find((event) => event.event_id === selectedEventId) ?? null,
    [activityEvents, selectedEventId],
  );

  const stageOptions = useMemo(
    () =>
      uniqStrings([
        ...executionNodes.map((node) => node.stage_id),
        ...activityEvents.map((event) => event.stage_id ?? undefined),
        activityFilters.stageId,
        selectedExecution?.stage_id,
        selectedEvent?.stage_id ?? undefined,
      ]).sort(),
    [activityEvents, activityFilters.stageId, executionNodes, selectedEvent?.stage_id, selectedExecution?.stage_id],
  );

  useEffect(() => {
    if (selectedExecutionId && !executionNodes.some((node) => node.id === selectedExecutionId)) {
      setSelectedExecutionId(null);
    }
  }, [executionNodes, selectedExecutionId]);

  useEffect(() => {
    if (selectedEventId && !activityEvents.some((event) => event.event_id === selectedEventId)) {
      setSelectedEventId(null);
    }
  }, [activityEvents, selectedEventId]);

  const patchActivityFilters = useCallback((patch: Partial<ActivityFilters>) => {
    setSelectedEventId(null);
    setActivityFilters((current) => {
      const next = { ...current, ...patch };
      return sameActivityFilters(current, next) ? current : next;
    });
  }, []);

  const clearActivityFilters = useCallback(() => {
    setSelectedEventId(null);
    setActivityFilters((current) =>
      sameActivityFilters(current, DEFAULT_FILTERS) ? current : DEFAULT_FILTERS,
    );
  }, []);

  const cancelExecution = useCallback(
    async (executionId = selectedExecutionId, sessionId = sessionRef.current) => {
      if (!sessionId || !executionId) return;
      setExecutionCancellingId(executionId);
      try {
        const response = await apiJson<{ cancelled?: boolean; error?: string }>(
          `/session/${sessionId}/executions/${encodeURIComponent(executionId)}/cancel`,
          { method: "POST" },
        );
        if (!response.cancelled) {
          throw new Error(response.error || "execution not found");
        }
        onInfo(`Cancelling ${executionId}`);
        await refreshExecutionActivity(sessionId);
      } catch (error) {
        onError(`Failed to cancel execution: ${formatError(error)}`);
      } finally {
        setExecutionCancellingId((current) => (current === executionId ? null : current));
      }
    },
    [apiJson, onError, onInfo, refreshExecutionActivity, selectedExecutionId],
  );

  return {
    executionTopology,
    activityEvents,
    activityLoading,
    activityFilters,
    knownEventTypes,
    stageOptions,
    executionNodes,
    selectedExecutionId,
    selectedExecution,
    executionCancellingId,
    selectedEventId,
    selectedEvent,
    setSelectedExecutionId,
    setSelectedEventId,
    patchActivityFilters,
    clearActivityFilters,
    cancelExecution,
    refreshExecutionActivity,
  };
}
