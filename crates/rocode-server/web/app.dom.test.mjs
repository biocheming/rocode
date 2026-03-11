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
  "message-render.js",
  "scheduler-stage.js",
  "question-panel.js",
  "output-blocks.js",
  "sidebar.js",
  "settings.js",
  "session-actions.js",
  "commands.js",
  "streaming.js",
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
