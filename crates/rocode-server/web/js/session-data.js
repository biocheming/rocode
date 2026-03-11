// ── Session & Project Data ──────────────────────────────────────────────────

function projectKey(session) {
  if (session.project_id && session.project_id !== "default") return session.project_id;
  return session.directory || "default";
}

function projectLabel(session) {
  if (session.project_id && session.project_id !== "default") return baseName(session.project_id);
  return baseName(session.directory);
}

function normalizeSession(session) {
  return {
    id: session.id,
    title: session.title || "(untitled)",
    project_id: session.project_id || "default",
    directory: session.directory || "",
    updated: session.time && session.time.updated ? session.time.updated : Date.now(),
    share_url: session.share && session.share.url ? session.share.url : null,
    metadata: session.metadata || null,
  };
}

function sortSessions(items) {
  return items.sort((a, b) => Number(b.updated) - Number(a.updated));
}

function normalizeSessions(items) {
  return sortSessions((items || []).filter((s) => !s.parent_id).map(normalizeSession));
}

function upsertSessionSnapshot(session) {
  const normalized = normalizeSession(session);
  const index = state.sessions.findIndex((item) => item.id === normalized.id);
  if (index >= 0) {
    state.sessions[index] = { ...state.sessions[index], ...normalized };
  } else {
    state.sessions.push(normalized);
  }
  sortSessions(state.sessions);
  buildProjects();
  renderProjects();
  syncInteractionState();
  return normalized;
}

function buildProjects() {
  const map = new Map();
  for (const session of state.sessions) {
    const key = projectKey(session);
    if (!map.has(key)) {
      map.set(key, { key, label: projectLabel(session), sessions: [] });
    }
    map.get(key).sessions.push(session);
  }

  const query = (nodes.projectSearch.value || "").trim().toLowerCase();
  state.projects = Array.from(map.values())
    .map((project) => {
      if (!query) return project;
      const byProject = project.label.toLowerCase().includes(query);
      if (byProject) return project;
      const sessions = project.sessions.filter((s) => s.title.toLowerCase().includes(query));
      return { ...project, sessions };
    })
    .filter((project) => project.sessions.length > 0)
    .sort((a, b) => Number(b.sessions[0].updated) - Number(a.sessions[0].updated));
}

function currentSession() {
  return state.sessions.find((s) => s.id === state.selectedSession) || null;
}
