"use client";

import { cn } from "@/lib/utils";
import { SchedulerStageCard } from "./SchedulerStageCard";
import {
  Message,
  MessageContent,
  MessageToolbar,
  MessageActions,
  MessageAction,
  MessageResponse,
} from "./ai-elements/message";
import { Button } from "@/components/ui/button";
import {
  CopyIcon,
  CheckIcon,
  ActivityIcon,
  ChevronDownIcon,
  BrainCircuitIcon,
} from "lucide-react";
import { useState, useCallback } from "react";
import type { FeedMessage, OutputBlock } from "../lib/history";

interface MessageCardProps {
  message: FeedMessage;
  highlighted?: boolean;
  activeStageId?: string | null;
  activeToolCallId?: string | null;
  onNavigateStage: (stageId: string) => void;
  onNavigateChildSession: (
    sessionId: string,
    context?: { stageId?: string | null; toolCallId?: string | null; label?: string | null },
  ) => void;
}

function ReasoningBlock({ text }: { text: string }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="rounded-xl border border-border/70 bg-muted/30 px-3 py-2">
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 text-xs font-medium text-muted-foreground hover:text-foreground transition-colors"
      >
        <BrainCircuitIcon className="size-3.5 shrink-0" />
        <span>Reasoning</span>
        <ChevronDownIcon
          className={cn(
            "ml-auto size-3.5 shrink-0 transition-transform duration-200",
            expanded && "rotate-180"
          )}
        />
      </button>
      <div
        className={cn(
          "relative overflow-hidden transition-all duration-200 mt-2",
          expanded ? "max-h-[2000px]" : "max-h-20"
        )}
      >
        <p className="text-xs text-muted-foreground whitespace-pre-wrap">{text}</p>
        {!expanded && (
          <div className="pointer-events-none absolute bottom-0 left-0 right-0 h-6 bg-gradient-to-t from-muted/60 to-transparent rounded-b-xl" />
        )}
      </div>
    </div>
  );
}

function StatusBlock({ message }: { message: OutputBlock }) {
  const isError = message.tone === "error";

  return (
    <div
      className={cn(
        "flex items-start gap-2 rounded-lg px-3 py-2 text-xs",
        isError
          ? "bg-destructive/10 text-destructive"
          : "bg-muted/50 text-muted-foreground"
      )}
    >
      <ActivityIcon className="mt-0.5 size-3.5 shrink-0" />
      <span>{message.text}</span>
    </div>
  );
}

function ToolBlock({ message }: { message: OutputBlock }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="rounded-xl border border-border/50 bg-muted/20 px-3 py-2">
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 text-xs text-muted-foreground hover:text-foreground transition-colors"
      >
        <CheckIcon className="size-3.5 shrink-0 text-emerald-600" />
        <span className="font-medium">{message.title || message.kind}</span>
        <ChevronDownIcon
          className={cn(
            "ml-auto size-3.5 shrink-0 transition-transform duration-200",
            expanded && "rotate-180"
          )}
        />
      </button>
      {expanded && message.text && (
        <div className="mt-2 text-xs text-muted-foreground">
          <MessageResponse>{message.text}</MessageResponse>
        </div>
      )}
    </div>
  );
}

function InfoBlock({ message }: { message: OutputBlock }) {
  return (
    <div className="rounded-xl border border-border/50 bg-muted/20 px-3 py-2">
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <ActivityIcon className="size-3.5 shrink-0" />
        <span className="font-medium">{message.title || "Info"}</span>
      </div>
      {message.text ? (
        <p className="mt-2 text-sm text-foreground whitespace-pre-wrap">{message.text}</p>
      ) : null}
      {message.fields?.length ? (
        <dl className="mt-3 grid gap-1.5 text-sm">
          {message.fields.map((field, index) => (
            <div
              key={`${message.id ?? message.text}-field-${index}`}
              className="grid grid-cols-[100px_1fr] gap-3 py-1.5 border-b border-border/30 last:border-0"
            >
              <dt className="font-medium text-muted-foreground text-xs">
                {field.label ?? "Field"}
              </dt>
              <dd className="text-foreground text-xs whitespace-pre-wrap">
                {String(field.value ?? "")}
              </dd>
            </div>
          ))}
        </dl>
      ) : null}
    </div>
  );
}

export function MessageCard({
  message,
  highlighted = false,
  activeStageId = null,
  activeToolCallId = null,
  onNavigateStage,
  onNavigateChildSession,
}: MessageCardProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    await navigator.clipboard.writeText(message.text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }, [message.text]);

  if (message.kind === "scheduler_stage") {
    return (
      <SchedulerStageCard
        message={message}
        highlighted={highlighted || Boolean(activeStageId && message.stage_id === activeStageId)}
        onNavigateStage={onNavigateStage}
        onNavigateChildSession={onNavigateChildSession}
      />
    );
  }

  // Reasoning blocks get a special collapsible treatment
  if (message.kind === "reasoning") {
    if (!message.text.trim()) {
      return null;
    }
    return <ReasoningBlock text={message.text} />;
  }

  // Status messages (including errors)
  if (message.kind === "status") {
    return <StatusBlock message={message} />;
  }

  // Tool blocks get a compact collapsible
  if (message.kind === "tool") {
    return <ToolBlock message={message} />;
  }

  if (message.kind === "multimodal_info") {
    return <InfoBlock message={message} />;
  }

  const role = message.role ?? "assistant";
  const isUser = role === "user";

  return (
    <article
      className={cn(
        "group/message",
        highlighted && "ring-2 ring-primary/30 ring-offset-2 ring-offset-background rounded-xl",
        activeStageId && message.stage_id === activeStageId && "ring-2 ring-amber-500/30 ring-offset-2 ring-offset-background rounded-xl",
      )}
      data-testid="message-card"
      data-feed-id={message.feedId}
      data-block-id={message.id}
      data-stage-id={message.stage_id}
      data-kind={message.kind}
    >
      <Message from={isUser ? "user" : "assistant"}>
        <MessageContent>
          {/* Main content with Streamdown markdown rendering */}
          {message.text ? (
            <MessageResponse>{message.text}</MessageResponse>
          ) : null}

          {/* Fields */}
          {message.fields?.length ? (
            <dl className="mt-3 grid gap-1.5 text-sm">
              {message.fields.map((field, index) => (
                <div
                  key={`${message.feedId}-field-${index}`}
                  className="grid grid-cols-[100px_1fr] gap-3 py-1.5 border-b border-border/30 last:border-0"
                >
                  <dt className="font-medium text-muted-foreground text-xs">{field.label ?? "Field"}</dt>
                  <dd className="text-foreground text-xs">{String(field.value ?? "")}</dd>
                </div>
              ))}
            </dl>
          ) : null}
        </MessageContent>

        {/* Toolbar — only for assistant messages with content */}
        {!isUser && message.text ? (
          <MessageToolbar>
            <MessageActions>
              <MessageAction
                tooltip={copied ? "Copied!" : "Copy"}
                onClick={handleCopy}
                label={copied ? "Copied" : "Copy message"}
              >
                {copied ? <CheckIcon className="size-4" /> : <CopyIcon className="size-4" />}
              </MessageAction>
            </MessageActions>
          </MessageToolbar>
        ) : null}
      </Message>
    </article>
  );
}
