import type { ChangeEvent } from "react";
import React, { Suspense, useRef } from "react";
import type { useConversationJump } from "../hooks/useConversationJump";
import type { useExecutionActivity } from "../hooks/useExecutionActivity";
import type { useSchedulerNavigation } from "../hooks/useSchedulerNavigation";
import type { useTerminalSessions } from "../hooks/useTerminalSessions";
import type { FeedMessage } from "../lib/history";
import type { FileTreeNodeRecord } from "../lib/workspace";
import { WorkspaceTreeNode } from "./WorkspaceTreeNode";
import { DeferredTerminalPanel } from "./DeferredTerminalPanel";

const ExecutionActivityPanel = React.lazy(async () => {
  const module = await import("./ExecutionActivityPanel");
  return { default: module.ExecutionActivityPanel };
});

const SessionInsightsPanel = React.lazy(async () => {
  const module = await import("./SessionInsightsPanel");
  return { default: module.SessionInsightsPanel };
});

interface WorkspaceInspectorPanelProps {
  apiJson: <T>(path: string, options?: RequestInit) => Promise<T>;
  workspaceLoading: boolean;
  fileTree: FileTreeNodeRecord | null;
  workspaceRootLabel: string;
  selectedWorkspacePath: string | null;
  workspaceLinkLabel: string | null;
  workspaceLinkStageId: string | null;
  selectedWorkspaceFilename: string | null;
  selectedWorkspaceType: "file" | "directory";
  workspaceDirty: boolean;
  selectedFilePath: string | null;
  selectedFileContent: string;
  fileLoading: boolean;
  fileSaving: boolean;
  fileDeleting: boolean;
  fileUploading: boolean;
  selectedWorkspaceIsRoot: boolean;
  selectedWorkspaceReference: string | null;
  lastAssistant: Pick<FeedMessage, "title" | "text"> | null;
  activeStageId: string | null;
  previewStageId: string | null;
  executionActivity: ReturnType<typeof useExecutionActivity>;
  conversationJump: ReturnType<typeof useConversationJump>;
  schedulerNavigation: ReturnType<typeof useSchedulerNavigation>;
  terminalExpanded: boolean;
  terminalSessions: ReturnType<typeof useTerminalSessions>;
  onExpandTerminal: () => void;
  onCreateWorkspaceFile: () => void | Promise<void>;
  onCreateWorkspaceDirectory: () => void | Promise<void>;
  onUploadWorkspaceFiles: (event: ChangeEvent<HTMLInputElement>) => void | Promise<void>;
  onSelectWorkspaceNode: (path: string, type?: "file" | "directory") => void;
  onWorkspaceContentChange: (value: string) => void;
  onInsertWorkspaceReference: () => void;
  onAttachSelectedWorkspaceNode: () => void;
  onDownloadSelectedFile: () => void;
  onDeleteSelectedWorkspaceNode: () => void | Promise<void>;
  onSaveSelectedFile: () => void | Promise<void>;
}

function InspectorLoadingCard({ label }: { label: string }) {
  return (
    <div className="roc-panel p-5 grid gap-2.5 text-muted-foreground">
      <h3>Loading {label}...</h3>
      <p>This panel is being loaded as a separate chunk.</p>
    </div>
  );
}

const workspaceActionButtonClass =
  "roc-action min-h-[42px] px-4 cursor-pointer transition-colors";

export function WorkspaceInspectorPanel({
  apiJson,
  workspaceLoading,
  fileTree,
  workspaceRootLabel,
  selectedWorkspacePath,
  workspaceLinkLabel,
  workspaceLinkStageId,
  selectedWorkspaceFilename,
  selectedWorkspaceType,
  workspaceDirty,
  selectedFilePath,
  selectedFileContent,
  fileLoading,
  fileSaving,
  fileDeleting,
  fileUploading,
  selectedWorkspaceIsRoot,
  selectedWorkspaceReference,
  lastAssistant,
  activeStageId,
  previewStageId,
  executionActivity,
  conversationJump,
  schedulerNavigation,
  terminalExpanded,
  terminalSessions,
  onExpandTerminal,
  onCreateWorkspaceFile,
  onCreateWorkspaceDirectory,
  onUploadWorkspaceFiles,
  onSelectWorkspaceNode,
  onWorkspaceContentChange,
  onInsertWorkspaceReference,
  onAttachSelectedWorkspaceNode,
  onDownloadSelectedFile,
  onDeleteSelectedWorkspaceNode,
  onSaveSelectedFile,
}: WorkspaceInspectorPanelProps) {
  const workspaceUploadInputRef = useRef<HTMLInputElement | null>(null);

  return (
    <div className="flex flex-col gap-5 min-h-0 overflow-y-auto p-5" data-testid="workspace-inspector">
      <div className="roc-panel p-5 grid gap-3 min-h-0" data-testid="workspace-tree-card">
        <div className="flex items-start justify-between gap-3">
          <div>
            <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Workspace</p>
            <h3>File Tree</h3>
          </div>
          <div className="flex items-center flex-wrap gap-2.5 justify-end">
            <span className="roc-pill px-3 py-1.5 text-xs">
              {workspaceLoading ? "loading" : `${fileTree?.children?.length ?? 0} items`}
            </span>
            <button
              className={workspaceActionButtonClass}
              type="button"
              data-testid="workspace-new-file"
              onClick={() => void onCreateWorkspaceFile()}
            >
              New File
            </button>
            <button
              className={workspaceActionButtonClass}
              type="button"
              data-testid="workspace-new-folder"
              onClick={() => void onCreateWorkspaceDirectory()}
            >
              New Folder
            </button>
            <button
              className={workspaceActionButtonClass}
              type="button"
              data-testid="workspace-upload"
              disabled={fileUploading}
              onClick={() => workspaceUploadInputRef.current?.click()}
            >
              {fileUploading ? "Uploading..." : "Upload"}
            </button>
            <input
              ref={workspaceUploadInputRef}
              className="hidden"
              data-testid="workspace-upload-input"
              type="file"
              multiple
              onChange={onUploadWorkspaceFiles}
            />
          </div>
        </div>
        <p className="mt-2.5 text-sm leading-relaxed text-muted-foreground break-all">{workspaceRootLabel}</p>
        <div className="mt-3.5 flex flex-col gap-1 max-h-80 overflow-auto" data-testid="workspace-tree">
          {workspaceLoading ? (
            <p className="grid gap-2.5 text-muted-foreground">Loading workspace tree...</p>
          ) : fileTree ? (
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
          ) : (
            <p className="grid gap-2.5 text-muted-foreground">No workspace tree available yet.</p>
          )}
        </div>
      </div>

      <div className="roc-panel p-5 grid gap-3 min-h-0" data-testid="workspace-editor-card">
        <div className="flex items-start justify-between gap-3">
          <div>
            <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Workspace</p>
            <h3>{selectedWorkspaceFilename || "Workspace Preview"}</h3>
          </div>
          <div className="flex items-center flex-wrap gap-2.5 justify-end">
            <span className="roc-pill px-3 py-1.5 text-xs">
              {selectedWorkspaceType === "directory" ? "directory" : workspaceDirty ? "dirty" : "saved"}
            </span>
            {selectedWorkspacePath && workspaceLinkLabel ? <span className="roc-pill px-3 py-1.5 text-xs">{workspaceLinkLabel}</span> : null}
            <button
              className={workspaceActionButtonClass}
              type="button"
              data-testid="workspace-insert-reference"
              disabled={!selectedWorkspacePath}
              onClick={onInsertWorkspaceReference}
            >
              Insert @
            </button>
            <button
              className={workspaceActionButtonClass}
              type="button"
              data-testid="workspace-attach"
              disabled={!selectedWorkspacePath}
              onClick={onAttachSelectedWorkspaceNode}
            >
              Attach
            </button>
            <button
              className={workspaceActionButtonClass}
              type="button"
              data-testid="workspace-download"
              disabled={!selectedFilePath || fileDeleting || fileSaving}
              onClick={onDownloadSelectedFile}
            >
              Download
            </button>
            <button
              className={workspaceActionButtonClass}
              type="button"
              data-testid="workspace-delete"
              disabled={!selectedWorkspacePath || selectedWorkspaceIsRoot || fileDeleting || fileSaving}
              onClick={() => void onDeleteSelectedWorkspaceNode()}
            >
              {fileDeleting ? "Deleting..." : "Delete"}
            </button>
            <button
              className={workspaceActionButtonClass}
              type="button"
              data-testid="workspace-save"
              disabled={!selectedFilePath || fileLoading || fileSaving || fileDeleting || !workspaceDirty}
              onClick={() => void onSaveSelectedFile()}
            >
              {fileSaving ? "Saving..." : "Save"}
            </button>
          </div>
        </div>
        {selectedFilePath ? (
          <>
            <p className="mt-2.5 text-sm leading-relaxed text-muted-foreground break-all">{selectedFilePath}</p>
            <textarea
              className="roc-textarea mt-3.5 min-h-80 w-full resize-y p-3.5 text-foreground leading-relaxed font-mono text-sm"
              data-testid="workspace-editor"
              value={selectedFileContent}
              onChange={(event) => onWorkspaceContentChange(event.target.value)}
              disabled={fileLoading}
              spellCheck={false}
            />
          </>
        ) : selectedWorkspacePath && selectedWorkspaceType === "directory" ? (
          <div className="grid gap-2.5 text-muted-foreground">
            <h3>{selectedWorkspaceFilename || "Directory selected"}</h3>
            <p>{selectedWorkspacePath}</p>
            <p>
              This directory can be referenced with `{selectedWorkspaceReference ? `@${selectedWorkspaceReference}` : "@."}`{" "}
              or attached directly as a workspace context part.
            </p>
          </div>
        ) : (
          <div className="grid gap-2.5 text-muted-foreground">
            <h3>{lastAssistant?.title || "No file selected"}</h3>
            <p>
              {lastAssistant?.text ||
                "Pick a file from the workspace tree to read and edit it in the new frontend."}
            </p>
          </div>
        )}
      </div>

      <Suspense fallback={<InspectorLoadingCard label="scheduler activity" />}>
        <ExecutionActivityPanel
          activity={executionActivity}
          activeStageId={activeStageId}
          previewStageId={previewStageId}
          onJumpToConversation={conversationJump.jumpOrQueueConversationTarget}
          onNavigateStage={schedulerNavigation.navigateToStage}
          onNavigateChildSession={schedulerNavigation.navigateToChildSession}
          onNavigateToolCall={schedulerNavigation.navigateToToolCall}
        />
      </Suspense>

      <Suspense fallback={<InspectorLoadingCard label="session insights" />}>
        <SessionInsightsPanel activity={executionActivity} apiJson={apiJson} />
      </Suspense>

      <DeferredTerminalPanel
        expanded={terminalExpanded}
        onExpand={onExpandTerminal}
        terminal={terminalSessions}
      />
    </div>
  );
}
