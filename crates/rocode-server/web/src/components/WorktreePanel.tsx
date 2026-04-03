"use client";

import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  GitBranchIcon,
  PlusIcon,
  Trash2Icon,
  RefreshCwIcon,
  LoaderCircleIcon,
  CheckCircleIcon,
  XCircleIcon,
  FolderIcon,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { apiJson } from "../lib/api";

interface WorktreeInfo {
  path: string;
  branch: string;
  head: string;
}

interface WorktreePanelProps {
  className?: string;
}

function formatDate(timestamp: number): string {
  const date = new Date(timestamp * 1000);
  return date.toLocaleDateString() + " " + date.toLocaleTimeString();
}

export function WorktreePanel({ className }: WorktreePanelProps) {
  const [worktrees, setWorktrees] = useState<WorktreeInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [newBranchName, setNewBranchName] = useState("");
  const [newPath, setNewPath] = useState("");
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadWorktrees = useCallback(async () => {
    try {
      const data = await apiJson<WorktreeInfo[]>("/worktree");
      setWorktrees(data);
      setError(null);
    } catch (err) {
      setError(`Failed to load worktrees: ${err}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadWorktrees();
  }, [loadWorktrees]);

  const handleCreate = useCallback(async () => {
    setCreating(true);
    try {
      await apiJson<WorktreeInfo>("/worktree", {
        method: "POST",
        body: JSON.stringify({
          branch: newBranchName || null,
          path: newPath || null,
        }),
      });
      setCreateOpen(false);
      setNewBranchName("");
      setNewPath("");
      void loadWorktrees();
    } catch (err) {
      setError(`Failed to create worktree: ${err}`);
    } finally {
      setCreating(false);
    }
  }, [newBranchName, newPath, loadWorktrees]);

  const handleDelete = useCallback(
    async (path: string) => {
      if (!confirm(`Remove worktree at ${path}?`)) return;
      try {
        await apiJson("/worktree", {
          method: "DELETE",
          body: JSON.stringify({ path, force: false }),
        });
        void loadWorktrees();
      } catch (err) {
        setError(`Failed to remove worktree: ${err}`);
      }
    },
    [loadWorktrees]
  );

  return (
    <div className={cn("flex flex-col h-full overflow-hidden", className)}>
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-muted/30">
        <div className="flex items-center gap-2">
          <GitBranchIcon className="size-4" />
          <h3 className="font-medium text-sm">Worktrees</h3>
          <span className="text-xs text-muted-foreground">({worktrees.length})</span>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon-sm"
            className="size-7"
            onClick={() => {
              setRefreshing(true);
              void loadWorktrees().then(() => setRefreshing(false));
            }}
            disabled={refreshing}
          >
            <RefreshCwIcon className={cn("size-4", refreshing && "animate-spin")} />
          </Button>
          <Dialog open={createOpen} onOpenChange={setCreateOpen}>
            <DialogTrigger asChild>
              <Button variant="ghost" size="icon-sm" className="size-7">
                <PlusIcon className="size-4" />
              </Button>
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>Create Worktree</DialogTitle>
                <DialogDescription>
                  Create a new Git worktree for experimenting with branches
                  without affecting the main repository.
                </DialogDescription>
              </DialogHeader>
              <div className="grid gap-4 py-4">
                <div className="grid gap-2">
                  <label htmlFor="branch" className="text-sm font-medium">
                    Branch name (optional)
                  </label>
                  <Input
                    id="branch"
                    placeholder="feature/experiment"
                    value={newBranchName}
                    onChange={(e) => setNewBranchName(e.target.value)}
                  />
                </div>
                <div className="grid gap-2">
                  <label htmlFor="path" className="text-sm font-medium">
                    Path (optional)
                  </label>
                  <Input
                    id="path"
                    placeholder="repo-feature-1"
                    value={newPath}
                    onChange={(e) => setNewPath(e.target.value)}
                  />
                </div>
              </div>
              <DialogFooter>
                <Button variant="outline" onClick={() => setCreateOpen(false)}>
                  Cancel
                </Button>
                <Button onClick={() => void handleCreate()} disabled={creating}>
                  {creating ? (
                    <>
                      <LoaderCircleIcon className="size-4 animate-spin mr-2" />
                      Creating...
                    </>
                  ) : (
                    <>
                      <PlusIcon className="size-4 mr-2" />
                      Create
                    </>
                  )}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        </div>
      </div>

      {/* Error message */}
      {error && (
        <div className="mx-3 mt-3 p-3 rounded-lg bg-destructive/10 text-destructive text-sm">
          {error}
        </div>
      )}

      {/* Worktree list */}
      <div className="flex-1 overflow-auto p-3 space-y-2">
        {loading ? (
          <div className="flex items-center justify-center py-8 text-muted-foreground">
            <LoaderCircleIcon className="size-5 animate-spin mr-2" />
            Loading worktrees...
          </div>
        ) : worktrees.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-8 text-muted-foreground text-sm">
            <GitBranchIcon className="size-8 opacity-30 mb-2" />
            <p>No worktrees</p>
            <p className="text-xs mt-1">Create a worktree to experiment safely</p>
          </div>
        ) : (
          worktrees.map((wt) => (
            <div
              key={wt.path}
              className="rounded-xl border bg-card/80 p-4 grid gap-3 hover:bg-card transition-colors"
            >
              {/* Header */}
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2 min-w-0">
                  <FolderIcon className="size-4 flex-shrink-0 text-muted-foreground" />
                  <span className="font-medium text-sm truncate">{wt.branch}</span>
                </div>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  className="size-7 text-muted-foreground hover:text-destructive"
                  onClick={() => void handleDelete(wt.path)}
                >
                  <Trash2Icon className="size-4" />
                </Button>
              </div>

              {/* Path */}
              <p className="text-xs text-muted-foreground font-mono truncate" title={wt.path}>
                {wt.path}
              </p>

              {/* Head */}
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                <span className="font-mono px-1.5 py-0.5 rounded bg-muted">
                  {wt.head.slice(0, 7)}
                </span>
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
