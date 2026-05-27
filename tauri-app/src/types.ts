export interface ModelRow {
  model: string;
  provider: string;
  base_url: string;
  api_key?: string;
  display_name?: string | null;
  max_context_limit?: number | null;
  max_output_tokens?: number | null;
  no_image_support?: boolean;
  extra_headers?: Record<string, unknown> | null;
  // serde flatten dumps unknown keys into the same object — we keep them for round-trip.
  [extra: string]: unknown;
}

export interface ModelsFile {
  models: ModelRow[];
  [extra: string]: unknown;
}

export interface CliOutput {
  command: string;
  args: string[];
  status: number | null;
  stdout: string;
  stderr: string;
  ok: boolean;
}

export interface HealthSnapshot {
  ok: boolean;
  url: string;
  status: number | null;
  models: number | null;
  raw: unknown;
  error: string | null;
}

export interface ShimStatus {
  cli: CliOutput;
  health: HealthSnapshot;
}

export interface RuntimeInfo {
  home_dir: string;
  default_settings_path: string;
  codex_auth_path: string;
  codex_config_path: string;
  detected_project_root: string | null;
  log_path: string;
  default_port: number;
  platform: string;
}

export interface AppSettingsDto {
  settings_path: string;
  port: number;
  cli_override: string | null;
  project_root_override: string | null;
}

export interface AuthSnapshot {
  auth_path: string;
  exists: boolean;
  passthrough_available: boolean;
  account_id: string | null;
  email: string | null;
  plan: string | null;
}

export type TabKey = "dashboard" | "models" | "active" | "logs" | "settings";

export const PROVIDERS = [
  "openai",
  "anthropic",
  "generic-chat-completion-api",
  "deepseek",
  "mimo",
  "minimax",
  "moonshot",
  "dashscope",
  "volcengine",
] as const;
export type Provider = (typeof PROVIDERS)[number];
