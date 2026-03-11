// ── Scheduler Stage Rendering ──────────────────────────────────────────────

function prettyStageName(stage) {
  if (!stage) return "Stage";
  return String(stage)
    .split(/[-_]/)
    .filter(Boolean)
    .map((chunk) => chunk.charAt(0).toUpperCase() + chunk.slice(1))
    .join(" ");
}

function schedulerStageTitle(block) {
  if (block.title && String(block.title).trim()) return String(block.title).trim();
  const profile = block.profile ? `${block.profile} · ` : "";
  return `${profile}${prettyStageName(block.stage)}`;
}

function schedulerStageText(block) {
  const raw = String(block.text || "");
  const lines = raw.split("\n");
  if (lines[0] && lines[0].startsWith("## ")) {
    return lines.slice(1).join("\n").trimStart();
  }
  return raw;
}

function schedulerStageTone(status) {
  if (status === "done") return "success";
  if (status === "blocked") return "error";
  if (status === "cancelled") return "error";
  if (status === "waiting") return "warning";
  return "warning";
}

function schedulerStageStatusLabel(status) {
  switch (status) {
    case "waiting":
      return "? waiting";
    case "running":
      return "@ running";
    case "cancelling":
      return "~ cancelling";
    case "cancelled":
      return "x cancelled";
    case "done":
      return "+ done";
    case "blocked":
      return "! blocked";
    default:
      return status ? `@ ${status}` : "@ running";
  }
}

function renderStageField(node, label, value) {
  node.replaceChildren();
  const labelNode = document.createElement("div");
  labelNode.className = "stage-field-label";
  labelNode.textContent = label;
  node.appendChild(labelNode);

  const valueNode = document.createElement("div");
  valueNode.className = "stage-field-value";
  valueNode.textContent = value || "—";
  node.appendChild(valueNode);
}

function renderStageActivity(node, value) {
  node.replaceChildren();
  if (!value || !String(value).trim()) {
    node.classList.add("hidden");
    return;
  }
  node.classList.remove("hidden");
  const labelNode = document.createElement("div");
  labelNode.className = "stage-section-label";
  labelNode.textContent = "Activity";
  node.appendChild(labelNode);

  const bodyNode = document.createElement("pre");
  bodyNode.className = "stage-section-body";
  bodyNode.textContent = value;
  node.appendChild(bodyNode);
}

function renderStageBody(node, value) {
  node.replaceChildren();
  const text = value ? String(value).trim() : "";
  if (!text) {
    node.classList.add("hidden");
    return;
  }
  node.classList.remove("hidden");
  const labelNode = document.createElement("div");
  labelNode.className = "stage-section-label";
  labelNode.textContent = "Body";
  node.appendChild(labelNode);

  const bodyNode = document.createElement("pre");
  bodyNode.className = "stage-section-body";
  bodyNode.textContent = text;
  node.appendChild(bodyNode);
}

function renderStageCapabilities(node, block) {
  const availableSkillCount = block.available_skill_count;
  const availableAgentCount = block.available_agent_count;
  const availableCategoryCount = block.available_category_count;
  const skills = block.active_skills;
  const agents = block.active_agents;
  const categories = block.active_categories;
  const hasCaps =
    availableSkillCount != null ||
    availableAgentCount != null ||
    availableCategoryCount != null ||
    (skills && skills.length > 0) ||
    (agents && agents.length > 0) ||
    (categories && categories.length > 0);

  if (!hasCaps) {
    node.classList.add("hidden");
    return;
  }
  node.classList.remove("hidden");
  node.replaceChildren();

  const available = [];
  if (availableSkillCount != null) available.push(`skills ${availableSkillCount}`);
  if (availableAgentCount != null) available.push(`agents ${availableAgentCount}`);
  if (availableCategoryCount != null) available.push(`categories ${availableCategoryCount}`);
  if (available.length > 0) {
    const row = document.createElement("div");
    row.className = "stage-caps-row";
    const label = document.createElement("span");
    label.className = "stage-field-label";
    label.textContent = "Available";
    row.appendChild(label);

    const summary = document.createElement("span");
    summary.className = "stage-caps-summary";
    summary.textContent = available.join(" · ");
    row.appendChild(summary);
    node.appendChild(row);
  }

  if (skills && skills.length > 0) {
    const row = document.createElement("div");
    row.className = "stage-caps-row";
    const label = document.createElement("span");
    label.className = "stage-field-label";
    label.textContent = "Active Skills";
    row.appendChild(label);
    for (const name of skills) {
      const chip = document.createElement("span");
      chip.className = "stage-cap-chip skill";
      chip.textContent = name;
      row.appendChild(chip);
    }
    node.appendChild(row);
  }

  if (agents && agents.length > 0) {
    const row = document.createElement("div");
    row.className = "stage-caps-row";
    const label = document.createElement("span");
    label.className = "stage-field-label";
    label.textContent = "Active Agents";
    row.appendChild(label);
    for (const name of agents) {
      const chip = document.createElement("span");
      chip.className = "stage-cap-chip agent";
      chip.textContent = name;
      row.appendChild(chip);
    }
    node.appendChild(row);
  }

  if (categories && categories.length > 0) {
    const row = document.createElement("div");
    row.className = "stage-caps-row";
    const label = document.createElement("span");
    label.className = "stage-field-label";
    label.textContent = "Active Categories";
    row.appendChild(label);
    for (const name of categories) {
      const chip = document.createElement("span");
      chip.className = "stage-cap-chip category";
      chip.textContent = name;
      row.appendChild(chip);
    }
    node.appendChild(row);
  }
}

function gateStatusLabel(status) {
  if (!status) return "running";
  if (status === "continue") return "? continue";
  if (status === "done") return "+ done";
  if (status === "blocked") return "! blocked";
  return status;
}

function decisionFieldValue(field) {
  if (!field) return "";
  return String(field.value || "");
}

function tokenDisplay(value) {
  return value === null || value === undefined ? "—" : String(value);
}

function stageSecondaryUsage(block) {
  const parts = [];
  if (block.reasoning_tokens !== null && block.reasoning_tokens !== undefined) {
    parts.push(`reasoning ${block.reasoning_tokens}`);
  }
  if (block.cache_read_tokens !== null && block.cache_read_tokens !== undefined) {
    parts.push(`cache read ${block.cache_read_tokens}`);
  }
  if (block.cache_write_tokens !== null && block.cache_write_tokens !== undefined) {
    parts.push(`cache write ${block.cache_write_tokens}`);
  }
  return parts.length ? parts.join(" · ") : null;
}

function appendSchedulerStage(block) {
  const article = document.createElement("article");
  article.className = "message scheduler-stage";

  const meta = document.createElement("div");
  meta.className = "message-meta";

  const titleNode = document.createElement("span");
  meta.appendChild(titleNode);

  const timeNode = document.createElement("span");
  meta.appendChild(timeNode);

  const chips = document.createElement("div");
  chips.className = "stage-chips";

  const dividerNode = document.createElement("div");
  dividerNode.className = "stage-divider";

  const stageChip = document.createElement("span");
  stageChip.className = "stage-chip primary";
  chips.appendChild(stageChip);

  const progressChip = document.createElement("span");
  progressChip.className = "stage-chip";
  chips.appendChild(progressChip);

  const stepChip = document.createElement("span");
  stepChip.className = "stage-chip";
  chips.appendChild(stepChip);

  const statusChip = document.createElement("span");
  statusChip.className = "stage-chip";
  chips.appendChild(statusChip);

  const tokenChip = document.createElement("span");
  tokenChip.className = "stage-chip";
  chips.appendChild(tokenChip);

  const grid = document.createElement("div");
  grid.className = "stage-grid";

  const focusNode = document.createElement("div");
  focusNode.className = "stage-field";
  grid.appendChild(focusNode);

  const eventNode = document.createElement("div");
  eventNode.className = "stage-field";
  grid.appendChild(eventNode);

  const waitingNode = document.createElement("div");
  waitingNode.className = "stage-field";
  grid.appendChild(waitingNode);

  const usageNode = document.createElement("div");
  usageNode.className = "stage-field stage-field-secondary";
  grid.appendChild(usageNode);

  const activityNode = document.createElement("div");
  activityNode.className = "stage-section stage-activity hidden";
  grid.appendChild(activityNode);

  const capsNode = document.createElement("div");
  capsNode.className = "stage-field stage-capabilities hidden";
  grid.appendChild(capsNode);

  const decisionNode = document.createElement("div");
  decisionNode.className = "stage-decision hidden";
  grid.appendChild(decisionNode);

  const bodyNode = document.createElement("div");
  bodyNode.className = "stage-section stage-body hidden";

  article.appendChild(meta);
  article.appendChild(dividerNode);
  article.appendChild(chips);
  article.appendChild(grid);
  article.appendChild(bodyNode);
  nodes.messageFeed.appendChild(article);
  nodes.messageFeed.scrollTop = nodes.messageFeed.scrollHeight;

  return {
    article,
    titleNode,
    timeNode,
    dividerNode,
    stageChip,
    progressChip,
    stepChip,
    statusChip,
    tokenChip,
    focusNode,
    eventNode,
    waitingNode,
    usageNode,
    activityNode,
    capsNode,
    decisionNode,
    bodyNode,
  };
}

function renderDecisionBlock(node, decision) {
  node.replaceChildren();
  if (!decision) {
    node.classList.add("hidden");
    return;
  }
  node.classList.remove("hidden");
  const spec = decision.spec || {};
  node.dataset.sectionSpacing = spec.section_spacing || "loose";

  const heading = document.createElement("div");
  heading.className = "decision-title";
  heading.textContent = decision.title || "Decision";
  node.appendChild(heading);

  const fields = Array.isArray(decision.fields) ? decision.fields : [];
  for (const field of fields) {
    const row = document.createElement("div");
    row.className = "decision-row";

    const label = document.createElement("span");
    label.className = "decision-label";
    label.textContent = `${field.label}:`;
    row.appendChild(label);

    const value = document.createElement("span");
    value.className = `decision-value ${field.tone || ""}`.trim();
    value.textContent = decisionFieldValue(field);
    if (field.tone === "status") {
      value.dataset.status = String(field.value || "").toLowerCase();
    }
    row.appendChild(value);
    node.appendChild(row);
  }

  const sections = Array.isArray(decision.sections) ? decision.sections : [];
  for (const section of sections) {
    const title = document.createElement("div");
    title.className = "decision-section-title";
    title.textContent = section.title || "Section";
    node.appendChild(title);

    const body = document.createElement("pre");
    body.className = "decision-section-body";
    body.textContent = section.body || "";
    node.appendChild(body);
  }
}

function updateSchedulerStage(entry, block) {
  const tone = schedulerStageTone(block.status);
  entry.article.classList.remove("warning", "success", "error");
  entry.article.classList.add(tone);
  entry.titleNode.textContent = schedulerStageTitle(block);
  entry.timeNode.textContent = formatTime(block.ts || Date.now());
  const decisionSpec = (block.decision && block.decision.spec) || {};
  entry.dividerNode.classList.toggle("hidden", decisionSpec.show_header_divider === false);
  entry.stageChip.textContent = block.stage ? `stage ${block.stage}` : "stage";
  entry.progressChip.textContent =
    block.stage_index && block.stage_total
      ? `${block.stage_index}/${block.stage_total}`
      : "live";
  entry.stepChip.textContent = block.step ? `step ${block.step}` : "step —";
  entry.statusChip.className = `stage-chip status-chip ${block.status || "running"}`;
  entry.statusChip.textContent = schedulerStageStatusLabel(block.status);
  entry.tokenChip.textContent = `tokens ${tokenDisplay(block.prompt_tokens)}/${tokenDisplay(block.completion_tokens)}`;
  renderStageField(entry.focusNode, "Focus", block.focus);
  renderStageField(entry.eventNode, "Last", block.last_event);
  renderStageField(entry.waitingNode, "Waiting", block.waiting_on);
  renderStageField(entry.usageNode, "Usage", stageSecondaryUsage(block));
  renderStageActivity(entry.activityNode, block.activity);
  renderStageCapabilities(entry.capsNode, block);
  if (block.decision) {
    renderDecisionBlock(entry.decisionNode, block.decision);
  } else {
    renderDecisionBlock(entry.decisionNode, null);
  }
  renderStageBody(entry.bodyNode, block.decision ? "" : schedulerStageText(block));
  nodes.messageFeed.scrollTop = nodes.messageFeed.scrollHeight;
}

function schedulerStageBlockFromMessage(message, body) {
  const meta = message && message.metadata ? message.metadata : null;
  if (!meta || !meta.scheduler_stage) return null;
  return {
    kind: "scheduler_stage",
    id: message.id,
    profile: meta.resolved_scheduler_profile || meta.scheduler_profile || null,
    stage: meta.scheduler_stage,
    title: schedulerStageTitle({
      profile: meta.resolved_scheduler_profile || meta.scheduler_profile || null,
      stage: meta.scheduler_stage,
      title: body && String(body).trim().startsWith("## ")
        ? String(body).trim().split("\n")[0].replace(/^##\s*/, "")
        : null,
    }),
    text: body || "",
    stage_index: meta.scheduler_stage_index || null,
    stage_total: meta.scheduler_stage_total || null,
    step: meta.scheduler_stage_step || null,
    status: meta.scheduler_stage_status || null,
    focus: meta.scheduler_stage_focus || null,
    last_event: meta.scheduler_stage_last_event || null,
    waiting_on: meta.scheduler_stage_waiting_on || null,
    activity: meta.scheduler_stage_activity || null,
    available_skill_count: meta.scheduler_stage_available_skill_count ?? null,
    available_agent_count: meta.scheduler_stage_available_agent_count ?? null,
    available_category_count: meta.scheduler_stage_available_category_count ?? null,
    active_skills: Array.isArray(meta.scheduler_stage_active_skills) ? meta.scheduler_stage_active_skills : null,
    active_agents: Array.isArray(meta.scheduler_stage_active_agents) ? meta.scheduler_stage_active_agents : null,
    active_categories: Array.isArray(meta.scheduler_stage_active_categories) ? meta.scheduler_stage_active_categories : null,
    prompt_tokens: meta.scheduler_stage_prompt_tokens ?? null,
    completion_tokens: meta.scheduler_stage_completion_tokens ?? null,
    reasoning_tokens: meta.scheduler_stage_reasoning_tokens ?? null,
    cache_read_tokens: meta.scheduler_stage_cache_read_tokens ?? null,
    cache_write_tokens: meta.scheduler_stage_cache_write_tokens ?? null,
    decision:
      meta.scheduler_decision_title || meta.scheduler_decision_fields || meta.scheduler_decision_sections
        ? {
            kind: meta.scheduler_decision_kind || null,
            title: meta.scheduler_decision_title || "Decision",
            spec: meta.scheduler_decision_spec || {
              version: "decision-card/v1",
              show_header_divider: true,
              field_order: "as-provided",
              field_label_emphasis: "bold",
              status_palette: "semantic",
              section_spacing: "loose",
              update_policy: "stable-shell-live-runtime-append-decision",
            },
            fields: Array.isArray(meta.scheduler_decision_fields) ? meta.scheduler_decision_fields : [],
            sections: Array.isArray(meta.scheduler_decision_sections) ? meta.scheduler_decision_sections : [],
          }
        : null,
    ts: message.created_at,
  };
}
