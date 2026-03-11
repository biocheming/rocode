// ── Utility Functions ───────────────────────────────────────────────────────

function timeGreeting() {
  const hour = new Date().getHours();
  if (hour < 6) return "Good night";
  if (hour < 12) return "Good morning";
  if (hour < 18) return "Good afternoon";
  return "Good evening";
}

function escapeHtml(input) {
  return String(input)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function formatTime(ts) {
  if (!ts) return "--";
  const date = new Date(Number(ts));
  if (Number.isNaN(date.getTime())) return "--";
  return date.toLocaleString();
}

function short(text, max = 42) {
  if (!text) return "(untitled)";
  const clean = String(text).trim();
  if (clean.length <= max) return clean;
  return clean.slice(0, max - 1) + "...";
}

function baseName(path) {
  if (!path) return "workspace";
  const chunks = String(path).split(/[\\/]/).filter(Boolean);
  return chunks[chunks.length - 1] || "workspace";
}

function compactPath(path, max = 52) {
  if (!path) return "workspace";
  let normalized = String(path).trim();
  normalized = normalized.replace(/^\/home\/[^/]+/, "~");
  if (normalized.length <= max) return normalized;
  return `...${normalized.slice(-(max - 3))}`;
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: {
      "Content-Type": "application/json",
      ...(options.headers || {}),
    },
    ...options,
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${response.status} ${response.statusText}: ${text}`);
  }
  return response;
}
