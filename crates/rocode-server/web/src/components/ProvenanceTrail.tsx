import type { BreadcrumbProvenance } from "../hooks/useSchedulerNavigation";

interface ProvenanceTrailProps {
  provenance: BreadcrumbProvenance | null;
  workspaceReference?: string | null;
  onNavigateSession: () => void;
  onNavigateStage: () => void;
  onNavigateToolCall: () => void;
}

export function ProvenanceTrail({
  provenance,
  workspaceReference = null,
  onNavigateSession,
  onNavigateStage,
  onNavigateToolCall,
}: ProvenanceTrailProps) {
  if (!provenance && !workspaceReference) return null;

  return (
    <div className="flex flex-wrap items-center gap-2" data-testid="provenance-trail">
      {provenance ? (
        <>
          <span className="text-muted-foreground text-sm">Source</span>
          <button
            className="text-xs text-primary underline underline-offset-2 hover:text-primary/80 transition-colors"
            data-testid="provenance-session"
            type="button"
            onClick={onNavigateSession}
          >
            {provenance.sourceSessionTitle}
          </button>
          {provenance.stageId ? (
            <button
              className="text-xs text-primary underline underline-offset-2 hover:text-primary/80 transition-colors"
              data-testid="provenance-stage"
              type="button"
              onClick={onNavigateStage}
            >
              stage {provenance.stageId}
            </button>
          ) : null}
          {provenance.toolCallId ? (
            <button
              className="text-xs text-primary underline underline-offset-2 hover:text-primary/80 transition-colors"
              data-testid="provenance-tool"
              type="button"
              onClick={onNavigateToolCall}
            >
              tool {provenance.toolCallId}
            </button>
          ) : null}
          {provenance.label ? <span className="text-xs text-muted-foreground">{provenance.label}</span> : null}
        </>
      ) : null}
      {workspaceReference ? (
        <span
          className="rounded-full border border-border bg-card/65 px-2.5 py-1.5 text-sm text-muted-foreground"
          data-testid="provenance-workspace"
        >
          @{workspaceReference}
        </span>
      ) : null}
    </div>
  );
}
