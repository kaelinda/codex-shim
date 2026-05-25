import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "@tauri-apps/api/core";
import type {
  AppSettingsDto,
  AuthSnapshot,
  CliOutput,
  HealthSnapshot,
  ModelsFile,
  RuntimeInfo,
  ShimStatus,
} from "./types";

// Use mock API in browser for debugging
const useMock = !isTauri();

export const api = useMock ? buildMockApi() : {
  runtimeInfo: () => invoke<RuntimeInfo>("get_runtime_info"),
  appSettings: () => invoke<AppSettingsDto>("get_app_settings"),
  updateAppSettings: (payload: {
    settings_path?: string;
    port?: number;
    cli_override?: string | null;
    project_root_override?: string | null;
  }) => invoke<AppSettingsDto>("update_app_settings", payload),
  status: () => invoke<ShimStatus>("shim_status"),
  health: () => invoke<HealthSnapshot>("shim_health"),
  start: () => invoke<CliOutput>("shim_start"),
  stop: () => invoke<CliOutput>("shim_stop"),
  restart: () => invoke<CliOutput>("shim_restart"),
  generate: () => invoke<CliOutput>("shim_generate"),
  enable: () => invoke<CliOutput>("shim_enable"),
  disable: () => invoke<CliOutput>("shim_disable"),
  listModels: () => invoke<CliOutput>("shim_list_models"),
  useModel: (slug: string) => invoke<CliOutput>("shim_use_model", { slug }),
  launchApp: (path?: string) => invoke<CliOutput>("shim_launch_app", { path }),
  patchApp: () => invoke<CliOutput>("shim_patch_app"),
  restoreApp: () => invoke<CliOutput>("shim_restore_app"),
  readModels: () => invoke<ModelsFile>("read_models_file"),
  writeModels: (file: ModelsFile) =>
    invoke<ModelsFile>("write_models_file", { file }),
  tailLog: (maxBytes?: number) =>
    invoke<string>("tail_log", { maxBytes: maxBytes ?? null }),
  readCodexAuth: () => invoke<AuthSnapshot>("read_codex_auth"),
  currentActiveModel: () => invoke<string | null>("current_active_model"),
};

function buildMockApi() {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const mock: Record<string, (...args: any[]) => any> = {};
  const real = {
    runtimeInfo: () => invoke<RuntimeInfo>("get_runtime_info"),
    appSettings: () => invoke<AppSettingsDto>("get_app_settings"),
    updateAppSettings: (payload: {
      settings_path?: string;
      port?: number;
      cli_override?: string | null;
      project_root_override?: string | null;
    }) => invoke<AppSettingsDto>("update_app_settings", payload),
    status: () => invoke<ShimStatus>("shim_status"),
    health: () => invoke<HealthSnapshot>("shim_health"),
    start: () => invoke<CliOutput>("shim_start"),
    stop: () => invoke<CliOutput>("shim_stop"),
    restart: () => invoke<CliOutput>("shim_restart"),
    generate: () => invoke<CliOutput>("shim_generate"),
    enable: () => invoke<CliOutput>("shim_enable"),
    disable: () => invoke<CliOutput>("shim_disable"),
    listModels: () => invoke<CliOutput>("shim_list_models"),
    useModel: (slug: string) => invoke<CliOutput>("shim_use_model", { slug }),
    launchApp: (path?: string) => invoke<CliOutput>("shim_launch_app", { path }),
    patchApp: () => invoke<CliOutput>("shim_patch_app"),
    restoreApp: () => invoke<CliOutput>("shim_restore_app"),
    readModels: () => invoke<ModelsFile>("read_models_file"),
    writeModels: (file: ModelsFile) =>
      invoke<ModelsFile>("write_models_file", { file }),
    tailLog: (maxBytes?: number) =>
      invoke<string>("tail_log", { maxBytes: maxBytes ?? null }),
    readCodexAuth: () => invoke<AuthSnapshot>("read_codex_auth"),
    currentActiveModel: () => invoke<string | null>("current_active_model"),
  };
  for (const key of Object.keys(real)) {
    mock[key] = (...args: unknown[]) => {
      console.warn(`[MockAPI] ${key} called — Tauri not available, returning mock data`);
      return mockData(key, args);
    };
  }
  return mock;
}

function mockData(key: string, args: unknown[]): unknown {
  switch (key) {
    case "runtimeInfo":
      return {
        home_dir: "/home/user",
        default_settings_path: "~/.codex-shim/models.json",
        codex_auth_path: "~/.codex/auth.json",
        codex_config_path: "~/.codex/config.toml",
        detected_project_root: null,
        log_path: ".codex-shim/shim.log",
        default_port: 8765,
        platform: "macos",
      };
    case "appSettings":
      return { settings_path: "~/.codex-shim/models.json", port: 8765, cli_override: null, project_root_override: null };
    case "updateAppSettings":
      return { settings_path: "~/.codex-shim/models.json", port: 8765, cli_override: null, project_root_override: null };
    case "shimStatus":
      return {
        cli: { command: "codex-shim", args: ["status"], status: 0, stdout: "Shim is stopped.", stderr: "", ok: true },
        health: { ok: false, url: "http://127.0.0.1:8765", status: null, models: null, raw: null, error: null },
      };
    case "shimHealth":
      return { ok: false, url: "http://127.0.0.1:8765", status: null, models: null, raw: null, error: null };
    case "shimListModels":
      return { command: "codex-shim", args: ["list"], status: 1, stdout: "", stderr: "No models available.", ok: false };
    case "readCodexAuth":
      return { auth_path: "~/.codex/auth.json", exists: false, passthrough_available: false, account_id: null, email: null, plan: null };
    case "currentActiveModel":
      return null;
    case "readModelsFile":
      return { models: [{ model: "MiniMax-M2.7", provider: "openai", base_url: "https://api.minimax.chat", display_name: "MiniMax-M2.7", api_key: "" }] };
    case "tailLog":
      return "[req] /v1/responses model=\"MiniMax-M2.7\" stream=true tools=0 [] input=1 ([message])\n";
    default:
      return { command: key, args, status: 0, stdout: "mock ok", stderr: "", ok: true };
  }
}

export function describeCli(cli: CliOutput): string {
  const head = `$ ${cli.command} ${cli.args.join(" ")}`;
  const exit = cli.status === null ? "exit: -" : `exit: ${cli.status}`;
  const stdout = cli.stdout.trim();
  const stderr = cli.stderr.trim();
  return [head, exit, stdout && `stdout:\n${stdout}`, stderr && `stderr:\n${stderr}`]
    .filter(Boolean)
    .join("\n");
}
