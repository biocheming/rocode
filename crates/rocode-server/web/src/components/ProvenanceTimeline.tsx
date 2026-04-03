"use client";

import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import {
  ClockIcon,
  LoaderCircleIcon,
  ChevronRightIcon,
  CopyIcon,
  CheckIcon,
  ArrowRightIcon,
  PlayIcon,
  CheckCircleIcon,
  XCircleIcon,
  MessageSquareIcon,
  WrenchIcon,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { apiJson } from "../lib/api";

interface StageEvent {
  event_id?: string;
  event_type: string;
  execution_id?: string;
  stage_id?: string;
  ts?: number;
  time?: string;
  scope?: string;
  payload?: Record<string, unknown>;
  [key: string]: unknown;
}

interface ProvenanceTimelineProps {
  sessionId: string;
  onNavigateStage?: (stageId: string) => void;
  onNavigateSession?: (sessionId: string) => void;
  className?: string;
}

function formatTimestamp(ts: number): string {
  const date = new Date(ts);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMins = Math.floor(diffMs / 60000);

  if (diffMins < 1) return "just now";
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffMins < 1440) return `${Math.floor(diffMins / 60)}h ago`;
  return date.toLocaleDateString();
}

function getEventIcon(eventType: string) {
  if (eventType.includes("start") || eventType.includes("begin")) {
    return <PlayIcon className="size-3.5" />;
  }
  if (eventType.includes("complete") || eventType.includes("done")) {
    return <CheckCircleIcon className="size-3.5 text-green-500" />;
  }
  if (eventType.includes("fail") || eventType.includes("error")) {
    return <XCircleIcon className="size-3.5 text-red-500" />;
  }
  if (eventType.includes("message") || eventType.includes("user")) {
    return <MessageSquareIcon className="size-3.5" />;
  }
  if (eventType.includes("tool") || eventType.includes("call")) {
    return <WrenchIcon className="size-3.5" />;
  }
  return <ClockIcon className="size-3.5" />;
}

function getEventLabel(eventType: string): string {
  return eventType
    .replace(/^execution\./, "")
    .replace(/^stage\./, "")
    .replace(/\./g, " ")
    .replace(/_/g, " ")
    .split(" ")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

function EventCard({
  event,
  onNavigateStage,
  onNavigateSession,
}: {
  event: StageEvent;
  onNavigateStage?: (stageId: string) => void;
  onNavigateSession?: (sessionId: string) => void;
}) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    await navigator.clipboard.writeText(JSON.stringify(event, null, 2));
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }, [event]);

  return (
    <div className="relative pl-6 pb-4">
      {/* Timeline line */}
      <div className="absolute left-[9px] top-2 bottom-0 w-px bg-border" />

      {/* Timeline dot */}
      <div className="absolute left-0 top-1.5 w-5 h-5 rounded-full bg-background border-2 border-primary flex items-center justify-center">
        <div className="w-2 h-2 rounded-full bg-primary" />
      </div>

      {/* Event card */}
      <div className="rounded-lg border bg-card/80 p-3 ml-4 hover:bg-card transition-colors">
        {/* Header */}
        <div className="flex items-center justify-between gap-2 mb-2">
          <div className="flex items-center gap-2">
            <span className="text-muted-foreground">
              {getEventIcon(event.event_type)}
            </span>
            <span className="font-medium text-sm">
              {getEventLabel(event.event_type)}
            </span>
          </div>
          <div className="flex items-center gap-1">
            {event.ts && (
              <span className="text-xs text-muted-foreground">
                {formatTimestamp(event.ts)}
              </span>
            )}
            <Button
              variant="ghost"
              size="icon-sm"
              className="size-6"
              onClick={handleCopy}
            >
              {copied ? (
                <CheckIcon className="size-3" />
              ) : (
                <CopyIcon className="size-3" />
              )}
            </Button>
          </div>
        </div>

        {/* IDs */}
        <div className="flex flex-wrap gap-2 text-xs">
          {event.stage_id && (
            <button
              className="flex items-center gap-1 px-2 py-1 rounded-full bg-primary/10 text-primary hover:bg-primary/20 transition-colors"
              onClick={() => onNavigateStage?.(event.stage_id!)}
              title="Navigate to stage"
            >
              <span className="font-mono">{event.stage_id.slice(0, 8)}</span>
              <ChevronRightIcon className="size-3" />
            </button>
          )}
          {event.execution_id && (
            <span className="flex items-center gap-1 px-2 py-1 rounded-full bg-muted text-muted-foreground font-mono">
              {event.execution_id.slice(0, 8)}
            </span>
          )}
        </div>

        {/* Scope badge */}
        {event.scope && (
          <div className="mt-2">
            <span className="text-xs px-2 py-0.5 rounded-full bg-muted text-muted-foreground">
              {event.scope}
            </span>
          </div>
        )}

        {/* Payload preview */}
        {event.payload && Object.keys(event.payload).length > 0 && (
          <div className="mt-2 text-xs text-muted-foreground">
            {JSON.stringify(event.payload).slice(0, 100)}
            {JSON.stringify(event.payload).length > 100 && "..."}
          </div>
        )}
      </div>
    </div>
  );
}

export function ProvenanceTimeline({
  sessionId,
  onNavigateStage,
  onNavigateSession,
  className,
}: ProvenanceTimelineProps) {
  const [events, setEvents] = useState<StageEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadEvents = useCallback(async () => {
    if (!sessionId) return;
    try {
      const data = await apiJson<StageEvent[]>(
        `/session/${sessionId}/events?limit=50`
      );
      setEvents(data.reverse()); // Most recent first
      setError(null);
    } catch (err) {
      setError(`Failed to load events: ${err}`);
    } finally {
      setLoading(false);
    }
  }, [sessionId]);

  useEffect(() => {
    loadEvents();
    // Poll for updates every 5 seconds
    const interval = setInterval(loadEvents, 5000);
    return () => clearInterval(interval);
  }, [loadEvents]);

  const handleExport = useCallback(() => {
    const markdown = events
      .map((e) => {
        const ts = e.ts ? new Date(e.ts).toISOString() : "";
        return `- **[${getEventLabel(e.event_type)}]** ${ts}\n  - stage: \`${e.stage_id || "N/A"}\`\n  - execution: \`${e.execution_id || "N/A"}\``;
      })
      .join("\n");

    const content = `# Session Provenance\n\nSession: ${sessionId}\nExported: ${new Date().toISOString()}\n\n## Events\n\n${markdown}`;

    const blob = new Blob([content], { type: "text/markdown" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `provenance-${sessionId.slice(0, 8)}.md`;
    a.click();
    URL.revokeObjectURL(url);
  }, [events, sessionId]);

  return (
    <div className={cn("flex flex-col h-full overflow-hidden", className)}>
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-muted/30">
        <div className="flex items-center gap-2">
          <ClockIcon className="size-4" />
          <h3 className="font-medium text-sm">Provenance</h3>
          <span className="text-xs text-muted-foreground">({events.length})</span>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon-sm"
            className="size-7"
            onClick={() => {
              setRefreshing(true);
              void loadEvents().then(() => setRefreshing(false));
            }}
            disabled={refreshing}
          >
            <LoaderCircleIcon className={cn("size-4", refreshing && "animate-spin")} />
          </Button>
          {events.length > 0 && (
            <Button
              variant="ghost"
              size="icon-sm"
              className="size-7"
              onClick={handleExport}
              title="Export as Markdown"
            >
              <ArrowRightIcon className="size-4" />
            </Button>
          )}
        </div>
      </div>

      {/* Error message */}
      {error && (
        <div className="mx-3 mt-3 p-3 rounded-lg bg-destructive/10 text-destructive text-sm">
          {error}
        </div>
      )}

      {/* Events timeline */}
      <div className="flex-1 overflow-auto p-3">
        {loading ? (
          <div className="flex items-center justify-center py-8 text-muted-foreground">
            <LoaderCircleIcon className="size-5 animate-spin mr-2" />
            Loading events...
          </div>
        ) : events.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-8 text-muted-foreground text-sm">
            <ClockIcon className="size-8 opacity-30 mb-2" />
            <p>No events recorded</p>
            <p className="text-xs mt-1">Events will appear as the session runs</p>
          </div>
        ) : (
          <div className="space-y-0">
            {events.map((event, idx) => (
              <EventCard
                key={event.event_id || idx}
                event={event}
                onNavigateStage={onNavigateStage}
                onNavigateSession={onNavigateSession}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
