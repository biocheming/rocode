// ── Output Block Dispatcher ────────────────────────────────────────────────

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
      state.streamMessageNode = appendMessage(block.role || "assistant", "", Date.now(), {
        title: block.role || "assistant",
      }).bodyNode;
      return;
    }
    if (block.phase === "delta") {
      if (!state.streamMessageNode) {
        state.streamMessageNode = appendMessage(block.role || "assistant", "", Date.now(), {
          title: block.role || "assistant",
        }).bodyNode;
      }
      state.streamMessageNode.textContent += block.text || "";
      nodes.messageFeed.scrollTop = nodes.messageFeed.scrollHeight;
      return;
    }
    if (block.phase === "end") {
      state.streamMessageNode = null;
      return;
    }
    if (block.phase === "full") {
      appendMessage(block.role || "assistant", block.text || "", block.ts || Date.now(), {
        title: block.title || (block.role || "assistant"),
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
    let entry = state.streamToolBlocks.get(key);
    if (!entry) {
      entry = appendSessionEventBlock(block);
      state.streamToolBlocks.set(key, entry);
    }
    updateSessionEventBlock(entry, block);
    state.streamToolBlocks.delete(key);
    return;
  }

  if (block.kind === "scheduler_stage") {
    const key = block.id || `${block.profile || "scheduler"}:${block.stage || "stage"}`;
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
