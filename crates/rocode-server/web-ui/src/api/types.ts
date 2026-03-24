// ── API Types ──────────────────────────────────────────────────────────────

export interface Session {
  id: string;
  title: string;
  project_id?: string;
  directory?: string;
  updated?: number;
  share_url?: string;
  parent_id?: string;
}

export interface Provider {
  id: string;
  name: string;
  base_url?: string;
  models?: Model[];
  [key: string]: unknown;
}

export interface KnownProviderEntry {
  id: string;
  name: string;
  env: string[];
  model_count: number;
  connected: boolean;
}

export interface ConnectProtocolOption {
  id: string;
  name: string;
}

export interface ProviderConnectSchemaResponse {
  providers: KnownProviderEntry[];
  protocols: ConnectProtocolOption[];
}

export interface ConnectProviderRequest {
  provider_id: string;
  api_key: string;
  base_url?: string;
  protocol?: string;
}

export interface ManagedProvider {
  id: string;
  name: string;
  status: string;
  connected: boolean;
  has_auth: boolean;
  auth_type?: string;
  configured: boolean;
  known: boolean;
  env: string[];
  known_model_count: number;
  base_url?: string;
  protocol?: string;
  model_overrides: ManagedModelOverride[];
  models: Model[];
}

export interface ManagedModelOverride {
  key: string;
  name?: string;
  model?: string;
  base_url?: string;
  family?: string;
  reasoning?: boolean;
  tool_call?: boolean;
  headers?: Record<string, string>;
  options?: Record<string, unknown>;
  variants?: Record<string, unknown>;
  modalities?: Record<string, unknown>;
  interleaved?: unknown;
  cost?: Record<string, unknown>;
  limit?: Record<string, unknown>;
  attachment?: boolean;
  temperature?: boolean;
  status?: string;
  release_date?: string;
  experimental?: boolean;
}

export interface ManagedProvidersResponse {
  providers: ManagedProvider[];
}

export interface UpdateProviderRequest {
  name?: string;
  base_url?: string;
  protocol?: string;
}

export interface UpdateProviderModelRequest {
  name?: string;
  model?: string;
  base_url?: string;
  family?: string;
  reasoning?: boolean;
  tool_call?: boolean;
  headers?: Record<string, string>;
  options?: Record<string, unknown>;
  variants?: Record<string, unknown>;
  modalities?: Record<string, unknown>;
  interleaved?: unknown;
  cost?: Record<string, unknown>;
  limit?: Record<string, unknown>;
  attachment?: boolean;
  temperature?: boolean;
  status?: string;
  release_date?: string;
  experimental?: boolean;
}

export interface ProviderAuthMethod {
  name: string;
  description: string;
}

export interface ProviderAuthMethodsResponse {
  [providerId: string]: ProviderAuthMethod[];
}

export interface OAuthAuthorizeResponse {
  url: string;
  method: string;
  instructions: string;
}

export interface Model {
  id: string;
  name: string;
  provider_id: string;
  provider?: string;
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

export type PromptPart =
  | {
      type: "text";
      text: string;
    }
  | {
      type: "file";
      url: string;
      filename?: string;
      mime?: string;
    }
  | {
      type: "agent";
      name: string;
    }
  | {
      type: "subtask";
      prompt: string;
      description?: string;
      agent: string;
    };

export interface OutputBlock {
  kind: string;
  phase?: string;
  role?: string;
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
