import { useEffect, useRef } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import type { useTerminalSessions } from "../hooks/useTerminalSessions";

interface TerminalPanelProps {
  terminal: ReturnType<typeof useTerminalSessions>;
}

export function TerminalPanel({ terminal }: TerminalPanelProps) {
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const xtermRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const renderedBufferRef = useRef("");
  const renderedSessionIdRef = useRef<string | null>(null);
  const activeSessionIdRef = useRef<string | null>(null);

  useEffect(() => {
    activeSessionIdRef.current = terminal.activeSession?.id ?? null;
  }, [terminal.activeSession]);

  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;

    const xterm = new XTerm({
      cursorBlink: true,
      fontFamily: '"SFMono-Regular", "Cascadia Code", "Fira Code", monospace',
      fontSize: 13,
      lineHeight: 1.3,
      rows: 24,
      cols: 80,
      theme: {
        background: "#0f172a",
        foreground: "#e2e8f0",
        cursor: "#f8fafc",
        cursorAccent: "#0f172a",
        selectionBackground: "rgba(148, 163, 184, 0.34)",
      },
    });
    const fitAddon = new FitAddon();
    xterm.loadAddon(fitAddon);
    xterm.open(viewport);

    const syncSize = () => {
      const activeSessionId = activeSessionIdRef.current;
      if (!activeSessionId) return;
      fitAddon.fit();
      void terminal.resizeSession(activeSessionId, xterm.cols, xterm.rows);
    };

    const queueSizeSync = () => {
      window.requestAnimationFrame(syncSize);
    };

    const dataDisposable = xterm.onData((data) => {
      terminal.sendInput(data);
    });
    const resizeObserver = new ResizeObserver(() => {
      queueSizeSync();
    });

    resizeObserver.observe(viewport);
    xtermRef.current = xterm;
    fitAddonRef.current = fitAddon;
    queueSizeSync();

    return () => {
      resizeObserver.disconnect();
      dataDisposable.dispose();
      fitAddon.dispose();
      xterm.dispose();
      xtermRef.current = null;
      fitAddonRef.current = null;
      renderedBufferRef.current = "";
      renderedSessionIdRef.current = null;
    };
  }, [terminal.resizeSession, terminal.sendInput]);

  useEffect(() => {
    const xterm = xtermRef.current;
    if (!xterm) return;

    if (!terminal.activeSession) {
      if (renderedSessionIdRef.current || renderedBufferRef.current) {
        xterm.reset();
      }
      renderedSessionIdRef.current = null;
      renderedBufferRef.current = "";
      return;
    }

    const sessionId = terminal.activeSession.id;
    const buffer = terminal.activeBuffer;
    const switchingSessions = renderedSessionIdRef.current !== sessionId;

    if (switchingSessions) {
      xterm.reset();
      if (buffer) {
        xterm.write(buffer);
      }
      renderedSessionIdRef.current = sessionId;
      renderedBufferRef.current = buffer;
      fitAddonRef.current?.fit();
      void terminal.resizeSession(sessionId, xterm.cols, xterm.rows);
      xterm.focus();
      return;
    }

    const previous = renderedBufferRef.current;
    if (!buffer) {
      if (previous) {
        xterm.reset();
      }
      renderedBufferRef.current = "";
      return;
    }

    if (buffer.startsWith(previous)) {
      const delta = buffer.slice(previous.length);
      if (delta) {
        xterm.write(delta);
      }
    } else {
      xterm.reset();
      xterm.write(buffer);
    }

    renderedBufferRef.current = buffer;
  }, [terminal.activeBuffer, terminal.activeSession, terminal.resizeSession]);

  return (
    <div className="rounded-2xl border border-border bg-card/75 backdrop-blur-sm shadow-lg p-5 grid gap-4" data-testid="terminal-panel">
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Terminal</p>
          <h3>PTY Sessions</h3>
        </div>
        <div className="flex items-center gap-2">
          <button
            className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
            type="button"
            data-testid="terminal-refresh"
            onClick={terminal.refresh}
            disabled={terminal.loading}
          >
            {terminal.loading ? "Refreshing..." : "Refresh"}
          </button>
          <button
            className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
            type="button"
            data-testid="terminal-create"
            onClick={() => void terminal.createSession()}
            disabled={terminal.creating}
          >
            {terminal.creating ? "Creating..." : "+ New"}
          </button>
          <button
            className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
            type="button"
            data-testid="terminal-delete"
            onClick={() => void terminal.deleteSession(terminal.activeSession!.id)}
            disabled={!terminal.activeSession}
          >
            Delete
          </button>
        </div>
      </div>

      {terminal.sessions.length ? (
        <>
          <div className="flex flex-wrap gap-2 border-b border-border pb-3">
            {terminal.sessions.map((session) => (
              <button
                key={session.id}
                data-testid="terminal-tab"
                data-session-id={session.id}
                className={terminal.activeId === session.id ? "px-4 py-2 rounded-full border-0 cursor-pointer text-sm bg-foreground text-background font-semibold" : "px-4 py-2 rounded-full border border-border cursor-pointer text-sm bg-card/70 text-foreground hover:bg-accent"}
                type="button"
                onClick={() => terminal.setActiveId(session.id)}
              >
                {session.command || "shell"}
              </button>
            ))}
          </div>

          {terminal.activeSession ? (
            <>
              <div className="flex flex-wrap gap-2">
                <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">{terminal.activeSession.status}</span>
                <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">{terminal.activeSession.cwd || "cwd unknown"}</span>
                <span className="rounded-full border border-border bg-muted px-3 py-1.5 text-xs text-muted-foreground font-medium">{terminal.activeSession.id}</span>
              </div>
              <div className="grid gap-2">
                <div
                  ref={viewportRef}
                  data-testid="terminal-viewport"
                  className="terminal-viewport"
                  onClick={() => xtermRef.current?.focus()}
                />
                <p className="text-xs text-muted-foreground italic">
                  Connected to PTY WebSocket. Keyboard input is sent directly to the shell.
                </p>
              </div>
            </>
          ) : null}
        </>
      ) : (
        <div className="flex flex-col items-center justify-center gap-3 text-muted-foreground py-8">
          <h3>No terminal sessions</h3>
          <p>Create a PTY session here instead of switching back to the legacy frontend.</p>
        </div>
      )}
    </div>
  );
}
