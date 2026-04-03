import { cn } from "@/lib/utils";

interface DecisionField {
  label?: string;
  value?: string;
  tone?: string;
}

interface DecisionSection {
  title?: string;
  body?: string;
}

interface SchedulerDecision {
  title?: string;
  fields?: DecisionField[];
  sections?: DecisionSection[];
}

interface SchedulerStageLike {
  feedId: string;
  kind: string;
  role?: string;
  stage_id?: string;
  tool_call_id?: string;
  title?: string;
  profile?: string;
  stage?: string;
  stage_index?: number;
  stage_total?: number;
  step?: number;
  status?: string;
  focus?: string;
  last_event?: string;
  waiting_on?: string;
  activity?: string;
  child_session_id?: string;
  active_skills?: string[];
  active_agents?: string[];
  active_categories?: string[];
  prompt_tokens?: number;
  completion_tokens?: number;
  reasoning_tokens?: number;
  cache_read_tokens?: number;
  cache_write_tokens?: number;
  decision?: SchedulerDecision | null;
  text?: string;
}

interface SchedulerStageCardProps {
  message: SchedulerStageLike;
  highlighted?: boolean;
  onNavigateStage: (stageId: string) => void;
  onNavigateChildSession: (
    sessionId: string,
    context?: { stageId?: string | null; toolCallId?: string | null; label?: string | null },
  ) => void;
}

function tokenSummary(message: SchedulerStageLike) {
  return [
    message.prompt_tokens ? `in ${message.prompt_tokens}` : null,
    message.completion_tokens ? `out ${message.completion_tokens}` : null,
    message.reasoning_tokens ? `reason ${message.reasoning_tokens}` : null,
  ].filter(Boolean);
}

export function SchedulerStageCard({
  message,
  highlighted = false,
  onNavigateStage,
  onNavigateChildSession,
}: SchedulerStageCardProps) {
  const chips = [
    message.profile,
    message.status,
    message.stage_index && message.stage_total
      ? `${message.stage_index}/${message.stage_total}`
      : null,
    typeof message.step === "number" ? `step ${message.step}` : null,
  ].filter(Boolean);
  const tokens = tokenSummary(message);

  return (
    <article
      className={cn(
        "rounded-2xl border border-border bg-card/70 p-4 grid gap-3.5 bg-gradient-to-b from-card/95 to-card/85",
        (message.role ?? "assistant") === "user" && "bg-primary/10",
        highlighted && "border-primary/30 shadow-[0_0_0_2px_var(--ring)] shadow-lg",
      )}
      data-testid="scheduler-stage-card"
      data-feed-id={message.feedId}
      data-stage-id={message.stage_id}
      data-child-session-id={message.child_session_id}
    >
      <header className="flex gap-2 items-center text-xs uppercase tracking-wider text-muted-foreground">
        <span>scheduler stage</span>
        <span>{message.role ?? "assistant"}</span>
      </header>
      <div className="flex flex-wrap gap-2.5 items-start justify-between">
        <div>
          <h3>{message.title || message.stage || "Scheduler Stage"}</h3>
          {message.focus ? <p className="text-sm text-muted-foreground italic">{message.focus}</p> : null}
          <div className="flex flex-wrap gap-2 mt-1">
            {message.stage_id ? (
              <button
                className="text-xs text-primary underline underline-offset-2 hover:text-primary/80 transition-colors"
                type="button"
                data-testid="scheduler-stage-open-stage"
                onClick={() => onNavigateStage(message.stage_id!)}
              >
                stage {message.stage_id}
              </button>
            ) : null}
            {message.child_session_id ? (
              <button
                className="text-xs text-primary underline underline-offset-2 hover:text-primary/80 transition-colors"
                type="button"
                data-testid="scheduler-stage-open-child"
                onClick={() =>
                  onNavigateChildSession(message.child_session_id!, {
                    stageId: message.stage_id ?? null,
                    toolCallId: message.tool_call_id ?? null,
                    label: message.title || message.stage || message.stage_id || message.child_session_id,
                  })
                }
              >
                child {message.child_session_id}
              </button>
            ) : null}
          </div>
        </div>
        <div className="flex flex-wrap gap-1.5">
          {chips.map((chip, index) => (
            <span key={`${message.feedId}-chip-${index}`} className="rounded-full border border-border bg-card/70 px-3 py-2">
              {chip}
            </span>
          ))}
        </div>
      </div>
      {(message.waiting_on || message.last_event || message.child_session_id) && (
        <dl className="mt-3 grid gap-2">
          {message.waiting_on ? (
            <div>
              <dt>Waiting</dt>
              <dd>{message.waiting_on}</dd>
            </div>
          ) : null}
          {message.last_event ? (
            <div>
              <dt>Last Event</dt>
              <dd>{message.last_event}</dd>
            </div>
          ) : null}
          {message.child_session_id ? (
            <div>
              <dt>Child Session</dt>
              <dd>
                <button
                  className="text-xs text-primary underline underline-offset-2 hover:text-primary/80 transition-colors"
                  type="button"
                  data-testid="scheduler-stage-open-child"
                    onClick={() =>
                      onNavigateChildSession(message.child_session_id!, {
                        stageId: message.stage_id ?? null,
                        toolCallId: message.tool_call_id ?? null,
                        label: message.title || message.stage || message.stage_id || message.child_session_id,
                      })
                    }
                >
                  {message.child_session_id}
                </button>
              </dd>
            </div>
          ) : null}
        </dl>
      )}
      {tokens.length ? (
        <div className="flex flex-wrap gap-1.5">
          {tokens.map((token) => (
            <span key={`${message.feedId}-${token}`} className="rounded-full border border-border bg-card/70 px-3 py-2">
              {token}
            </span>
          ))}
        </div>
      ) : null}
      {message.decision ? (
        <section className="rounded-xl border border-border bg-card/60 p-3 grid gap-2">
          {message.decision.title ? <h4>{message.decision.title}</h4> : null}
          {message.decision.fields?.length ? (
            <dl className="mt-3 grid gap-2">
              {message.decision.fields.map((field, index) => (
                <div key={`${message.feedId}-decision-field-${index}`}>
                  <dt>{field.label ?? "Field"}</dt>
                  <dd>{field.value ?? ""}</dd>
                </div>
              ))}
            </dl>
          ) : null}
          {message.decision.sections?.map((section, index) => (
            <details key={`${message.feedId}-decision-section-${index}`} className="rounded-lg border border-border bg-card/50 p-2.5" open>
              <summary>{section.title || `Section ${index + 1}`}</summary>
              <pre>{section.body || ""}</pre>
            </details>
          ))}
        </section>
      ) : null}
      {message.activity ? (
        <details className="rounded-lg border border-border bg-card/50 p-2.5" open>
          <summary>Activity</summary>
          <pre>{message.activity}</pre>
        </details>
      ) : null}
      {message.text?.trim() ? (
        <details className="rounded-lg border border-border bg-card/50 p-2.5">
          <summary>Raw Block</summary>
          <pre>{message.text}</pre>
        </details>
      ) : null}
      {(message.active_skills?.length || message.active_agents?.length || message.active_categories?.length) && (
        <div className="grid gap-2 text-sm text-muted-foreground">
          {message.active_skills?.length ? (
            <div>
              <span className="font-medium">Skills</span>
              <p>{message.active_skills.join(", ")}</p>
            </div>
          ) : null}
          {message.active_agents?.length ? (
            <div>
              <span className="font-medium">Agents</span>
              <p>{message.active_agents.join(", ")}</p>
            </div>
          ) : null}
          {message.active_categories?.length ? (
            <div>
              <span className="font-medium">Categories</span>
              <p>{message.active_categories.join(", ")}</p>
            </div>
          ) : null}
        </div>
      )}
    </article>
  );
}
