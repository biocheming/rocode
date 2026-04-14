"use client";

import type { ChangeEvent } from "react";
import { Suspense, lazy, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import type { FileTreeNodeRecord } from "@/lib/workspace";
import { WorkspaceTreeNode } from "./WorkspaceTreeNode";
import {
  FolderTreeIcon,
  LightbulbIcon,
  PlusIcon,
  FolderPlusIcon,
  UploadIcon,
} from "lucide-react";
import type { useExecutionActivity } from "../hooks/useExecutionActivity";

const SessionInsightsPanel = lazy(async () => {
  const module = await import("./SessionInsightsPanel");
  return { default: module.SessionInsightsPanel };
});

interface WorkspacePanelProps {
  apiJson: <T>(path: string, options?: RequestInit) => Promise<T>;
  workspaceLoading: boolean;
  fileTree: FileTreeNodeRecord | null;
  workspaceRootPath: string;
  workspaceRootLabel: string;
  selectedWorkspacePath: string | null;
  selectedWorkspaceType: "file" | "directory";
  workspaceLinkLabel: string | null;
  workspaceLinkStageId: string | null;
  selectedFilePath: string | null;
  selectedFileContent: string;
  fileLoading: boolean;
  fileSaving: boolean;
  fileDeleting: boolean;
  fileUploading: boolean;
  workspaceDirty: boolean;
  selectedWorkspaceIsRoot: boolean;
  selectedWorkspaceReference: string | null;
  lastAssistant: { title?: string; text?: string } | null;
  activeStageId: string | null;
  previewStageId: string | null;
  executionActivity: ReturnType<typeof useExecutionActivity>;
  conversationJump: unknown;
  schedulerNavigation: {
    navigateToStage: (stageId: string) => void;
    navigateToChildSession: (sessionId: string, context?: unknown) => void;
    previewStage: (stageId: string | null) => void;
    restoreActiveStage: () => void;
  };
  terminalExpanded: boolean;
  terminalSessions: unknown;
  onExpandTerminal: () => void;
  onCreateWorkspaceFile: () => Promise<void>;
  onCreateWorkspaceDirectory: () => Promise<void>;
  onUploadWorkspaceFiles: (event: ChangeEvent<HTMLInputElement>) => void;
  onSelectWorkspaceNode: (path: string, type: "file" | "directory") => void;
  onWorkspaceContentChange: (content: string) => void;
  onInsertWorkspaceReference: () => void;
  onAttachSelectedWorkspaceNode: () => void;
  onDownloadSelectedFile: () => void;
  onDeleteSelectedWorkspaceNode: () => Promise<void>;
  onSaveSelectedFile: () => Promise<void>;
}

export function WorkspacePanel({
  apiJson,
  workspaceLoading,
  fileTree,
  workspaceRootPath,
  workspaceRootLabel,
  selectedWorkspacePath,
  workspaceLinkLabel,
  workspaceLinkStageId,
  onCreateWorkspaceFile,
  onCreateWorkspaceDirectory,
  onUploadWorkspaceFiles,
  onSelectWorkspaceNode,
  schedulerNavigation,
  executionActivity,
}: WorkspacePanelProps) {
  const workspaceUploadInputRef = useRef<HTMLInputElement | null>(null);
  const [activeTab, setActiveTab] = useState<"files" | "insights">("files");

  return (
    <div className="flex flex-col h-full overflow-hidden" data-testid="workspace-panel">
      {/* Header */}
      <div className="flex items-center justify-between px-2.5 py-2 border-b border-border/60 bg-muted/20 shrink-0">
        <div className="flex items-center gap-1.5 min-w-0">
          <FolderTreeIcon className="size-3.5 text-muted-foreground/70 flex-shrink-0" />
          <span className="font-medium text-[11px] text-muted-foreground/90 truncate">{workspaceRootLabel}</span>
        </div>
        <div className="flex items-center gap-0.5 flex-shrink-0">
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            onClick={() => void onCreateWorkspaceFile()}
            title="New file"
          >
            <PlusIcon className="size-3" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            onClick={() => void onCreateWorkspaceDirectory()}
            title="New folder"
          >
            <FolderPlusIcon className="size-3" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            onClick={() => workspaceUploadInputRef.current?.click()}
            title="Upload"
          >
            <UploadIcon className="size-3" />
          </Button>
        </div>
      </div>

      <div className="flex items-center border-b border-border/60 bg-muted/10 shrink-0">
        <button
          className={cn(
            "flex-1 flex items-center justify-center gap-1.5 px-2 py-2 text-[11px] transition-colors",
            activeTab === "files"
              ? "bg-background text-foreground border-b border-primary"
              : "text-muted-foreground hover:text-foreground"
          )}
          type="button"
          onClick={() => setActiveTab("files")}
        >
          <FolderTreeIcon className="size-3.5" />
          <span>Files</span>
        </button>
        <button
          className={cn(
            "flex-1 flex items-center justify-center gap-1.5 px-2 py-2 text-[11px] transition-colors",
            activeTab === "insights"
              ? "bg-background text-foreground border-b border-primary"
              : "text-muted-foreground hover:text-foreground"
          )}
          type="button"
          onClick={() => setActiveTab("insights")}
        >
          <LightbulbIcon className="size-3.5" />
          <span>Insights</span>
        </button>
      </div>

      {/* File Tree */}
      <div className="flex-1 overflow-auto py-1">
        {activeTab === "insights" ? (
          <Suspense
            fallback={
              <div className="flex items-center justify-center py-6 text-muted-foreground/60">
                <span className="text-[10px]">Loading insights...</span>
              </div>
            }
          >
            <div className="p-2">
              <SessionInsightsPanel activity={executionActivity} apiJson={apiJson} />
            </div>
          </Suspense>
        ) : null}
        {activeTab === "files"
          ? workspaceLoading
            ? (
              <div className="flex items-center justify-center py-6 text-muted-foreground/60">
                <span className="text-[10px]">Loading...</span>
              </div>
            )
            : fileTree
              ? (
                <WorkspaceTreeNode
                  node={fileTree}
                  selectedPath={selectedWorkspacePath}
                  linkedPath={workspaceLinkLabel ? selectedWorkspacePath : null}
                  linkedLabel={workspaceLinkLabel}
                  linkedStageId={workspaceLinkStageId}
                  onSelectNode={(node) => {
                    onSelectWorkspaceNode(node.path, node.type);
                    schedulerNavigation.restoreActiveStage();
                  }}
                  onPreviewStage={schedulerNavigation.previewStage}
                />
              )
              : (
                <div className="text-[10px] text-muted-foreground/50 px-3 py-2">
                  No workspace
                </div>
              )
          : null}
      </div>

      {/* Hidden file input */}
      <input
        ref={workspaceUploadInputRef}
        className="hidden"
        type="file"
        multiple
        onChange={onUploadWorkspaceFiles}
      />
    </div>
  );
}
