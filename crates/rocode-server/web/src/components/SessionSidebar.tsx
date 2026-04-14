import { useMemo, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  FolderPlus,
  FolderTree,
  Layers2,
  Search,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import type { SessionTreeNode, WorkspaceSummary } from "@/lib/sidebar";
import { cn } from "@/lib/utils";

interface SessionSidebarProps {
  workspaces: WorkspaceSummary[];
  currentWorkspacePath: string | null;
  currentWorkspaceLabel: string | null;
  currentWorkspaceRootPath: string | null;
  currentWorkspaceMode: "shared" | "isolated" | null;
  sessionTree: SessionTreeNode[];
  selectedSessionId: string | null;
  onCreateProject: (input: { path: string; title?: string }) => void;
  onCreateSession: () => void;
  onSelectWorkspace: (workspacePath: string) => void;
  onSelectSession: (sessionId: string) => void;
}

function SessionTreeList({
  nodes,
  selectedSessionId,
  collapsedIds,
  depth = 0,
  onToggleCollapsed,
  onSelectSession,
}: {
  nodes: SessionTreeNode[];
  selectedSessionId: string | null;
  collapsedIds: Set<string>;
  depth?: number;
  onToggleCollapsed: (sessionId: string) => void;
  onSelectSession: (sessionId: string) => void;
}) {
  return (
    <div className="flex flex-col gap-2">
      {nodes.map((node) => (
        <div key={node.id} className="flex flex-col gap-2">
          <div
            className={cn(
              "px-0 transition-colors border-l-2",
              node.id === selectedSessionId
                ? "bg-accent/40 border-l-primary"
                : "hover:bg-accent/20 border-l-transparent",
            )}
            style={{ marginLeft: `${depth * 16}px` }}
          >
            <div className="flex items-start gap-1.5 px-3 py-3">
              {node.children.length > 0 ? (
                <button
                  type="button"
                  className="mt-0.5 rounded-md p-0.5 text-muted-foreground hover:bg-muted"
                  aria-label={collapsedIds.has(node.id) ? "Expand session" : "Collapse session"}
                  onClick={() => onToggleCollapsed(node.id)}
                >
                  {collapsedIds.has(node.id) ? (
                    <ChevronRight className="h-3.5 w-3.5" />
                  ) : (
                    <ChevronDown className="h-3.5 w-3.5" />
                  )}
                </button>
              ) : (
                <span className="w-5 shrink-0" />
              )}

              <button
                type="button"
                data-testid="session-item"
                data-session-id={node.id}
                className="min-w-0 flex-1 text-left"
                onClick={() => onSelectSession(node.id)}
              >
                <div className="truncate text-sm font-semibold text-foreground">
                  {node.title || "(untitled)"}
                </div>
                <div className="mt-2 flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
                  <span className="rounded-full bg-muted px-2 py-0.5">
                    {depth === 0 ? "root" : `branch ${depth}`}
                  </span>
                  {node.children.length > 0 ? (
                    <span className="rounded-full bg-muted/60 px-2 py-0.5">
                      {node.children.length} children
                    </span>
                  ) : null}
                </div>
              </button>
            </div>
          </div>

          {node.children.length > 0 && !collapsedIds.has(node.id) ? (
            <SessionTreeList
              nodes={node.children}
              selectedSessionId={selectedSessionId}
              collapsedIds={collapsedIds}
              depth={depth + 1}
              onToggleCollapsed={onToggleCollapsed}
              onSelectSession={onSelectSession}
            />
          ) : null}
        </div>
      ))}
    </div>
  );
}

export function SessionSidebar({
  workspaces,
  currentWorkspacePath,
  currentWorkspaceLabel,
  currentWorkspaceRootPath,
  currentWorkspaceMode,
  sessionTree,
  selectedSessionId,
  onCreateProject,
  onCreateSession,
  onSelectWorkspace,
  onSelectSession,
}: SessionSidebarProps) {
  const [workspaceQuery, setWorkspaceQuery] = useState("");
  const [createOpen, setCreateOpen] = useState(false);
  const [newProjectPath, setNewProjectPath] = useState("");
  const [newProjectTitle, setNewProjectTitle] = useState("");
  const [collapsedSessionIds, setCollapsedSessionIds] = useState<Set<string>>(new Set());

  const filteredWorkspaces = useMemo(() => {
    const query = workspaceQuery.trim().toLowerCase();
    if (!query) return workspaces;
    return workspaces.filter(
      (workspace) =>
        workspace.label.toLowerCase().includes(query) ||
        workspace.path.toLowerCase().includes(query),
    );
  }, [workspaceQuery, workspaces]);

  const submitCreateProject = () => {
    const path = newProjectPath.trim();
    if (!path) return;
    onCreateProject({
      path,
      title: newProjectTitle.trim() || undefined,
    });
    setCreateOpen(false);
    setNewProjectPath("");
    setNewProjectTitle("");
  };

  const toggleCollapsed = (sessionId: string) => {
    setCollapsedSessionIds((current) => {
      const next = new Set(current);
      if (next.has(sessionId)) {
        next.delete(sessionId);
      } else {
        next.add(sessionId);
      }
      return next;
    });
  };

  return (
    <aside
      className="h-full overflow-y-auto border-r border-border bg-sidebar"
      data-testid="session-sidebar"
    >
      <div className="space-y-6">
        <div className="px-4 py-3 border-b border-border">
          <div className="grid grid-cols-1 gap-2">
            <Button
              variant="outline"
              size="sm"
              className="h-9 rounded-xl"
              type="button"
              data-testid="project-new"
              onClick={() => setCreateOpen(true)}
            >
              <FolderPlus className="mr-1.5 h-3.5 w-3.5" />
              New Project
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-9 rounded-xl"
              type="button"
              data-testid="session-new"
              onClick={onCreateSession}
              disabled={!currentWorkspacePath}
            >
              <Layers2 className="mr-1.5 h-3.5 w-3.5" />
              New Session
            </Button>
          </div>
        </div>
        <div className="flex flex-col px-4">
          <div className="mb-3 space-y-3">
            <div className="flex items-center justify-between gap-2">
              <div>
                <p className="text-[11px] font-medium tracking-[0.18em] text-muted-foreground uppercase">
                  Workspaces
                </p>
                <p className="text-xs text-muted-foreground">Switch projects under the current service root.</p>
              </div>
              <span className="roc-pill px-2">
                {filteredWorkspaces.length}
              </span>
            </div>

            <div className="relative">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={workspaceQuery}
                onChange={(event) => setWorkspaceQuery(event.target.value)}
                placeholder="Search workspaces"
                className="h-9 rounded-lg border-border/60 bg-background pl-8 text-sm"
              />
            </div>
          </div>

          <div className="min-h-0 overflow-y-auto pr-1">
            <div className="flex flex-col gap-2">
              {filteredWorkspaces.length === 0 ? (
                <div className="rounded-lg border border-dashed border-border/40 bg-muted/30 px-3 py-4 text-sm text-muted-foreground">
                  {workspaces.length === 0 ? "No workspaces yet." : "No matching workspaces."}
                </div>
              ) : (
                filteredWorkspaces.map((workspace) => (
                  <button
                    key={workspace.path}
                    type="button"
                    className={cn(
                      "px-3 py-2.5 text-left transition-colors border-l-2",
                      workspace.path === currentWorkspacePath
                        ? "bg-accent/40 border-l-primary"
                        : "hover:bg-accent/20 border-l-transparent",
                    )}
                    onClick={() => onSelectWorkspace(workspace.path)}
                  >
                    <div className="flex items-center gap-2 text-sm font-semibold text-foreground">
                      <FolderTree className="h-3.5 w-3.5 text-muted-foreground" />
                      <span className="truncate">{workspace.label}</span>
                    </div>
                    <div className="mt-1 truncate text-[11px] text-muted-foreground">
                      {workspace.path}
                    </div>
                    <div className="mt-2 flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
                      <span className="roc-pill px-2 py-0.5">
                        {workspace.sessionCount} sessions
                      </span>
                      <span className="roc-pill px-2 py-0.5">
                        {workspace.rootCount} roots
                      </span>
                    </div>
                  </button>
                ))
              )}
            </div>
          </div>
        </div>
        <div
          className="flex flex-col px-4"
          data-testid="session-list"
        >
          <div className="mb-3">
            <div>
              <p className="text-[11px] font-medium tracking-[0.18em] text-muted-foreground uppercase">
                Session Tree
              </p>
              <p className="truncate text-xs text-muted-foreground">
                {currentWorkspaceLabel ? `${currentWorkspaceLabel} workspace` : "Select a workspace"}
              </p>
            </div>
          </div>

          <div className="mb-3 rounded-lg border border-dashed border-border/40 bg-muted/30 px-3 py-2 text-[11px] text-muted-foreground">
            This tree only shows sessions under the selected workspace.
          </div>

          <div className="min-h-0 overflow-y-auto pr-1">
            {sessionTree.length === 0 ? (
              <div className="rounded-lg border border-dashed border-border/40 bg-muted/30 px-3 py-4 text-sm text-muted-foreground">
                No sessions in this workspace yet.
              </div>
            ) : (
              <SessionTreeList
                nodes={sessionTree}
                selectedSessionId={selectedSessionId}
                collapsedIds={collapsedSessionIds}
                onToggleCollapsed={toggleCollapsed}
                onSelectSession={onSelectSession}
              />
            )}
          </div>
        </div>
      </div>

      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Create Project</DialogTitle>
            <DialogDescription>
              Create a new workspace folder and open its root session in the left sidebar.
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-4 py-2">
            <div className="grid gap-2">
              <label htmlFor="project-path" className="text-sm font-medium">
                Workspace Folder
              </label>
              <Input
                id="project-path"
                placeholder="projects/new-project"
                value={newProjectPath}
                onChange={(event) => setNewProjectPath(event.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <label htmlFor="project-title" className="text-sm font-medium">
                Root Session Title
              </label>
              <Input
                id="project-title"
                placeholder="Natural Products Workspace"
                value={newProjectTitle}
                onChange={(event) => setNewProjectTitle(event.target.value)}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>
              Cancel
            </Button>
            <Button onClick={submitCreateProject} disabled={!newProjectPath.trim()}>
              Create Project
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </aside>
  );
}
