// ── API Types ──────────────────────────────────────────────────────────────

export interface Session {
  /** 会话唯一 ID（后端主键）。 */
  id: string;
  /** 会话标题（可为默认自动标题或用户自定义标题）。 */
  title: string;
  /** 所属项目 ID（部分接口可能不返回）。 */
  project_id?: string;
  /** 会话工作目录（部分接口可能不返回）。 */
  directory?: string;
  /** 最近更新时间（Unix 毫秒时间戳，部分接口可能不返回）。 */
  updated?: number;
  /** 分享链接（仅会话已分享时返回）。 */
  share_url?: string;
  /** 父会话 ID（根会话通常为空）。 */
  parent_id?: string;
}

export interface Provider {
  id: string;
  name: string;
  base?: string;
  base_url?: string;
  models?: Model[];
  [key: string]: unknown;
}

export interface Model {
  id: string;
  name: string;
  provider_id: string;
  family?: string;
  reasoning?: boolean;
  tool_call?: boolean;
  [key: string]: unknown;
}

export interface ExecutionMode {
  id: string;
  name: string;
  kind: string;
  description?: string;
  mode?: string;
  hidden?: boolean;
  color?: string;
  orchestrator?: string;
}

export interface UiCommand {
  id: string;
  name: string;
  description?: string;
  aliases?: string[];
  argumentKind?: string;
}

export interface OutputBlock {
  kind: string;
  phase?: string;
  role?: "user" | "assistant" | "system";
  title?: string;
  text?: string;
  tone?: string;
  silent?: boolean;
  id?: string;
  name?: string;
  status?: string;
  summary?: string;
  fields?: Record<string, unknown>[];
  preview?: string;
  body?: string;
  [key: string]: unknown;
}

export interface OutputBlockEvent {
  sessionID?: string;
  sessionId?: string;
  id?: string;
  block?: OutputBlock;
  [key: string]: unknown;
}

export interface UsageEvent {
  sessionID?: string;
  sessionId?: string;
  prompt_tokens?: number;
  completion_tokens?: number;
  promptTokens?: number;
  completionTokens?: number;
}

export interface QuestionInteraction {
  request_id: string;
  session_id?: string;
  questions: QuestionItem[];
}

export interface QuestionItem {
  question: string;
  options?: QuestionOption[];
  multi_select?: boolean;
}

export interface QuestionOption {
  label: string;
  value: string;
}

export interface PermissionInteraction {
  permission_id: string;
  session_id?: string;
  message?: string;
  permission?: string;
  patterns?: string[];
  command?: string;
  filepath?: string;
}

export interface ExecutionTopology {
  nodes: ExecutionNode[];
  active_count: number;
  running_count: number;
  waiting_count: number;
  cancelling_count: number;
}

export interface ExecutionNode {
  id: string;
  kind: string;
  status: string;
  label?: string;
  children?: ExecutionNode[];
}

export interface RecoveryProtocol {
  actions: RecoveryAction[];
  checkpoints: RecoveryCheckpoint[];
}

export interface RecoveryAction {
  kind: string;
  label: string;
  description?: string;
  target_id?: string;
}

export interface RecoveryCheckpoint {
  kind: string;
  label: string;
  status: string;
  summary?: string;
}

export interface TerminalSession {
  id: string;
  command?: string;
  cwd?: string;
}

export interface HealthResponse {
  status: string;
  version: string;
}

export interface ConfigResponse {
  [key: string]: unknown;
}

export interface StageEvent {
  time: string;
  event_type: string;
  execution_id?: string;
  stage_id?: string;
  [key: string]: unknown;
}
