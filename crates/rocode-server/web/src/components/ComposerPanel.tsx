"use client";

import type { FormEvent, ClipboardEvent, DragEvent } from "react";
import type { BreadcrumbProvenance } from "../hooks/useSchedulerNavigation";
import { AttachmentDetailsPanel } from "./AttachmentDetailsPanel";
import { ComposerContextStrip } from "./ComposerContextStrip";
import type { ComposerAttachmentLike } from "../lib/composerContext";
import { cn } from "@/lib/utils";
import {
  PlusIcon,
  SendIcon,
  CommandIcon,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

interface ComposerPanelProps {
  composer: string;
  composerDragActive: boolean;
  streaming: boolean;
  references: string[];
  attachments: ComposerAttachmentLike[];
  selectedAttachmentIndex: number | null;
  selectedAttachment: ComposerAttachmentLike | null;
  selectedWorkspacePath: string | null;
  workspaceRootPath: string;
  activeStageId: string | null;
  provenance: BreadcrumbProvenance | null;
  onPreviewStage?: (stageId: string | null) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onRemoveReference: (reference: string) => void;
  onRemoveAttachment: (index: number) => void;
  onSelectAttachment: (index: number, attachment: ComposerAttachmentLike) => void;
  onLocateAttachment: (attachment: ComposerAttachmentLike) => void;
  onNavigateStage: (stageId: string) => void;
  onNavigateProvenanceSession: () => void;
  onNavigateProvenanceStage: () => void;
  onNavigateProvenanceToolCall: () => void;
  onDragEnter: (event: DragEvent<HTMLDivElement>) => void;
  onDragOver: (event: DragEvent<HTMLDivElement>) => void;
  onDragLeave: (event: DragEvent<HTMLDivElement>) => void;
  onDrop: (event: DragEvent<HTMLDivElement>) => void;
  onFileChange: (event: React.ChangeEvent<HTMLInputElement>) => void | Promise<void>;
  onPaste: (event: ClipboardEvent<HTMLTextAreaElement>) => void | Promise<void>;
  onComposerChange: (value: string) => void;
}

export function ComposerPanel({
  composer,
  composerDragActive,
  streaming,
  references,
  attachments,
  selectedAttachmentIndex,
  selectedAttachment,
  selectedWorkspacePath,
  workspaceRootPath,
  activeStageId,
  provenance,
  onPreviewStage,
  onSubmit,
  onRemoveReference,
  onRemoveAttachment,
  onSelectAttachment,
  onLocateAttachment,
  onNavigateStage,
  onNavigateProvenanceSession,
  onNavigateProvenanceStage,
  onNavigateProvenanceToolCall,
  onDragEnter,
  onDragOver,
  onDragLeave,
  onDrop,
  onFileChange,
  onPaste,
  onComposerChange,
}: ComposerPanelProps) {
  const workspaceLabel =
    selectedWorkspacePath?.split("/").filter(Boolean).pop() ||
    workspaceRootPath.split("/").filter(Boolean).pop() ||
    "workspace";

  return (
    <div className="flex flex-col gap-3" data-testid="composer-form">
      <ComposerContextStrip
        references={references}
        attachments={attachments}
        selectedAttachmentIndex={selectedAttachmentIndex}
        selectedWorkspacePath={selectedWorkspacePath}
        workspaceRootPath={workspaceRootPath}
        activeStageId={activeStageId}
        provenance={provenance}
        onPreviewStage={onPreviewStage}
        onRemoveReference={onRemoveReference}
        onRemoveAttachment={onRemoveAttachment}
        onSelectAttachment={onSelectAttachment}
      />
      <AttachmentDetailsPanel
        attachment={selectedAttachment}
        workspaceRootPath={workspaceRootPath}
        activeStageId={activeStageId}
        provenance={provenance}
        onLocateAttachment={onLocateAttachment}
        onNavigateStage={onNavigateStage}
        onNavigateProvenanceSession={onNavigateProvenanceSession}
        onNavigateProvenanceStage={onNavigateProvenanceStage}
        onNavigateProvenanceToolCall={onNavigateProvenanceToolCall}
      />

      <div
        className={cn(
          "overflow-hidden rounded-xl border border-border/45 bg-background/95 shadow-sm transition-colors",
          composerDragActive ? "border-primary/40 bg-primary/5" : ""
        )}
      >
        <form
          className="w-full"
          onSubmit={onSubmit}
          data-testid="composer-dropzone"
        >
          <div
            className="flex flex-col"
            onDragEnter={onDragEnter}
            onDragOver={onDragOver}
            onDragLeave={onDragLeave}
            onDrop={onDrop}
          >
            {attachments.length > 0 ? (
              <div className="border-b border-border/60 px-4 py-3">
                <div className="flex flex-wrap gap-1.5">
                {attachments.map((att, index) => (
                  <Button
                    key={index}
                    type="button"
                    variant={selectedAttachmentIndex === index ? "secondary" : "outline"}
                    size="sm"
                    className={cn(
                      "h-6 gap-1 rounded-md text-[11px] border-border/60",
                      selectedAttachmentIndex === index && "border-primary/40 bg-primary/10"
                    )}
                    onClick={() => onSelectAttachment(index, att)}
                  >
                    <span className="text-muted-foreground">◦</span>
                    <span className="max-w-[100px] truncate">
                      {"filename" in att ? att.filename : `Attachment ${index + 1}`}
                    </span>
                    <span
                      className="ml-0.5 flex size-3.5 items-center justify-center rounded hover:bg-muted"
                      onClick={(e) => {
                        e.stopPropagation();
                        onRemoveAttachment(index);
                      }}
                    >
                      <span className="text-[10px] text-muted-foreground">×</span>
                    </span>
                  </Button>
                ))}
                </div>
              </div>
            ) : null}

            <div className="border-b border-border/50 px-5 py-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="roc-pill-outline px-2.5 text-foreground">
                    {workspaceLabel}
                  </span>
                  <span className="text-[11px] text-muted-foreground">
                    Prompt, attach files, or continue the current workflow.
                  </span>
                </div>
                <span className="text-[11px] text-muted-foreground/70">
                  {streaming ? "Running" : "Ready"}
                </span>
              </div>
            </div>

            <div className="px-5 pt-4">
              <textarea
                name="message"
                placeholder="Describe the task, use @file to reference material, or paste an image..."
                value={composer}
                onChange={(e) => onComposerChange(e.target.value)}
                onPaste={onPaste}
                disabled={streaming}
                className="min-h-[132px] max-h-[340px] w-full resize-none border-0 bg-transparent text-[15px] leading-7 text-foreground outline-none placeholder:text-muted-foreground/50"
              />
            </div>

            <div className="flex items-center justify-between border-t border-border/60 px-5 py-3.5">
              <div className="flex min-w-0 items-center gap-2">
                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="h-9 w-9 rounded-lg"
                        onClick={() => {
                          document.querySelector<HTMLInputElement>(
                            '[data-testid="composer-file-input"]',
                          )?.click();
                        }}
                      >
                        <PlusIcon className="size-3.5" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>Add files or images</TooltipContent>
                  </Tooltip>
                </TooltipProvider>

              <input
                data-testid="composer-file-input"
                type="file"
                multiple
                className="hidden"
                onChange={onFileChange}
              />

              <span className="flex items-center gap-1 rounded-full bg-muted/25 px-2 py-1 text-[11px] text-muted-foreground/70">
                <CommandIcon className="size-3" />
                <span>+</span>
                <span className="font-mono">K</span>
              </span>
              <span className="truncate text-[11px] text-muted-foreground/60">
                Paste images or drop files here
              </span>
              </div>

              <div className="flex items-center gap-2">
              {streaming ? (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-9 gap-1.5 rounded-lg border-border/45 text-xs"
                  onClick={() => {
                    window.dispatchEvent(new CustomEvent('rocode:stop-streaming'));
                  }}
                >
                  <span className="size-2 rounded-sm bg-current" />
                  Stop
                  </Button>
                ) : null}

                <Button
                  type="submit"
                  variant="default"
                  size="sm"
                  disabled={!composer.trim() && attachments.length === 0}
                  className="h-10 rounded-lg px-4"
                >
                  <span className="mr-1 text-xs font-medium">Send</span>
                  <SendIcon className="size-3.5" />
                </Button>
              </div>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
