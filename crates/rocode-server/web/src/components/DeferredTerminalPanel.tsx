import { Suspense, lazy } from "react";
import type { useTerminalSessions } from "../hooks/useTerminalSessions";

const TerminalPanel = lazy(async () => {
  const module = await import("./TerminalPanel");
  return { default: module.TerminalPanel };
});

interface DeferredTerminalPanelProps {
  expanded: boolean;
  onExpand: () => void;
  terminal: ReturnType<typeof useTerminalSessions>;
}

function TerminalLoadingFallback() {
  return (
    <div className="rounded-2xl border border-border bg-card/75 backdrop-blur-sm shadow-lg p-5 flex flex-col items-center justify-center gap-3 text-muted-foreground" data-testid="terminal-loading">
      <h3>Loading terminal...</h3>
      <p>The xterm.js terminal is being loaded as a separate chunk.</p>
    </div>
  );
}

export function DeferredTerminalPanel({
  expanded,
  onExpand,
  terminal,
}: DeferredTerminalPanelProps) {
  if (!expanded) {
    return (
      <div className="rounded-2xl border border-border bg-card/75 backdrop-blur-sm shadow-lg p-5 grid gap-4" data-testid="terminal-collapsed">
        <div className="flex items-start justify-between gap-3">
          <div>
            <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Terminal</p>
            <h3>PTY Sessions</h3>
          </div>
          <button
            className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
            type="button"
            data-testid="terminal-open"
            onClick={onExpand}
          >
            Open Terminal
          </button>
        </div>
        <div className="text-center text-muted-foreground py-4">
          <p>
            Terminal stays collapsed by default, so PTY sessions and `xterm.js` are not loaded on
            first paint.
          </p>
          <p>Expand this panel when you actually need shell access.</p>
        </div>
      </div>
    );
  }

  return (
    <Suspense fallback={<TerminalLoadingFallback />}>
      <TerminalPanel terminal={terminal} />
    </Suspense>
  );
}
