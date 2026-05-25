import type { AppSettingsDto, RuntimeInfo } from "../types";

interface Props {
  runtime: RuntimeInfo | null;
  settings: AppSettingsDto | null;
}

export default function StatusBar({ runtime, settings }: Props) {
  if (!runtime || !settings) {
    return <footer className="statusbar">…</footer>;
  }
  return (
    <footer className="statusbar">
      <span>os: {runtime.platform}</span>
      <span>·</span>
      <span>port: {settings.port}</span>
      <span>·</span>
      <span title={settings.settings_path}>
        models.json: {shortenPath(settings.settings_path)}
      </span>
      <span>·</span>
      <span title={runtime.detected_project_root ?? "未检测到"}>
        project: {runtime.detected_project_root ? shortenPath(runtime.detected_project_root) : "—"}
      </span>
    </footer>
  );
}

function shortenPath(p: string): string {
  if (p.length <= 50) return p;
  const head = p.slice(0, 20);
  const tail = p.slice(-26);
  return `${head}…${tail}`;
}
