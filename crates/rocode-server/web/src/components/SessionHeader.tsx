import type { BreadcrumbProvenance, SessionBreadcrumb } from "../hooks/useSchedulerNavigation";
import { ProvenanceTrail } from "./ProvenanceTrail";

interface SessionHeaderProps {
  title: string;
  subtitle?: string | null;
  pathLabel?: string | null;
  workspaceLabel?: string | null;
  contextSummary?: string | null;
  contextTitle?: string | null;
  modeLabel?: string | null;
  modelLabel?: string | null;
  activeStageId: string | null;
  currentWorkspaceReference?: string | null;
  breadcrumbs: SessionBreadcrumb[];
  provenance: BreadcrumbProvenance | null;
  onNavigateStage: (stageId: string) => void;
  onNavigateBreadcrumb: (sessionId: string) => void;
  onNavigateProvenanceSession: () => void;
  onNavigateProvenanceStage: () => void;
  onNavigateProvenanceToolCall: () => void;
}

export function SessionHeader({
  title,
  subtitle = null,
  pathLabel = null,
  workspaceLabel = null,
  contextSummary = null,
  contextTitle = null,
  modeLabel = null,
  modelLabel = null,
  activeStageId,
  currentWorkspaceReference = null,
  breadcrumbs,
  provenance,
  onNavigateStage,
  onNavigateBreadcrumb,
  onNavigateProvenanceSession,
  onNavigateProvenanceStage,
  onNavigateProvenanceToolCall,
}: SessionHeaderProps) {
  const showTrace = breadcrumbs.length > 1 || Boolean(provenance);
  const secondaryMeta = subtitle?.trim() || null;

  return (
    <header className="roc-session-header grid gap-2" data-testid="session-header">
      <div className="flex flex-col gap-1.5 xl:flex-row xl:items-center xl:justify-between">
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 flex-wrap items-center gap-2.5">
            <span className="text-[10px] font-semibold uppercase tracking-[0.24em] text-muted-foreground">
              Session
            </span>
            <h1 className="min-w-0 truncate text-[1.15rem] font-semibold tracking-[-0.03em] text-foreground md:text-[1.28rem]">
              {title}
            </h1>
          </div>
          {secondaryMeta ? (
            <div className="mt-0.5 text-[12px] leading-5 text-muted-foreground">
              <span className="truncate">{secondaryMeta}</span>
            </div>
          ) : null}
        </div>

        <div className="flex shrink-0 flex-wrap items-center gap-1.5 xl:justify-end">
          {contextSummary ? (
            <span className="roc-chip-subtle" title={contextTitle || contextSummary}>
              {contextSummary}
            </span>
          ) : null}
          {activeStageId ? (
            <button
              className="rounded-full border border-primary/25 bg-primary/10 px-3 py-1.5 text-xs font-semibold tracking-tight text-foreground transition-colors hover:border-primary/40 hover:bg-primary/15"
              type="button"
              onClick={() => onNavigateStage(activeStageId)}
            >
              stage {activeStageId}
            </button>
          ) : null}
          {modeLabel ? <span className="roc-chip-subtle">{modeLabel}</span> : null}
          {modelLabel ? <span className="roc-chip-subtle">{modelLabel}</span> : null}
        </div>
      </div>

      {breadcrumbs.length > 1 ? (
        <nav
          className="flex flex-wrap gap-2"
          data-testid="session-breadcrumbs"
          aria-label="Session breadcrumbs"
        >
          {breadcrumbs.map((crumb, index) => (
            <div
              key={`${crumb.sessionId}:${index}`}
              className="inline-flex items-center gap-2 rounded-full border border-border/60 bg-background/66 px-3 py-1.5"
            >
              <button
                className="border-0 bg-transparent p-0 text-sm text-foreground transition-colors hover:text-primary"
                type="button"
                data-testid="session-breadcrumb"
                data-session-id={crumb.sessionId}
                onClick={() => onNavigateBreadcrumb(crumb.sessionId)}
              >
                {crumb.title}
              </button>
              {crumb.viaLabel ? (
                <span className="text-xs text-muted-foreground">
                  {crumb.viaLabel}
                </span>
              ) : null}
            </div>
          ))}
        </nav>
      ) : null}

      {showTrace ? (
        <div className="border-t border-border/50 pt-3">
          <ProvenanceTrail
            provenance={provenance}
            workspaceReference={provenance ? currentWorkspaceReference : null}
            onNavigateSession={onNavigateProvenanceSession}
            onNavigateStage={onNavigateProvenanceStage}
            onNavigateToolCall={onNavigateProvenanceToolCall}
          />
        </div>
      ) : null}
    </header>
  );
}
