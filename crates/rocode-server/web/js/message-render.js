// ── Message & Tool Block Rendering (Simplified Design System) ────────────────

function clearFeed() {
  nodes.messageFeed.innerHTML = "";
  state.streamMessageNode = null;
  state.streamToolBlocks.clear();
  state.streamStageBlocks.clear();
}

// ─────────────────────────────────────────────────────────────────────────────
// Core Message Types (Simplified to 3: user, ai, system)
// ─────────────────────────────────────────────────────────────────────────────

function createMessageElement(role, options = {}) {
  const article = document.createElement("article");
  article.className = `message message-${role}`;
  if (options.animate !== false) {
    article.classList.add("animate-fade-in");
  }

  // Header (role + timestamp) - only for non-user messages or if explicitly shown
  if (role !== "user" || options.showHeader) {
    const header = document.createElement("div");
    header.className = "message-header";

    const roleSpan = document.createElement("span");
    roleSpan.className = "message-role";
    roleSpan.textContent = options.title || (role === "ai" ? "ROCode" : role);
    header.appendChild(roleSpan);

    const timeSpan = document.createElement("span");
    timeSpan.className = "message-time";
    timeSpan.textContent = formatTime(options.ts || Date.now());
    header.appendChild(timeSpan);

    article.appendChild(header);
  }

  // Content
  const content = document.createElement("div");
  content.className = "message-content";
  article.appendChild(content);

  return { article, content };
}

function appendMessage(role, text, ts, options = {}) {
  const mappedRole = role === "assistant" ? "ai" : (role === "tool" || role === "system") ? "system" : role;
  const { article, content } = createMessageElement(mappedRole, { ...options, ts });

  const body = text && text.trim().length > 0 ? text : "(empty)";
  content.textContent = body;

  nodes.messageFeed.appendChild(article);
  nodes.messageFeed.scrollTop = nodes.messageFeed.scrollHeight;

  return { article, content };
}

// ─────────────────────────────────────────────────────────────────────────────
// Tool Block Rendering
// ─────────────────────────────────────────────────────────────────────────────

function toolPhaseLabel(phase) {
  switch (phase) {
    case "start": return "start";
    case "running": return "running";
    case "done":
    case "result": return "done";
    case "error": return "error";
    default: return phase || "tool";
  }
}

function toolPhaseTone(phase) {
  if (phase === "error") return "error";
  if (phase === "done" || phase === "result") return "success";
  return "warning";
}

function statusChipTone(status) {
  const normalized = String(status || "").toLowerCase();
  if (!normalized) return "running";
  if (normalized === "completed" || normalized === "done" || normalized === "success") return "done";
  if (normalized === "failed" || normalized === "error") return "error";
  if (normalized === "pending" || normalized === "queued") return "waiting";
  if (normalized === "in_progress" || normalized === "in-progress" || normalized === "running") return "running";
  if (normalized === "cancelled" || normalized === "canceled") return "cancelled";
  if (normalized === "blocked") return "blocked";
  return normalized;
}

function humanStatusLabel(status) {
  if (!status) return "event";
  return String(status).replace(/_/g, " ");
}

function humanEventLabel(event) {
  const normalized = String(event || "").toLowerCase();
  switch (normalized) {
    case "subtask": return "Subtask";
    case "retry": return "Retry";
    case "step": return "Step";
    case "agent": return "Agent";
    default: return event || "Event";
  }
}

function sessionEventTone(status) {
  if (status === "error" || status === "failed") return "error";
  if (status === "completed" || status === "done" || status === "success") return "success";
  if (status === "running" || status === "pending" || status === "in_progress") return "warning";
  return "warning";
}

// ─────────────────────────────────────────────────────────────────────────────
// Tool Block (Rendered as system message)
// ─────────────────────────────────────────────────────────────────────────────

function appendToolBlock(block) {
  const { article, content } = createMessageElement("system", { title: "Tool", ts: block.ts });

  // Tool header with name and phase badge
  const header = document.createElement("div");
  header.style.cssText = "display: flex; align-items: center; gap: var(--space-2); margin-bottom: var(--space-2);";

  const nameBadge = document.createElement("span");
  nameBadge.className = "badge";
  nameBadge.textContent = block.name || "tool";
  header.appendChild(nameBadge);

  const phaseBadge = document.createElement("span");
  phaseBadge.className = "badge badge-running";
  phaseBadge.textContent = "running";
  header.appendChild(phaseBadge);

  content.appendChild(header);

  // Summary text
  const summary = document.createElement("div");
  summary.className = "text-secondary";
  summary.style.cssText = "font-size: var(--text-sm); line-height: var(--leading-relaxed);";
  content.appendChild(summary);

  // Fields grid
  const fieldsNode = document.createElement("div");
  fieldsNode.className = "hidden";
  fieldsNode.style.cssText = "display: grid; gap: var(--space-2); margin-top: var(--space-3);";
  content.appendChild(fieldsNode);

  // Preview section
  const previewNode = document.createElement("div");
  previewNode.className = "hidden";
  previewNode.style.cssText = "margin-top: var(--space-3); padding: var(--space-3); background: var(--bg-base); border-radius: var(--radius-md); border: 1px solid var(--border-subtle);";
  content.appendChild(previewNode);

  nodes.messageFeed.appendChild(article);
  nodes.messageFeed.scrollTop = nodes.messageFeed.scrollHeight;

  return {
    article,
    phaseBadge,
    summary,
    fieldsNode,
    previewNode,
  };
}

function updateToolBlock(entry, block) {
  const phase = block.phase || "start";
  const display = block.display || {};

  // Update phase badge
  const tone = toolPhaseTone(phase);
  entry.phaseBadge.className = `badge badge-${tone}`;
  entry.phaseBadge.textContent = toolPhaseLabel(phase);

  // Update summary
  const summaryText = display.summary || block.detail || "";
  entry.summary.textContent = summaryText;
  entry.summary.classList.toggle("hidden", !summaryText);

  // Update fields
  if (display.fields && display.fields.length > 0) {
    entry.fieldsNode.classList.remove("hidden");
    entry.fieldsNode.innerHTML = "";
    display.fields.forEach(field => {
      const fieldEl = document.createElement("div");
      fieldEl.style.cssText = "display: flex; flex-direction: column; gap: var(--space-1); padding: var(--space-2); background: var(--bg-elevated); border-radius: var(--radius-sm);";

      const label = document.createElement("span");
      label.className = "text-tertiary";
      label.style.fontSize = "var(--text-xs)";
      label.textContent = field.label || "Field";
      fieldEl.appendChild(label);

      const value = document.createElement("span");
      value.className = "text-secondary";
      value.style.fontSize = "var(--text-sm)";
      value.textContent = field.value || "—";
      fieldEl.appendChild(value);

      entry.fieldsNode.appendChild(fieldEl);
    });
  } else {
    entry.fieldsNode.classList.add("hidden");
  }

  // Update preview
  renderPreviewLines(entry.previewNode, display.preview);

  nodes.messageFeed.scrollTop = nodes.messageFeed.scrollHeight;
}

function renderPreviewLines(node, preview) {
  if (!preview || !preview.text) {
    node.classList.add("hidden");
    return;
  }

  node.classList.remove("hidden");
  node.innerHTML = "";

  const label = document.createElement("div");
  label.className = "text-tertiary";
  label.style.cssText = "font-size: var(--text-xs); margin-bottom: var(--space-2); text-transform: uppercase; letter-spacing: 0.05em;";
  label.textContent = preview.kind === "diff" ? "Diff Preview" : "Preview";
  node.appendChild(label);

  if (preview.kind === "diff") {
    const container = document.createElement("div");
    container.style.cssText = "font-family: var(--font-mono); font-size: var(--text-xs); line-height: 1.5; overflow-x: auto;";

    for (const line of String(preview.text).split("\n")) {
      const lineNode = document.createElement("div");
      lineNode.style.cssText = "padding: 1px 0; white-space: pre;";

      if (line.startsWith("+") && !line.startsWith("+++")) {
        lineNode.style.backgroundColor = "var(--accent-success-soft)";
        lineNode.style.color = "var(--accent-success)";
      } else if (line.startsWith("-") && !line.startsWith("---")) {
        lineNode.style.backgroundColor = "var(--accent-error-soft)";
        lineNode.style.color = "var(--accent-error)";
      } else if (line.startsWith("@@")) {
        lineNode.style.color = "var(--accent-primary)";
      } else {
        lineNode.style.color = "var(--text-secondary)";
      }

      lineNode.textContent = line;
      container.appendChild(lineNode);
    }
    node.appendChild(container);
  } else {
    const pre = document.createElement("pre");
    pre.className = "text-secondary";
    pre.style.cssText = "font-size: var(--text-sm); white-space: pre-wrap; word-break: break-word; margin: 0;";
    pre.textContent = preview.text;
    node.appendChild(pre);
  }

  if (preview.truncated) {
    const tail = document.createElement("div");
    tail.className = "text-tertiary";
    tail.style.cssText = "font-size: var(--text-xs); margin-top: var(--space-2); font-style: italic;";
    tail.textContent = "truncated";
    node.appendChild(tail);
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Session Event Block (Rendered as system message)
// ─────────────────────────────────────────────────────────────────────────────

function appendSessionEventBlock(block) {
  const { article, content } = createMessageElement("system", {
    title: humanEventLabel(block.event),
    ts: block.ts
  });

  // Status badge
  const statusBadge = document.createElement("span");
  statusBadge.className = "badge badge-running";
  statusBadge.style.marginBottom = "var(--space-2)";
  statusBadge.textContent = humanStatusLabel(block.status);
  content.appendChild(statusBadge);

  // Summary
  const summary = document.createElement("div");
  summary.className = "text-secondary hidden";
  summary.style.cssText = "font-size: var(--text-sm); margin-top: var(--space-2);";
  content.appendChild(summary);

  // Fields
  const fieldsNode = document.createElement("div");
  fieldsNode.className = "hidden";
  fieldsNode.style.cssText = "display: grid; gap: var(--space-2); margin-top: var(--space-3);";
  content.appendChild(fieldsNode);

  // Body
  const bodyNode = document.createElement("div");
  bodyNode.className = "hidden";
  bodyNode.style.cssText = "margin-top: var(--space-3); padding: var(--space-3); background: var(--bg-base); border-radius: var(--radius-md);";
  content.appendChild(bodyNode);

  nodes.messageFeed.appendChild(article);
  nodes.messageFeed.scrollTop = nodes.messageFeed.scrollHeight;

  return {
    article,
    statusBadge,
    summary,
    fieldsNode,
    bodyNode,
  };
}

function updateSessionEventBlock(entry, block) {
  const tone = sessionEventTone(block.status);
  entry.statusBadge.className = `badge badge-${tone}`;
  entry.statusBadge.textContent = humanStatusLabel(block.status);

  if (block.summary) {
    entry.summary.classList.remove("hidden");
    entry.summary.textContent = block.summary;
  } else {
    entry.summary.classList.add("hidden");
  }

  if (block.fields && block.fields.length > 0) {
    entry.fieldsNode.classList.remove("hidden");
    entry.fieldsNode.innerHTML = "";
    block.fields.forEach(field => {
      const fieldEl = document.createElement("div");
      fieldEl.style.cssText = "display: flex; justify-content: space-between; align-items: baseline; gap: var(--space-2);";

      const label = document.createElement("span");
      label.className = "text-tertiary";
      label.style.fontSize = "var(--text-xs)";
      label.textContent = field.label || "Field";
      fieldEl.appendChild(label);

      const value = document.createElement("span");
      value.className = "text-secondary";
      value.style.fontSize = "var(--text-sm)";
      value.textContent = field.value || "—";
      fieldEl.appendChild(value);

      entry.fieldsNode.appendChild(fieldEl);
    });
  } else {
    entry.fieldsNode.classList.add("hidden");
  }

  if (block.body) {
    entry.bodyNode.classList.remove("hidden");
    entry.bodyNode.innerHTML = "";

    const label = document.createElement("div");
    label.className = "text-tertiary";
    label.style.cssText = "font-size: var(--text-xs); margin-bottom: var(--space-2); text-transform: uppercase; letter-spacing: 0.05em;";
    label.textContent = "Details";
    entry.bodyNode.appendChild(label);

    const pre = document.createElement("pre");
    pre.className = "text-secondary";
    pre.style.cssText = "font-size: var(--text-sm); white-space: pre-wrap; word-break: break-word; margin: 0;";
    pre.textContent = block.body;
    entry.bodyNode.appendChild(pre);
  } else {
    entry.bodyNode.classList.add("hidden");
  }

  nodes.messageFeed.scrollTop = nodes.messageFeed.scrollHeight;
}
