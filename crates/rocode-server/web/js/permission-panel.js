function humanPermissionStatusLabel(status) {
  switch (String(status || "").toLowerCase()) {
    case "resolved":
      return "Resolved";
    case "rejected":
      return "Rejected";
    default:
      return "Awaiting Approval";
  }
}

function permissionStatusTone(status) {
  const normalized = String(status || "").toLowerCase();
  if (normalized === "resolved") return "done";
  if (normalized === "rejected") return "error";
  return "waiting";
}

function openPermissionPanel(interaction) {
  if (!interaction || !interaction.permission_id) return;
  state.activePermissionInteraction = interaction;
  state.permissionSubmitting = false;
  renderPermissionPanel();
  nodes.permissionPanel.classList.remove("hidden");
}

function closePermissionPanel() {
  if (state.permissionSubmitting) return;
  nodes.permissionPanel.classList.add("hidden");
  state.activePermissionInteraction = null;
  nodes.permissionBody.replaceChildren();
}

function renderPermissionPanel() {
  const interaction = state.activePermissionInteraction;
  nodes.permissionBody.replaceChildren();
  if (!interaction) return;

  nodes.permissionPanelTitle.textContent = "Permission Request";
  nodes.permissionPanelStatus.textContent = humanPermissionStatusLabel(interaction.status);
  nodes.permissionPanelStatus.className = `badge status-chip ${permissionStatusTone(interaction.status)}`;
  nodes.permissionPanelMeta.textContent = `${interaction.permission || "permission"} · ${interaction.permission_id}`;
  nodes.permissionRejectBtn.disabled = state.permissionSubmitting;
  nodes.permissionAllowBtn.disabled = state.permissionSubmitting;
  nodes.permissionAlwaysBtn.disabled = state.permissionSubmitting;
  nodes.permissionAlwaysBtn.textContent = state.permissionSubmitting ? "Submitting..." : "Allow Always";

  const sections = [
    ["Message", interaction.message || "Permission required"],
    ["Permission", interaction.permission || "unknown"],
    ["Patterns", Array.isArray(interaction.patterns) && interaction.patterns.length ? interaction.patterns.join(", ") : "n/a"],
  ];

  sections.forEach(([label, value]) => {
    const card = document.createElement("section");
    card.className = "question-item";

    const header = document.createElement("div");
    header.className = "question-item-header";
    const title = document.createElement("div");
    title.className = "question-item-label";
    title.textContent = label;
    header.appendChild(title);
    card.appendChild(header);

    const text = document.createElement("div");
    text.className = "question-item-text";
    text.textContent = value;
    card.appendChild(text);

    nodes.permissionBody.appendChild(card);
  });

  if (interaction.command || interaction.filepath) {
    const card = document.createElement("section");
    card.className = "question-item";
    const header = document.createElement("div");
    header.className = "question-item-header";
    const title = document.createElement("div");
    title.className = "question-item-label";
    title.textContent = interaction.command ? "Command" : "Path";
    header.appendChild(title);
    card.appendChild(header);

    const text = document.createElement("pre");
    text.className = "question-item-text";
    text.textContent = interaction.command || interaction.filepath;
    card.appendChild(text);
    nodes.permissionBody.appendChild(card);
  }
}

async function submitPermissionInteractionReply(reply) {
  const interaction = state.activePermissionInteraction;
  if (!interaction || !interaction.permission_id || state.permissionSubmitting) return;

  state.permissionSubmitting = true;
  renderPermissionPanel();
  try {
    await api(`/permission/${interaction.permission_id}/reply`, {
      method: "POST",
      body: JSON.stringify({
        reply,
        message:
          reply === "reject"
            ? "rejected"
            : reply === "always"
              ? "approved always"
              : "approved",
      }),
    });
  } finally {
    state.permissionSubmitting = false;
  }
}

function permissionInteractionFromLiveEvent(payload) {
  const info = payload && payload.info ? payload.info : {};
  const input = info && info.input ? info.input : {};
  const metadata = input && input.metadata ? input.metadata : {};
  const patterns = Array.isArray(input.patterns) ? input.patterns : [];

  return {
    type: "permission",
    status: "pending",
    permission_id: payload.permissionID || payload.permissionId || info.id,
    session_id: payload.sessionID || payload.sessionId || info.session_id || null,
    permission: input.permission || info.tool || null,
    message: info.message || "Permission required",
    patterns,
    command: metadata.command || null,
    filepath: metadata.filepath || metadata.path || null,
  };
}
