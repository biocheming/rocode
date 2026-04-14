"use client";

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { FormEvent, ClipboardEvent, DragEvent } from "react";
import type { BreadcrumbProvenance } from "../hooks/useSchedulerNavigation";
import {
  browserSpeechRecognitionConstructor,
  type BrowserSpeechRecognition,
} from "../lib/browserSpeech";
import { AttachmentDetailsPanel } from "./AttachmentDetailsPanel";
import { ComposerContextStrip } from "./ComposerContextStrip";
import type { ComposerAttachmentRecord } from "../lib/composerContext";
import { cn } from "@/lib/utils";
import {
  ImageIcon,
  MicIcon,
  PaperclipIcon,
  PlusIcon,
  SendIcon,
  SquareIcon,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

interface ComposerPanelProps {
  composer: string;
  composerDragActive: boolean;
  streaming: boolean;
  multimodalHints: Array<{ tone: "info" | "warning"; text: string }>;
  allowAudioInput: boolean;
  allowImageInput: boolean;
  allowFileInput: boolean;
  modeOptions: Array<{ key: string; label: string }>;
  selectedMode: string;
  onModeChange: (value: string) => void;
  modelOptions: Array<{ key: string; label: string }>;
  selectedModel: string;
  onModelChange: (value: string) => void;
  references: string[];
  attachments: ComposerAttachmentRecord[];
  selectedAttachmentIndex: number | null;
  selectedAttachment: ComposerAttachmentRecord | null;
  selectedWorkspacePath: string | null;
  workspaceRootPath: string;
  activeStageId: string | null;
  provenance: BreadcrumbProvenance | null;
  onPreviewStage?: (stageId: string | null) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onRemoveReference: (reference: string) => void;
  onRemoveAttachment: (index: number) => void;
  onSelectAttachment: (index: number, attachment: ComposerAttachmentRecord) => void;
  onLocateAttachment: (attachment: ComposerAttachmentRecord) => void;
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
  multimodalHints,
  allowAudioInput,
  allowImageInput,
  allowFileInput,
  modeOptions,
  selectedMode,
  onModeChange,
  modelOptions,
  selectedModel,
  onModelChange,
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
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const imageInputRef = useRef<HTMLInputElement>(null);
  const recognitionRef = useRef<BrowserSpeechRecognition | null>(null);
  const voiceBaseTextRef = useRef("");
  const [voiceSupported, setVoiceSupported] = useState(false);
  const [voiceListening, setVoiceListening] = useState(false);
  const [voiceError, setVoiceError] = useState<string | null>(null);

  useLayoutEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;

    textarea.style.height = "auto";

    const computed = window.getComputedStyle(textarea);
    const lineHeight = Number.parseFloat(computed.lineHeight) || 24;
    const paddingTop = Number.parseFloat(computed.paddingTop) || 0;
    const paddingBottom = Number.parseFloat(computed.paddingBottom) || 0;
    const borderTop = Number.parseFloat(computed.borderTopWidth) || 0;
    const borderBottom = Number.parseFloat(computed.borderBottomWidth) || 0;
    const maxHeight =
      lineHeight * 10 + paddingTop + paddingBottom + borderTop + borderBottom;
    const nextHeight = Math.min(textarea.scrollHeight, maxHeight);

    textarea.style.height = `${nextHeight}px`;
    textarea.style.overflowY =
      textarea.scrollHeight > maxHeight ? "auto" : "hidden";
  }, [composer]);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const RecognitionCtor = browserSpeechRecognitionConstructor(window);
    setVoiceSupported(Boolean(RecognitionCtor));

    return () => {
      recognitionRef.current?.stop();
      recognitionRef.current = null;
    };
  }, []);

  const stopVoiceRecognition = () => {
    recognitionRef.current?.stop();
    recognitionRef.current = null;
    setVoiceListening(false);
  };

  const startVoiceRecognition = () => {
    if (typeof window === "undefined") return;
    const RecognitionCtor = browserSpeechRecognitionConstructor(window);
    if (!RecognitionCtor) {
      setVoiceSupported(false);
      setVoiceError("This browser does not support speech recognition.");
      return;
    }

    setVoiceError(null);
    voiceBaseTextRef.current = composer.trimEnd();

    const recognition = new RecognitionCtor();
    recognition.continuous = false;
    recognition.interimResults = true;
    recognition.lang =
      typeof navigator !== "undefined" && navigator.language
        ? navigator.language
        : "en-US";
    recognition.onresult = (event) => {
      let finalTranscript = "";
      let interimTranscript = "";

      for (let index = event.resultIndex; index < event.results.length; index += 1) {
        const result = event.results[index];
        const transcript = result[0]?.transcript ?? result.item(0)?.transcript ?? "";
        if (!transcript) continue;
        if (result.isFinal) {
          finalTranscript += transcript;
        } else {
          interimTranscript += transcript;
        }
      }

      const spokenText = [finalTranscript, interimTranscript]
        .map((value) => value.trim())
        .filter(Boolean)
        .join(" ")
        .trim();

      const base = voiceBaseTextRef.current;
      if (!spokenText) {
        onComposerChange(base);
        return;
      }

      onComposerChange(base ? `${base}\n${spokenText}` : spokenText);
    };
    recognition.onerror = (event) => {
      if (event.error === "no-speech") {
        setVoiceError("No speech detected.");
      } else if (event.error === "not-allowed") {
        setVoiceError("Microphone permission was denied.");
      } else {
        setVoiceError(`Voice input failed: ${event.error}`);
      }
      setVoiceListening(false);
      recognitionRef.current = null;
    };
    recognition.onend = () => {
      setVoiceListening(false);
      recognitionRef.current = null;
    };

    recognitionRef.current = recognition;
    setVoiceListening(true);
    recognition.start();
  };

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
          "overflow-hidden rounded-2xl border border-border/50 bg-background/95 shadow-sm transition-colors",
          composerDragActive ? "border-primary/40 bg-primary/5" : "",
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
            <div className="px-5 pt-4">
              <textarea
                ref={textareaRef}
                name="message"
                rows={1}
                placeholder="Ask ROCode"
                value={composer}
                onChange={(e) => onComposerChange(e.target.value)}
                onPaste={onPaste}
                disabled={streaming}
                className="max-h-[17.5rem] w-full resize-none border-0 bg-transparent py-1 text-[15px] leading-7 text-foreground outline-none placeholder:text-muted-foreground/50"
              />
            </div>

            {multimodalHints.length > 0 ? (
              <div className="px-5 pb-2">
                <div className="flex flex-wrap gap-1.5">
                  {multimodalHints.map((hint, index) => (
                    <span
                      key={`${hint.tone}:${hint.text}:${index}`}
                      className={cn(
                        "rounded-full px-2.5 py-1 text-[11px]",
                        hint.tone === "warning"
                          ? "bg-amber-500/10 text-amber-700 dark:text-amber-300"
                          : "bg-muted text-muted-foreground",
                      )}
                    >
                      {hint.text}
                    </span>
                  ))}
                </div>
              </div>
            ) : null}

            <div className="flex items-center justify-between border-t border-border/60 px-3.5 py-2">
              <div className="flex min-w-0 flex-1 items-center gap-2 pr-3">
                <select
                  aria-label="Execution mode"
                  value={selectedMode}
                  onChange={(event) => onModeChange(event.target.value)}
                  className="h-7 min-w-0 max-w-[10rem] rounded-md bg-transparent px-2 text-[12px] text-muted-foreground hover:text-foreground outline-none transition cursor-pointer"
                >
                  <option value="">Mode: Auto</option>
                  {modeOptions.map((mode) => (
                    <option key={mode.key} value={mode.key}>
                      {mode.label}
                    </option>
                  ))}
                </select>
                <select
                  aria-label="Model"
                  value={selectedModel}
                  onChange={(event) => onModelChange(event.target.value)}
                  className="h-7 min-w-0 max-w-[11rem] rounded-md bg-transparent px-2 text-[12px] text-muted-foreground hover:text-foreground outline-none transition cursor-pointer"
                >
                  <option value="">Model: Auto</option>
                  {modelOptions.map((model) => (
                    <option key={model.key} value={model.key}>
                      {model.label}
                    </option>
                  ))}
                </select>
                <input
                  ref={fileInputRef}
                  data-testid="composer-file-input"
                  type="file"
                  multiple
                  className="hidden"
                  onChange={onFileChange}
                />
                <input
                  ref={imageInputRef}
                  data-testid="composer-image-input"
                  type="file"
                  accept="image/*"
                  multiple
                  className="hidden"
                  onChange={onFileChange}
                />
              </div>

              <div className="flex shrink-0 items-center gap-1.5">
                {voiceListening ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 rounded-full text-foreground"
                    title="Stop voice input"
                    onClick={stopVoiceRecognition}
                  >
                    <SquareIcon className="size-3.5 fill-current" />
                  </Button>
                ) : null}

                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 rounded-full text-muted-foreground hover:text-foreground"
                      title="Add attachment"
                    >
                      <PlusIcon className="size-4" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="min-w-[8rem]">
                    <DropdownMenuItem
                      disabled={!allowAudioInput || !voiceSupported}
                      onClick={startVoiceRecognition}
                      className="gap-2 text-xs"
                    >
                      <MicIcon className="size-3.5" />
                      Voice
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      disabled={!allowFileInput}
                      onClick={() => fileInputRef.current?.click()}
                      className="gap-2 text-xs"
                    >
                      <PaperclipIcon className="size-3.5" />
                      File
                      {attachments.length > 0 ? (
                        <span className="ml-auto rounded-full bg-muted px-1.5 py-0.5 text-[10px] text-foreground">
                          {attachments.length}
                        </span>
                      ) : null}
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      disabled={!allowImageInput}
                      onClick={() => imageInputRef.current?.click()}
                      className="gap-2 text-xs"
                    >
                      <ImageIcon className="size-3.5" />
                      Image
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>

                {streaming ? (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="h-8 gap-1.5 rounded-full border-border/45 px-3 text-[11px]"
                    onClick={() => {
                      window.dispatchEvent(new CustomEvent("rocode:stop-streaming"));
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
                  className="h-8 rounded-full px-3"
                >
                  <span className="mr-1 text-[11px] font-medium">Send</span>
                  <SendIcon className="size-3.25" />
                </Button>
              </div>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
