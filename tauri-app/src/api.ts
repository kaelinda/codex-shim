import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettingsDto,
  AuthSnapshot,
  CliOutput,
  HealthSnapshot,
  ModelsFile,
  RuntimeInfo,
  ShimStatus,
} from "./types";

export const api = {
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

export function describeCli(cli: CliOutput): string {
  const head = `$ ${cli.command} ${cli.args.join(" ")}`;
  const exit = cli.status === null ? "exit: -" : `exit: ${cli.status}`;
  const stdout = cli.stdout.trim();
  const stderr = cli.stderr.trim();
  return [head, exit, stdout && `stdout:\n${stdout}`, stderr && `stderr:\n${stderr}`]
    .filter(Boolean)
    .join("\n");
}
