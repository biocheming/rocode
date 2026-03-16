// ── Output Block Dispatcher ────────────────────────────────────────────────

function targetsSelectedSession(sessionId) {
  return !sessionId || sessionId === state.selectedSession;
}

function applyOutputBlockEvent(payload) {
  if (!payload) return false;
  const sessionId = payload.sessionID || payload.sessionId || null;
  if (!targetsSelectedSession(sessionId)) {
    return false;
  }
  const block = payload && payload.block ? payload.block : payload;
  applyOutputBlock(block);
  return true;
}

function applyOutputBlock(block) {
  if (!block || !block.kind) return;

  if (block.kind === "status") {
    const tone = toneForMessage(block.tone || "normal");
    setBadge(block.text || "status", toneForBadge(tone));
    if (!block.silent) {
      appendMessage("status", block.text || "status", Date.now(), {
        title: `status · ${tone}`,
        tone,
      });
    }
    return;
  }

  if (block.kind === "message") {
    if (block.phase === "start") {
      const result = appendMessage(block.role || "assistant", "", Date.now(), {
        title: block.role || "assistant",
      });
      state.streamMessageNode = result.bodyNode;
      state.streamMessageText = "";
      return;
    }
    if (block.phase === "delta") {
      if (!state.streamMessageNode) {
        const result = appendMessage(block.role || "assistant", "", Date.now(), {
          title: block.role || "assistant",
        });
        state.streamMessageNode = result.bodyNode;
        state.streamMessageText = "";
      }
      state.streamMessageText = (state.streamMessageText || "") + (block.text || "");
      state.streamMessageNode.innerHTML = "";
      state.streamMessageNode.classList.add("md-root");
      renderMarkdownToNode(state.streamMessageNode, state.streamMessageText);
      nodes.messageFeed.scrollTop = nodes.messageFeed.scrollHeight;
      return;
    }
    if (block.phase === "end") {
      state.streamMessageNode = null;
      state.streamMessageText = "";
      return;
    }
    if (block.phase === "full") {
      appendMessage(block.role || "assistant", block.text || "", block.ts || Date.now(), {
        title: block.title || (block.role || "assistant"),
      });
    }
    return;
  }

  if (block.kind === "reasoning") {
    if (!state.showThinking) {
      return;
    }

    if (block.phase === "start") {
      const result = appendMessage("reasoning", "", Date.now(), {
        title: "reasoning",
        tone: "muted",
      });
      state.streamReasoningNode = result.bodyNode;
      state.streamReasoningText = "";
      return;
    }
    if (block.phase === "delta") {
      if (!state.streamReasoningNode) {
        const result = appendMessage("reasoning", "", Date.now(), {
          title: "reasoning",
          tone: "muted",
        });
        state.streamReasoningNode = result.bodyNode;
        state.streamReasoningText = "";
      }
      state.streamReasoningText = (state.streamReasoningText || "") + (block.text || "");
      state.streamReasoningNode.textContent = state.streamReasoningText;
      nodes.messageFeed.scrollTop = nodes.messageFeed.scrollHeight;
      return;
    }
    if (block.phase === "end") {
      state.streamReasoningNode = null;
      state.streamReasoningText = "";
      return;
    }
    if (block.phase === "full") {
      appendMessage("reasoning", block.text || "", block.ts || Date.now(), {
        title: "reasoning",
        tone: "muted",
      });
    }
    return;
  }

  if (block.kind === "tool") {
    const phase = block.phase || "start";
    const key = block.id || block.name || `tool-${Date.now()}`;
    let entry = state.streamToolBlocks.get(key);
    if (!entry) {
      entry = appendToolBlock(block);
      state.streamToolBlocks.set(key, entry);
    }
    updateToolBlock(entry, block);

    if (phase === "done" || phase === "result" || phase === "error") {
      state.streamToolBlocks.delete(key);
    }
    return;
  }

  if (block.kind === "session_event") {
    const key = block.id || `${block.event || "event"}:${block.title || Date.now()}`;
    let entry = state.streamEventBlocks.get(key);
    if (!entry) {
      entry = appendSessionEventBlock(block);
      state.streamEventBlocks.set(key, entry);
    }
    updateSessionEventBlock(entry, block);
    state.streamEventBlocks.delete(key);
    return;
  }

  if (block.kind === "queue_item") {
    appendQueueItemBlock(block);
    return;
  }

  if (block.kind === "inspect") {
    renderInspectBlockPayload(block);
    return;
  }

  if (block.kind === "scheduler_stage") {
    const key =
      block.stage_id ||
      block.id ||
      `${block.profile || "scheduler"}:${block.stage || "stage"}`;
    let entry = state.streamStageBlocks.get(key);
    if (!entry) {
      entry = appendSchedulerStage(block);
      state.streamStageBlocks.set(key, entry);
    }
    updateSchedulerStage(entry, block);
    if (block.status === "done" || block.status === "blocked") {
      state.streamStageBlocks.delete(key);
    }
  }
}

function messageBodyFromParts(parts) {
  if (!Array.isArray(parts) || parts.length === 0) return "";
  const out = [];
  for (const part of parts) {
    const type = part.type;
    if ((type === "text" || type === "reasoning" || type === "compaction") && part.text) {
      out.push(part.text);
    }
  }
  return out.join("\n").trim();
}

function historyOutputBlocksFromParts(parts) {
  if (!Array.isArray(parts) || parts.length === 0) return [];
  return parts
    .map((part) => part && part.output_block ? part.output_block : null)
    .filter(Boolean);
}
