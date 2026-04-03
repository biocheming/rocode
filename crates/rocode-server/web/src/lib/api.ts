export async function api(path: string, options: RequestInit = {}): Promise<Response> {
  const headers = new Headers(options.headers);
  if (!headers.has("Content-Type") && options.body) {
    headers.set("Content-Type", "application/json");
  }
  const response = await fetch(path, { ...options, headers });
  if (!response.ok) {
    throw new Error(await response.text());
  }
  return response;
}

export async function apiJson<T>(path: string, options: RequestInit = {}): Promise<T> {
  const response = await api(path, options);
  return response.json() as Promise<T>;
}

export async function parseSSE(
  response: Response,
  onEvent: (eventName: string, data: unknown) => void,
): Promise<void> {
  if (!response.body) return;
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let eventName: string | null = null;
  let dataLines: string[] = [];

  const flush = () => {
    if (dataLines.length === 0) {
      eventName = null;
      return;
    }
    const data = dataLines.join("\n");
    dataLines = [];
    let parsed: unknown;
    try {
      parsed = JSON.parse(data);
    } catch {
      parsed = { raw: data };
    }
    onEvent(eventName ?? "message", parsed);
    eventName = null;
  };

  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split("\n");
    buffer = lines.pop() ?? "";
    for (const rawLine of lines) {
      const line = rawLine.endsWith("\r") ? rawLine.slice(0, -1) : rawLine;
      if (!line) {
        flush();
        continue;
      }
      if (line.startsWith("event:")) {
        eventName = line.slice(6).trim();
      } else if (line.startsWith("data:")) {
        dataLines.push(line.slice(5).trimStart());
      }
    }
  }

  flush();
}

export function formatError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return "Unknown error";
}
