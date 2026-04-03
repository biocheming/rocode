import type { BreadcrumbProvenance, SessionBreadcrumb } from "../hooks/useSchedulerNavigation";
import { ProvenanceTrail } from "./ProvenanceTrail";
import { cn } from "@/lib/utils";

interface SessionHeaderProps {
  title: string;
  subtitle: string;
  theme: string;
  mode: string;
  model: string;
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
  subtitle,
  theme,
  mode,
  model,
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
  return (
    <header
      className="flex items-center justify-between gap-4"
      data-testid="session-header"
    >
      <div className="grid gap-2">
        <p className="m-0 mb-1.5 text-xs tracking-widest uppercase text-amber-700 font-bold">
          Current Session
        </p>
        <h2 className="m-0 font-serif">{title}</h2>
        <p className="mt-2 leading-relaxed text-muted-foreground">{subtitle}</p>
        {breadcrumbs.length > 1 ? (
          <nav
            className="flex flex-wrap gap-2"
            data-testid="session-breadcrumbs"
            aria-label="Session breadcrumbs"
          >
            {breadcrumbs.map((crumb, index) => (
              <div
                key={`${crumb.sessionId}:${index}`}
                className="inline-flex items-center gap-2 rounded-full border border-border bg-card/70 px-2.5 py-1.5"
              >
                <button
                  className="border-0 bg-transparent text-inherit cursor-pointer font-inherit p-0"
                  type="button"
                  data-testid="session-breadcrumb"
                  data-session-id={crumb.sessionId}
                  onClick={() => onNavigateBreadcrumb(crumb.sessionId)}
                >
                  {crumb.title}
                </button>
                {crumb.viaLabel ? (
                  <span className="text-muted-foreground text-[0.82rem]">
                    {crumb.viaLabel}
                  </span>
                ) : null}
              </div>
            ))}
          </nav>
        ) : null}
        <ProvenanceTrail
          provenance={provenance}
          workspaceReference={currentWorkspaceReference}
          onNavigateSession={onNavigateProvenanceSession}
          onNavigateStage={onNavigateProvenanceStage}
          onNavigateToolCall={onNavigateProvenanceToolCall}
        />
      </div>
      <div className="flex flex-wrap gap-2">
        {activeStageId ? (
          <button
            className="border-0 bg-primary/10 text-inherit rounded-full px-2.5 py-1.5 cursor-pointer font-bold"
            type="button"
            onClick={() => onNavigateStage(activeStageId)}
          >
            stage {activeStageId}
          </button>
        ) : null}
        <span className="rounded-full border border-border bg-card/70 px-3 py-2">
          theme {theme}
        </span>
        <span className="rounded-full border border-border bg-card/70 px-3 py-2">
          mode {mode || "auto"}
        </span>
        <span className="rounded-full border border-border bg-card/70 px-3 py-2">
          model {model || "auto"}
        </span>
      </div>
    </header>
  );
}
