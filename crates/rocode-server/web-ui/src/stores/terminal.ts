// ── Terminal Store ──────────────────────────────────────────────────────────

import { createStore, produce } from "solid-js/store";
import { api } from "~/api/client";
import type { TerminalSession } from "~/api/types";

export interface TerminalState {
  open: boolean;
  sessions: TerminalSession[];
  activeId: string | null;
  buffers: Map<string, string>;
  sockets: Map<string, WebSocket>;
  cursorById: Map<string, number>;
}

const [terminal, setTerminal] = createStore<TerminalState>({
  open: false,
  sessions: [],
  activeId: null,
  buffers: new Map(),
  sockets: new Map(),
  cursorById: new Map(),
});

export { terminal, setTerminal };

const MAX_BUFFER_SIZE = 200 * 1024; // 200KB

// Strip ANSI escape sequences so raw PTY output is readable in a plain <div>
function stripAnsi(text: string): string {
  return text
    .replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "")   // CSI sequences (colors, cursor, etc.)
    .replace(/\x1b\([A-B]/g, "")               // charset selection
    .replace(/\x1b\][^\x07]*\x07/g, "")        // OSC sequences (title, etc.)
    .replace(/\x1b\[\?[0-9;]*[a-zA-Z]/g, "")   // private mode sequences
    .replace(/\r/g, "");                         // carriage returns
}

export function toggleTerminal() {
  setTerminal("open", !terminal.open);
}

export function setActiveTerminal(sessionId: string) {
  setTerminal("activeId", sessionId);
}

export function appendTerminalOutput(sessionId: string, chunk: string) {
  const current = terminal.buffers.get(sessionId) ?? "";
  let next = current + stripAnsi(chunk);
  if (next.length > MAX_BUFFER_SIZE) {
    next = next.slice(-MAX_BUFFER_SIZE);
  }
  setTerminal("buffers", new Map(terminal.buffers).set(sessionId, next));
}

export async function createTerminalSession(): Promise<TerminalSession> {
  const response = await api("/pty", {
    method: "POST",
    body: JSON.stringify({ command: "/bin/bash", cwd: "~" }),
  });
  const session: TerminalSession = await response.json();
  setTerminal(
    produce((s) => {
      s.sessions = [...s.sessions, session];
      s.activeId = session.id;
    }),
  );
  return session;
}

export async function deleteTerminalSession(sessionId: string): Promise<void> {
  // Close WebSocket if open
  const ws = terminal.sockets.get(sessionId);
  if (ws) {
    ws.close();
    const next = new Map(terminal.sockets);
    next.delete(sessionId);
    setTerminal("sockets", next);
  }

  await api(`/pty/${sessionId}`, { method: "DELETE" });

  setTerminal(
    produce((s) => {
      s.sessions = s.sessions.filter((t) => t.id !== sessionId);
      if (s.activeId === sessionId) {
        s.activeId = s.sessions[0]?.id ?? null;
      }
    }),
  );
}

export async function loadTerminalSessions(): Promise<void> {
  const response = await api("/pty");
  const sessions: TerminalSession[] = await response.json();
  setTerminal("sessions", sessions);
  if (!terminal.activeId && sessions.length > 0) {
    setTerminal("activeId", sessions[0].id);
  }
}

export function registerTerminalSocket(sessionId: string, ws: WebSocket) {
  setTerminal("sockets", new Map(terminal.sockets).set(sessionId, ws));
}
