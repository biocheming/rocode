import type { ChangeEvent, ClipboardEvent, DragEvent, FormEvent } from "react";
import { ComposerPanel } from "./ComposerPanel";
import type { BreadcrumbProvenance } from "../hooks/useSchedulerNavigation";
import type { ComposerAttachmentLike } from "../lib/composerContext";

interface ComposerSectionProps {
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
  onFileChange: (event: ChangeEvent<HTMLInputElement>) => void | Promise<void>;
  onPaste: (event: ClipboardEvent<HTMLTextAreaElement>) => void | Promise<void>;
  onComposerChange: (value: string) => void;
}

export function ComposerSection(props: ComposerSectionProps) {
  return (
    <div className="mx-auto w-full max-w-4xl">
      <ComposerPanel {...props} />
    </div>
  );
}
