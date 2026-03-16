import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
// Concatenate JS modules in the same order as web.rs
const jsDir = path.join(__dirname, "js");
const jsModules = [
  "constants.js",
  "utils.js",
  "session-data.js",
  "runtime-state.js",
  "execution-panel.js",
  "stage-inspector.js",
  "message-render.js",
  "scheduler-stage.js",
  "question-panel.js",
  "permission-panel.js",
  "output-blocks.js",
  "sidebar.js",
  "settings.js",
  "session-actions.js",
  "commands.js",
  "streaming.js",
  "global-events.js",
  "bootstrap.js",
];
const appSource = jsModules
  .map((name) => fs.readFileSync(path.join(jsDir, name), "utf8"))
  .join("\n");
const schedulerFixture = JSON.parse(
  fs.readFileSync(
    path.join(__dirname, "..", "..", "rocode-command", "governance", "scheduler_stage_fixture.json"),
    "utf8",
  ),
);

class FakeClassList {
  constructor(element) {
    this.element = element;
    this.classes = new Set();
  }

  setFromString(value) {
    this.classes = new Set(String(value || "").split(/\s+/).filter(Boolean));
    this.#sync();
  }

  add(...tokens) {
    for (const token of tokens) {
      if (token) this.classes.add(token);
    }
    this.#sync();
  }

  remove(...tokens) {
    for (const token of tokens) {
      this.classes.delete(token);
    }
    this.#sync();
  }

  toggle(token, force) {
    if (force === true) this.classes.add(token);
    else if (force === false) this.classes.delete(token);
    else if (this.classes.has(token)) this.classes.delete(token);
    else this.classes.add(token);
    this.#sync();
    return this.classes.has(token);
  }

  contains(token) {
    return this.classes.has(token);
  }

  #sync() {
    this.element._className = Array.from(this.classes).join(" ");
  }
}

class FakeElement {
  constructor(tagName, ownerDocument) {
    this.tagName = String(tagName || "div").toUpperCase();
    this.ownerDocument = ownerDocument;
    this.parentNode = null;
    this.children = [];
    this.dataset = {};
    this.style = {};
    this.attributes = new Map();
    this.eventListeners = new Map();
    this._className = "";
    this.classList = new FakeClassList(this);
    this._textContent = null;
    this._innerHTML = "";
    this.id = "";
    this.value = "";
    this.checked = false;
    this.disabled = false;
    this.placeholder = "";
    this.type = "";
    this.name = "";
    this.rows = 0;
    this.scrollTop = 0;
    this.scrollHeight = 0;
  }

  get className() {
    return this._className;
  }

  set className(value) {
    this.classList.setFromString(value);
  }

  get textContent() {
    if (this._textContent !== null) return this._textContent;
    return this.children.map((child) => child.textContent).join("");
  }

  set textContent(value) {
    this.children = [];
    this._innerHTML = "";
    this._textContent = String(value ?? "");
  }

  get innerHTML() {
    if (this._innerHTML) return this._innerHTML;
    if (this._textContent !== null) return this._textContent;
    return this.children.map((child) => child.textContent).join("");
  }

  set innerHTML(value) {
    this.children = [];
    this._textContent = null;
    this._innerHTML = String(value ?? "");
  }

  appendChild(child) {
    if (!(child instanceof FakeElement)) {
      throw new Error("FakeElement only supports FakeElement children");
    }
    child.parentNode = this;
    this.children.push(child);
    this._textContent = null;
    this._innerHTML = "";
    this.scrollHeight = this.children.length;
    return child;
  }

  replaceChildren(...children) {
    this.children = [];
    this._textContent = null;
    this._innerHTML = "";
    for (const child of children) {
      if (child instanceof FakeElement) this.appendChild(child);
    }
  }

  setAttribute(name, value) {
    const normalized = String(name);
    const stringValue = String(value ?? "");
    this.attributes.set(normalized, stringValue);
    if (normalized === "id") {
      this.id = stringValue;
      this.ownerDocument.registerElement(this);
    } else if (normalized === "class") {
      this.className = stringValue;
    } else if (normalized === "name") {
      this.name = stringValue;
    } else if (normalized === "for") {
      this.htmlFor = stringValue;
    }
  }

  addEventListener(type, handler) {
    if (!this.eventListeners.has(type)) {
      this.eventListeners.set(type, []);
    }
    this.eventListeners.get(type).push(handler);
  }

  dispatchEvent(event) {
    const handlers = this.eventListeners.get(event.type) || [];
    for (const handler of handlers) handler(event);
  }

  focus() {
    this.ownerDocument.activeElement = this;
  }

  querySelector(selector) {
    return this.querySelectorAll(selector)[0] || null;
  }

  querySelectorAll(selector) {
    const selectors = String(selector)
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean);
    const results = [];
    const visit = (node) => {
      for (const child of node.children) {
        if (selectors.some((entry) => matchesSelector(child, entry))) {
          results.push(child);
        }
        visit(child);
      }
    };
    visit(this);
    return results;
  }
}

class FakeDocument {
  constructor() {
    this.elementsById = new Map();
    this.activeElement = null;
    this.body = new FakeElement("body", this);
    this.eventListeners = new Map();
  }

  createElement(tagName) {
    return new FakeElement(tagName, this);
  }

  createTextNode(text) {
    const node = new FakeElement("#text", this);
    node.textContent = String(text);
    return node;
  }

  getElementById(id) {
    const key = String(id);
    if (!this.elementsById.has(key)) {
      const element = new FakeElement("div", this);
      element.id = key;
      this.registerElement(element);
      this.body.appendChild(element);
    }
    return this.elementsById.get(key);
  }

  querySelector(selector) {
    return this.body.querySelector(selector);
  }

  querySelectorAll(selector) {
    return this.body.querySelectorAll(selector);
  }

  addEventListener(type, handler) {
    if (!this.eventListeners.has(type)) {
      this.eventListeners.set(type, []);
    }
    this.eventListeners.get(type).push(handler);
  }

  registerElement(element) {
    if (element.id) this.elementsById.set(element.id, element);
  }
}

function matchesSelector(element, selector) {
  if (!selector) return false;
  if (selector.startsWith("#")) {
    return element.id === selector.slice(1);
  }
  if (selector.startsWith(".")) {
    return element.classList.contains(selector.slice(1));
  }

  const attrMatch = selector.match(/^([a-zA-Z0-9_-]+)(?:\[name="([^"]+)"\])?(?::checked)?$/);
  if (attrMatch) {
    const [, tagName, name] = attrMatch;
    if (element.tagName !== tagName.toUpperCase()) return false;
    if (name && element.name !== name) return false;
    if (selector.endsWith(":checked") && !element.checked) return false;
    return true;
  }

  return element.tagName === selector.toUpperCase();
}

function responseOf(data) {
  return {
    ok: true,
    status: 200,
    statusText: "OK",
    async json() {
      return data;
    },
    async text() {
      return JSON.stringify(data);
    },
  };
}

function buildContext(routeMap = new Map()) {
  const document = new FakeDocument();
  const storage = new Map();
  const testApi = {};

  const context = {
    console,
    document,
    navigator: { clipboard: { async writeText() {} } },
    window: null,
    localStorage: {
      getItem(key) {
        return storage.has(key) ? storage.get(key) : null;
      },
      setItem(key, value) {
        storage.set(String(key), String(value));
      },
      removeItem(key) {
        storage.delete(String(key));
      },
    },
    fetch: async (url) => {
      const key = String(url);
      if (!routeMap.has(key)) {
        throw new Error(`Unexpected fetch: ${key}`);
      }
      return responseOf(routeMap.get(key));
    },
    setTimeout,
    clearTimeout,
    Date,
    Map,
    Set,
    Array,
    Object,
    String,
    Number,
    Boolean,
    JSON,
    Math,
    Promise,
    URL,
    TextDecoder,
    TextEncoder,
    prompt: () => null,
    confirm: () => false,
    __ROCODE_WEB_DISABLE_BOOTSTRAP__: true,
    __ROCODE_WEB_TEST_API__: testApi,
  };

  context.window = context;
  context.globalThis = context;

  return { context, testApi, document };
}

function createHarness(routeMap = new Map()) {
  const { context, testApi } = buildContext(routeMap);
  vm.createContext(context);
  vm.runInContext(appSource, context, { filename: "app.js" });
  return { api: testApi, context };
}

function text(node) {
  return node ? node.textContent : "";
}

test("web history replay renders canonical scheduler stage card from shared fixture", async () => {
  const routes = new Map([
    [
      "/session/session-1/message",
      [
        {
          id: "msg-stage",
          role: "assistant",
          created_at: 1710000000000,
          metadata: schedulerFixture.metadata,
          parts: [{ type: "text", text: schedulerFixture.message_text }],
        },
      ],
    ],
    ["/session/session-1/executions", { active_count: 0 }],
    ["/session/session-1/recovery", { entries: [] }],
  ]);

  const { api } = createHarness(routes);
  api.state.selectedSession = "session-1";
  api.state.sessions = [
    {
      id: "session-1",
      title: "Atlas governance",
      directory: "/tmp/workspace",
      updated: 1710000000000,
      metadata: { scheduler_profile: "atlas" },
    },
  ];

  await api.loadMessages();

  const article = api.nodes.messageFeed.children[0];
  assert.ok(article, "scheduler stage article should exist");
  assert.ok(article.classList.contains("scheduler-stage"));
  assert.match(text(article), /Atlas · Coordination Gate/);
  assert.match(text(article), /stage coordination-gate/);
  assert.match(text(article), /2\/3/);
  assert.match(text(article), /step 4/);
  assert.match(text(article), /\? waiting/);
  assert.match(text(article), /tokens 1200\/320/);
  assert.match(text(article), /Decision pending on the unresolved task ledger\./);
  assert.equal(text(article).includes("## Atlas · Coordination Gate"), false);
  assert.match(text(article), /skills 8 · agents 4 · categories 2/);
  assert.match(text(article), /debug/);
  assert.match(text(article), /qa/);
  assert.match(text(article), /oracle/);
});

test("web live scheduler stage renders structured decision card under shared renderer rules", () => {
  const { api } = createHarness();
  const block = {
    ...schedulerFixture.payload,
    id: "live-stage-1",
    decision: {
      kind: "gate",
      title: "Decision",
      spec: {
        version: "decision-card/v1",
        show_header_divider: false,
        field_order: "as-provided",
        field_label_emphasis: "bold",
        status_palette: "semantic",
        section_spacing: "tight",
        update_policy: "stable-shell-live-runtime-append-decision",
      },
      fields: [
        { label: "Outcome", value: "continue", tone: "status" },
        { label: "Owner", value: "atlas", tone: null },
      ],
      sections: [
        { title: "Why", body: "Task B still lacks evidence." },
        { title: "Next Action", body: "Run one more worker round on task B." },
      ],
    },
  };

  api.applyOutputBlock(block);

  const article = api.nodes.messageFeed.children[0];
  const divider = article.querySelector(".stage-divider");
  const decision = article.querySelector(".stage-decision");
  const body = article.querySelector(".stage-body");
  const statusValue = article
    .querySelectorAll(".decision-value")
    .find((node) => node.dataset.status === "continue");

  assert.ok(decision, "decision card should render");
  assert.equal(decision.classList.contains("hidden"), false);
  assert.equal(decision.dataset.sectionSpacing, "tight");
  assert.equal(divider.classList.contains("hidden"), true);
  assert.equal(body.classList.contains("hidden"), true);
  assert.match(text(decision), /Decision/);
  assert.match(text(decision), /Outcome:/);
  assert.match(text(decision), /continue/);
  assert.match(text(decision), /Why/);
  assert.match(text(decision), /Task B still lacks evidence\./);
  assert.equal(statusValue.dataset.status, "continue");
});

test("web live question event opens the same question overlay with options and other input", () => {
  const { api, context } = createHarness();
  const interaction = api.interactionFromLiveQuestionEvent({
    requestId: "question-1",
    questions: [
      {
        header: "Coordination Gate",
        question: "Should Atlas continue execution?",
        multiple: false,
        options: [{ label: "Yes" }, { label: "No" }],
      },
    ],
  });

  api.openQuestionPanel(interaction);

  assert.equal(api.nodes.questionPanel.classList.contains("hidden"), false);
  assert.equal(api.nodes.questionPanelTitle.textContent, "Answer Question");
  assert.match(api.nodes.questionPanelStatus.textContent, /Awaiting Answer/);
  assert.match(api.nodes.questionPanelMeta.textContent, /question-1/);
  assert.equal(api.nodes.questionList.querySelectorAll('input[name="question-option-0"]').length, 2);

  const customInput = api.nodes.questionList.querySelector("#question-custom-0");
  assert.ok(customInput, "custom answer box should exist");
  assert.match(customInput.placeholder, /none of the options fit/i);
  assert.ok(
    context.document.activeElement === api.nodes.questionList.querySelector("input, textarea"),
    "first interactive control should receive focus",
  );
});

test("web global permission event opens the shared permission overlay", () => {
  const { api } = createHarness();
  api.state.selectedSession = "session-1";

  api.handleGlobalServerEvent("message", {
    type: "permission.requested",
    sessionID: "session-1",
    permissionID: "perm-1",
    info: {
      message: "Tool wants to write a file",
      input: {
        permission: "write",
        patterns: ["/tmp/demo.txt"],
        metadata: {
          filepath: "/tmp/demo.txt",
        },
      },
    },
  });

  assert.equal(api.nodes.permissionPanel.classList.contains("hidden"), false);
  assert.match(api.nodes.permissionPanelMeta.textContent, /perm-1/);
  assert.match(api.nodes.permissionBody.textContent, /Tool wants to write a file/);
  assert.match(api.nodes.permissionBody.textContent, /\/tmp\/demo\.txt/);
});

test("web ui preferences apply from config without localStorage", () => {
  const { api } = createHarness();
  api.state.modes = [{ key: "agent:atlas", id: "atlas", name: "Atlas", kind: "agent" }];

  api.applyWebUiPreferences({
    uiPreferences: {
      webTheme: "graphite",
      webMode: "agent:atlas",
      showThinking: true,
    },
  });

  assert.equal(api.state.selectedTheme, "graphite");
  assert.equal(api.state.selectedModeKey, "agent:atlas");
  assert.equal(api.state.showThinking, true);
  assert.equal(api.nodes.shell.dataset.theme, "graphite");
});

test("web reasoning blocks obey showThinking preference", () => {
  const { api } = createHarness();

  api.applyWebUiPreferences({
    uiPreferences: {
      showThinking: false,
    },
  });
  api.applyOutputBlock({
    kind: "reasoning",
    phase: "full",
    text: "hidden thought",
  });
  assert.equal(api.nodes.messageFeed.children.length, 0);

  api.applyWebUiPreferences({
    uiPreferences: {
      showThinking: true,
    },
  });
  api.applyOutputBlock({
    kind: "reasoning",
    phase: "full",
    text: "visible thought",
  });
  assert.match(api.nodes.messageFeed.textContent, /visible thought/);
});

test("web output_block events route by session id and do not leak child content into parent feed", () => {
  const { api } = createHarness();
  api.state.selectedSession = "root-session";

  const handledWhileRootFocused = api.applyOutputBlockEvent({
    type: "output_block",
    sessionID: "child-session",
    block: {
      kind: "message",
      phase: "full",
      role: "assistant",
      text: "child content",
    },
  });

  assert.equal(handledWhileRootFocused, false);
  assert.equal(api.nodes.messageFeed.children.length, 0);

  api.state.selectedSession = "child-session";
  const handledWhileChildFocused = api.applyOutputBlockEvent({
    type: "output_block",
    sessionID: "child-session",
    block: {
      kind: "message",
      phase: "full",
      role: "assistant",
      text: "child content",
    },
  });

  assert.equal(handledWhileChildFocused, true);
  assert.equal(api.nodes.messageFeed.children.length, 1);
  assert.match(api.nodes.messageFeed.textContent, /child content/);
});

test("global server events forward output_block to the selected child session when idle", () => {
  const { api } = createHarness();
  api.state.selectedSession = "child-session";
  api.state.streaming = false;

  api.handleGlobalServerEvent("output_block", {
    type: "output_block",
    sessionID: "child-session",
    block: {
      kind: "message",
      phase: "full",
      role: "assistant",
      text: "child live content",
    },
  });

  assert.match(api.nodes.messageFeed.textContent, /child live content/);
});

test("global server events do not double-render output_block while local stream is active", () => {
  const { api } = createHarness();
  api.state.selectedSession = "child-session";
  api.state.streaming = true;

  api.handleGlobalServerEvent("output_block", {
    type: "output_block",
    sessionID: "child-session",
    block: {
      kind: "message",
      phase: "full",
      role: "assistant",
      text: "duplicate content",
    },
  });

  assert.equal(api.nodes.messageFeed.children.length, 0);
});

test("web stream usage accepts zero values without keeping stale totals", () => {
  const { api } = createHarness();
  api.state.promptTokens = 9;
  api.state.completionTokens = 4;

  api.applyStreamUsage({
    prompt_tokens: 0,
    completion_tokens: 2,
  });

  assert.equal(api.state.promptTokens, 0);
  assert.equal(api.state.completionTokens, 2);
  assert.match(api.nodes.tokenUsage.textContent, /tokens: 0 \/ 2/);
});

test("web queue item block renders a visible queue card", () => {
  const { api } = createHarness();

  api.applyOutputBlock({
    kind: "queue_item",
    position: 2,
    text: "run verification",
    display: {
      summary: "Queued [2] run verification",
    },
  });

  const article = api.nodes.messageFeed.children[0];
  assert.ok(article, "queue item article should exist");
  assert.match(text(article), /Queued #2/);
  assert.match(text(article), /Queued \[2\] run verification/);
});

test("web inspect block updates the stage inspector panel", () => {
  const { api } = createHarness();

  api.applyOutputBlock({
    kind: "inspect",
    stage_ids: ["stage_plan_001", "stage_impl_002"],
    filter_stage_id: "stage_plan_001",
    events: [
      {
        ts: 1710000000000,
        event_type: "stage_started",
        execution_id: "exec_1",
        stage_id: "stage_plan_001",
      },
    ],
  });

  assert.equal(api.nodes.stageInspectorPanel.classList.contains("hidden"), false);
  assert.match(api.nodes.stageInspectorPanel.innerHTML, /Stage Inspector/);
  assert.match(api.nodes.stageInspectorPanel.innerHTML, /stage_plan_001/);
  assert.match(api.nodes.stageInspectorPanel.innerHTML, /stage_started/);
});

test("web stage streaming keeps concurrent same-name stages separate by stage_id", () => {
  const { api } = createHarness();

  api.applyOutputBlock({
    kind: "scheduler_stage",
    stage_id: "stage_a",
    profile: "atlas",
    stage: "plan",
    title: "Atlas · Plan",
    status: "running",
    step: 1,
  });
  api.applyOutputBlock({
    kind: "scheduler_stage",
    stage_id: "stage_b",
    profile: "atlas",
    stage: "plan",
    title: "Atlas · Plan",
    status: "running",
    step: 2,
  });

  const articles = api.nodes.messageFeed.querySelectorAll(".scheduler-stage");
  assert.equal(articles.length, 2, "same-name stages should not overwrite each other");
});

test("web tool block renders historical question interaction state", () => {
  const { api } = createHarness();

  api.applyOutputBlock({
    kind: "tool",
    id: "call_question_2",
    name: "question",
    phase: "done",
    detail: "User response received",
    interaction: {
      type: "question",
      status: "answered",
      can_reply: false,
      can_reject: false,
    },
  });

  const article = api.nodes.messageFeed.children[0];
  assert.ok(article, "tool article should exist");
  assert.match(text(article), /Answered/);
});

// ── M5: Governance tests for stage inspector and multi-stage consistency ──

test("web stage inspector renders stage tabs from event stage list", async () => {
  const routes = new Map([
    [
      "/session/session-gov/events/stages",
      ["stage_plan_001", "stage_impl_002", "stage_review_003"],
    ],
    [
      "/session/session-gov/events?stage_id=stage_plan_001&limit=200",
      [
        {
          event_id: "evt_plan_001",
          scope: "stage",
          stage_id: "stage_plan_001",
          execution_id: "exec_plan_stage",
          event_type: "stage_started",
          ts: 1710000000000,
          payload: { stage: "planning", index: 0 },
        },
        {
          event_id: "evt_plan_002",
          scope: "agent",
          stage_id: "stage_plan_001",
          execution_id: "exec_plan_agent",
          event_type: "agent_started",
          ts: 1710000000100,
          payload: { agent: "planner" },
        },
      ],
    ],
  ]);

  const { api, context } = createHarness(routes);
  api.state.selectedSession = "session-gov";

  // Call renderStageInspector directly (it's a global function in the JS context)
  await context.renderStageInspector("session-gov");

  const panel = api.nodes.stageInspectorPanel;
  assert.equal(panel.classList.contains("hidden"), false, "panel should be visible");
  assert.match(panel.innerHTML, /Stage Inspector/, "should show header");
  assert.match(panel.innerHTML, /3 stages/, "should show stage count");
  assert.match(panel.innerHTML, /stage_plan_001/, "should have planning stage tab");
  assert.match(panel.innerHTML, /stage_impl_002/, "should have implementation stage tab");
  assert.match(panel.innerHTML, /stage_review_003/, "should have review stage tab");
});

test("web stage inspector hides when no stages exist", async () => {
  const routes = new Map([
    ["/session/session-empty/events/stages", []],
  ]);

  const { api, context } = createHarness(routes);
  api.state.selectedSession = "session-empty";

  await context.renderStageInspector("session-empty");

  const panel = api.nodes.stageInspectorPanel;
  assert.ok(panel.classList.contains("hidden"), "panel should be hidden when no stages");
});

test("web stage inspector hides when session is null", async () => {
  const { api, context } = createHarness();

  await context.renderStageInspector(null);

  const panel = api.nodes.stageInspectorPanel;
  assert.ok(panel.classList.contains("hidden"), "panel should be hidden for null session");
});

test("web multi-stage concurrent rendering displays all stage cards from fixture", async () => {
  // Load the multi-agent fixture
  const fixtureRaw = fs.readFileSync(
    path.join(__dirname, "..", "..", "rocode-command", "governance", "multi_agent_replay_fixture.json"),
    "utf8",
  );
  const fixture = JSON.parse(fixtureRaw);

  const messages = fixture.stages.map((entry, idx) => ({
    id: `msg-stage-${idx}`,
    role: "assistant",
    created_at: 1710000000000 + idx * 10000,
    metadata: entry.metadata,
    parts: [{ type: "text", text: entry.message_text }],
  }));

  const routes = new Map([
    ["/session/session-multi/message", messages],
    ["/session/session-multi/executions", { active_count: 0 }],
    ["/session/session-multi/recovery", { entries: [] }],
  ]);

  const { api } = createHarness(routes);
  api.state.selectedSession = "session-multi";
  api.state.sessions = [
    {
      id: "session-multi",
      title: "Multi-stage governance test",
      directory: "/tmp/workspace",
      updated: 1710000000000,
      metadata: { scheduler_profile: "atlas" },
    },
  ];

  await api.loadMessages();

  const articles = api.nodes.messageFeed.children;
  assert.equal(articles.length, fixture.expected.total_stages, "should render all 3 stage articles");

  // Stage 0: Planning
  assert.ok(articles[0].classList.contains("scheduler-stage"), "first article should be scheduler-stage");
  assert.match(text(articles[0]), /Atlas · Planning/);
  assert.match(text(articles[0]), /stage planning/);
  assert.match(text(articles[0]), /step 2/);

  // Stage 1: Implementation
  assert.ok(articles[1].classList.contains("scheduler-stage"));
  assert.match(text(articles[1]), /Atlas · Implementation/);
  assert.match(text(articles[1]), /step 3/);

  // Stage 2: Review (has decision)
  assert.ok(articles[2].classList.contains("scheduler-stage"));
  assert.match(text(articles[2]), /Atlas · Review/);
  assert.match(text(articles[2]), /\? waiting/);
});

test("web InspectBlock serialization roundtrip produces correct web shape", () => {
  const inspectBlock = {
    kind: "inspect",
    stage_ids: ["stage_plan_001", "stage_impl_002", "stage_review_003"],
    filter_stage_id: "stage_plan_001",
    events: [
      {
        ts: 1710000000000,
        event_type: "stage_started",
        execution_id: "exec_plan_stage",
        stage_id: "stage_plan_001",
      },
      {
        ts: 1710000000100,
        event_type: "agent_started",
        execution_id: "exec_plan_agent",
        stage_id: "stage_plan_001",
      },
    ],
  };

  // Verify shape matches what output_block_to_web would produce
  assert.equal(inspectBlock.kind, "inspect");
  assert.equal(inspectBlock.stage_ids.length, 3);
  assert.equal(inspectBlock.filter_stage_id, "stage_plan_001");
  assert.equal(inspectBlock.events.length, 2);
  assert.equal(inspectBlock.events[0].event_type, "stage_started");
  assert.equal(inspectBlock.events[1].event_type, "agent_started");

  // Verify JSON roundtrip
  const json = JSON.stringify(inspectBlock);
  const back = JSON.parse(json);
  assert.deepEqual(back, inspectBlock);
});

test("web multi-stage live output block applies concurrent stage blocks correctly", () => {
  const { api } = createHarness();

  // Apply 3 stage blocks in rapid succession (simulating concurrent stages)
  const stages = [
    {
      kind: "scheduler_stage",
      id: "live-plan",
      stage_id: "stage_plan_001",
      profile: "Atlas",
      stage: "planning",
      title: "Atlas · Planning",
      text: "Analyzing requirements.",
      stage_index: 0,
      stage_total: 3,
      step: 1,
      status: "running",
    },
    {
      kind: "scheduler_stage",
      id: "live-impl",
      stage_id: "stage_impl_002",
      profile: "Atlas",
      stage: "implementation",
      title: "Atlas · Implementation",
      text: "Writing code.",
      stage_index: 1,
      stage_total: 3,
      step: 2,
      status: "running",
    },
    {
      kind: "scheduler_stage",
      id: "live-review",
      stage_id: "stage_review_003",
      profile: "Atlas",
      stage: "review",
      title: "Atlas · Review",
      text: "Reviewing changes.",
      stage_index: 2,
      stage_total: 3,
      step: 1,
      status: "waiting",
    },
  ];

  for (const block of stages) {
    api.applyOutputBlock(block);
  }

  const articles = api.nodes.messageFeed.children;
  assert.equal(articles.length, 3, "should have 3 stage articles");

  // Verify each stage rendered correctly
  assert.match(text(articles[0]), /Atlas · Planning/);
  assert.match(text(articles[1]), /Atlas · Implementation/);
  assert.match(text(articles[2]), /Atlas · Review/);

  // Verify status displays
  assert.match(text(articles[0]), /running/i);
  assert.match(text(articles[2]), /waiting/i);
});

test("web help slash command renders shared ui command catalogue lines", async () => {
  const { api } = createHarness();
  api.state.uiCommands = [
    {
      action_id: "open_model_list",
      title: "Switch Model",
      description: "Choose a different model",
      category: "model_agent",
      keybind: "ctrl+m",
      include_in_palette: true,
      slash: {
        name: "/models",
        aliases: ["/model"],
        suggested: true,
      },
    },
    {
      action_id: "show_help",
      title: "Help",
      description: "Show help and shortcuts",
      category: "system",
      keybind: "f1",
      include_in_palette: true,
      slash: {
        name: "/help",
        aliases: ["/commands"],
        suggested: true,
      },
    },
  ];

  const handled = await api.handleSlashCommand("/help");
  assert.equal(handled, true);

  const article = api.nodes.messageFeed.children[0];
  assert.ok(article, "help output should be appended");
  assert.match(text(article), /\/models   Choose a different model/);
  assert.match(text(article), /\/help   Show help and shortcuts/);
});

test("web shared action aliases resolve help and status without local name branches", async () => {
  const { api } = createHarness();
  api.state.uiCommands = [
    {
      action_id: "show_help",
      title: "Help",
      description: "Show help and shortcuts",
      category: "system",
      keybind: "f1",
      include_in_palette: true,
      slash: {
        name: "/help",
        aliases: ["/commands"],
        suggested: true,
      },
    },
    {
      action_id: "show_status",
      title: "Status",
      description: "Show runtime status",
      category: "system",
      keybind: null,
      include_in_palette: true,
      slash: {
        name: "/status",
        aliases: ["/stats"],
        suggested: false,
      },
    },
  ];
  api.state.selectedModel = "openai/gpt-4.1";

  let handled = await api.handleSlashCommand("/commands");
  assert.equal(handled, true);
  assert.match(api.nodes.messageFeed.textContent, /Show help and shortcuts/);

  handled = await api.handleSlashCommand("/stats");
  assert.equal(handled, true);
  assert.match(api.nodes.messageFeed.textContent, /state:/);
  assert.match(api.nodes.messageFeed.textContent, /model: openai\/gpt-4\.1/);
});

test("web slash aliases resolve through shared ui command catalogue", async () => {
  const { api } = createHarness();
  api.state.uiCommands = [
    {
      action_id: "open_theme_list",
      title: "Themes",
      description: "Open theme picker",
      category: "display",
      keybind: null,
      include_in_palette: true,
      slash: {
        name: "/theme",
        aliases: ["/themes"],
        suggested: true,
      },
    },
    {
      action_id: "open_session_list",
      title: "Switch Session",
      description: "Open session switcher",
      category: "session",
      keybind: null,
      include_in_palette: true,
      slash: {
        name: "/sessions",
        aliases: ["/session", "/resume"],
        suggested: true,
      },
    },
  ];

  let handled = await api.handleSlashCommand("/themes");
  assert.equal(handled, true);
  assert.equal(api.nodes.commandPanel.classList.contains("hidden"), false);

  api.nodes.commandPanel.classList.add("hidden");
  handled = await api.handleSlashCommand("/resume");
  assert.equal(handled, true);
  assert.equal(api.nodes.commandPanel.classList.contains("hidden"), false);
});

test("web shared parameter commands apply model and preset selections", async () => {
  const { api } = createHarness();
  api.state.uiCommands = [
    {
      action_id: "open_model_list",
      title: "Switch Model",
      description: "Choose a different model",
      category: "model_agent",
      keybind: null,
      include_in_palette: true,
      slash: {
        name: "/models",
        aliases: ["/model"],
        suggested: true,
      },
    },
    {
      action_id: "open_preset_list",
      title: "Switch Preset",
      description: "Choose a preset",
      category: "model_agent",
      keybind: null,
      include_in_palette: true,
      slash: {
        name: "/preset",
        aliases: ["/presets"],
        suggested: true,
      },
    },
  ];
  api.state.modes = [
    { key: "preset:atlas", id: "atlas", name: "atlas", kind: "preset" },
    { key: "agent:build", id: "build", name: "build", kind: "agent" },
  ];

  const option = api.nodes.modelSelect.ownerDocument.createElement("option");
  option.value = "openai/gpt-4.1";
  option.textContent = "openai/gpt-4.1";
  api.nodes.modelSelect.appendChild(option);

  let handled = await api.handleSlashCommand("/model openai/gpt-4.1");
  assert.equal(handled, true);
  assert.equal(api.state.selectedModel, "openai/gpt-4.1");

  handled = await api.handleSlashCommand("/preset atlas");
  assert.equal(handled, true);
  assert.equal(api.state.selectedModeKey, "preset:atlas");
});

test("web repairs stale selected model against current provider catalogue", () => {
  const { api, context } = createHarness();
  api.state.selectedModel = "minimax/text-01";
  api.state.providers = [
    {
      id: "openai",
      name: "OpenAI",
      models: [{ id: "gpt-4.1", name: "GPT-4.1" }],
    },
    {
      id: "anthropic",
      name: "Anthropic",
      models: [{ id: "claude-sonnet-4", name: "Claude Sonnet 4" }],
    },
  ];

  context.renderModelOptions();

  assert.equal(api.state.selectedModel, "anthropic/claude-sonnet-4");
  assert.equal(api.nodes.modelSelect.value, "anthropic/claude-sonnet-4");
});

test("web provider selector loads current configured providers from config endpoint", async () => {
  const routes = new Map([
    [
      "/config/providers",
      {
        providers: [
          {
            id: "minimax-coding-plan",
            name: "minimax-coding-plan",
            models: [{ id: "m1", name: "m1", provider: "minimax-coding-plan" }],
          },
        ],
        default: {
          "minimax-coding-plan": "m1",
        },
      },
    ],
  ]);
  const { api, context } = createHarness(routes);

  await context.loadProviders();

  assert.equal(api.state.providers.length, 1);
  assert.equal(api.state.providers[0].id, "minimax-coding-plan");
  assert.equal(api.state.selectedModel, "minimax-coding-plan/m1");
  assert.equal(api.nodes.modelSelect.value, "minimax-coding-plan/m1");
});

test("web shared copy command writes current transcript to clipboard", async () => {
  const { api, context } = createHarness();
  api.state.uiCommands = [
    {
      action_id: "copy_session",
      title: "Copy Session",
      description: "Copy session transcript",
      category: "session",
      keybind: null,
      include_in_palette: true,
      slash: {
        name: "/copy",
        aliases: [],
        suggested: false,
      },
    },
  ];
  api.state.selectedSession = "session-1";
  api.nodes.messageFeed.textContent = "copied transcript";
  let clipboardText = "";
  context.navigator.clipboard.writeText = async (value) => {
    clipboardText = String(value);
  };

  const handled = await api.handleSlashCommand("/copy");
  assert.equal(handled, true);
  assert.equal(clipboardText, "copied transcript");
});

test("web shared abort command uses shared action semantics", async () => {
  const routes = new Map([
    ["/session/session-1/abort", () => ({ target: "session", aborted: true })],
  ]);
  const { api } = createHarness(routes);
  api.state.uiCommands = [
    {
      action_id: "abort_execution",
      title: "Abort Execution",
      description: "Cancel the active run",
      category: "session",
      keybind: null,
      include_in_palette: false,
      slash: {
        name: "/abort",
        aliases: [],
        suggested: false,
      },
    },
  ];

  let handled = await api.handleSlashCommand("/abort");
  assert.equal(handled, true);
  assert.match(api.nodes.messageFeed.textContent, /No active run to abort/);

  api.state.streaming = true;
  api.state.selectedSession = "session-1";
  handled = await api.handleSlashCommand("/abort");
  assert.equal(handled, true);
  assert.equal(api.state.abortRequested, true);
});

test("web command panel renders shared command catalogue", () => {
  const { api } = createHarness();
  api.state.uiCommands = [
    {
      action_id: "toggle_command_palette",
      title: "Command Palette",
      description: "Open command palette",
      category: "navigation",
      keybind: "ctrl+p",
      include_in_palette: false,
      slash: {
        name: "/command",
        aliases: ["/cmd", "/palette"],
        suggested: true,
      },
    },
  ];

  api.openCommandPanel("model");

  const entry = api.nodes.commandCatalog.children[0];
  assert.ok(entry, "command catalog entry should render");
  assert.match(text(entry), /\/command/);
  assert.match(text(entry), /Command Palette/);
  assert.match(text(entry), /Navigation/);
});
