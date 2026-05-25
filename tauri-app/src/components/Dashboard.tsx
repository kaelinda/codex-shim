import { useState } from "react";
import { api } from "../api";
import type {
  AppSettingsDto,
  AuthSnapshot,
  HealthSnapshot,
  RuntimeInfo,
} from "../types";

interface Props {
  runtime: RuntimeInfo;
  settings: AppSettingsDto;
  health: HealthSnapshot | null;
  auth: AuthSnapshot | null;
  activeModel: string | null;
  onAction: (label: string, action: () => Promise<unknown>) => Promise<void>;
  flash: (kind: "ok" | "err", text: string) => void;
}

export default function Dashboard({
  runtime,
  settings,
  health,
  auth,
  activeModel,
  onAction,
  flash,
}: Props) {
  const [busy, setBusy] = useState<string | null>(null);
  const [launchPath, setLaunchPath] = useState<string>(".");

  const wrap = (label: string, fn: () => Promise<unknown>) => async () => {
    setBusy(label);
    try {
      await onAction(label, fn);
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="dashboard">
      <div className="card-grid">
        <Card title="Daemon" tone={health?.ok ? "ok" : "bad"}>
          <KV k="status" v={health?.ok ? "running" : "stopped"} />
          <KV k="url" v={health?.url ?? `http://127.0.0.1:${settings.port}/health`} />
          <KV k="models" v={health?.models?.toString() ?? "—"} />
          {health?.error && <KV k="error" v={health.error} />}
        </Card>

        <Card title="Codex login" tone={auth?.passthrough_available ? "ok" : "warn"}>
          <KV k="passthrough" v={auth?.passthrough_available ? "available" : "未登录"} />
          <KV k="auth.json" v={auth?.exists ? "存在" : "缺失"} />
          {auth?.email && <KV k="email" v={auth.email} />}
          {auth?.plan && <KV k="plan" v={auth.plan} />}
        </Card>

        <Card title="Active model" tone={activeModel ? "ok" : "warn"}>
          <KV k="model" v={activeModel ?? "未设置"} />
          <KV k="config" v={runtime.codex_config_path} />
        </Card>

        <Card title="Project" tone={runtime.detected_project_root ? "ok" : "warn"}>
          <KV k="root" v={runtime.detected_project_root ?? "未检测到"} />
          <KV k="log" v={runtime.log_path} />
        </Card>
      </div>

      <Card title="Daemon 控制">
        <div className="btn-row">
          <button type="button" onClick={wrap("Start", api.start)} disabled={!!busy}>
            ▶ Start
          </button>
          <button type="button" onClick={wrap("Stop", api.stop)} disabled={!!busy}>
            ◼ Stop
          </button>
          <button type="button" onClick={wrap("Restart", api.restart)} disabled={!!busy}>
            ↻ Restart
          </button>
          <button type="button" onClick={wrap("Generate catalog", api.generate)} disabled={!!busy}>
            ⟳ Generate
          </button>
          <button type="button" onClick={wrap("Enable codex config", api.enable)} disabled={!!busy}>
            ✓ Enable
          </button>
          <button type="button" onClick={wrap("Disable codex config", api.disable)} disabled={!!busy}>
            ✗ Disable
          </button>
        </div>
        {busy && <div className="busy-hint">正在执行: {busy}…</div>}
      </Card>

      <Card title="Codex Desktop">
        <div className="row">
          <label className="row-label">project path</label>
          <input
            type="text"
            value={launchPath}
            onChange={(e) => setLaunchPath(e.target.value)}
            placeholder="."
          />
          <button
            type="button"
            onClick={wrap(`launch app (${launchPath})`, () => api.launchApp(launchPath))}
            disabled={!!busy}
          >
            🚀 Launch
          </button>
        </div>
        {runtime.platform === "macos" && (
          <div className="btn-row btn-row-tight">
            <button type="button" onClick={wrap("Patch macOS picker", api.patchApp)} disabled={!!busy}>
              ⚠ Patch picker
            </button>
            <button type="button" onClick={wrap("Restore picker", api.restoreApp)} disabled={!!busy}>
              ↩ Restore picker
            </button>
            <span className="hint">
              仅 macOS。需要 npx、osascript、codesign，并会修改 /Applications/Codex.app。
            </span>
          </div>
        )}
        {runtime.platform !== "macos" && (
          <div className="hint">picker patch 仅适用于 macOS。</div>
        )}
      </Card>

      <Card title="健康检查">
        <pre className="code-block">
          {JSON.stringify(health ?? {}, null, 2)}
        </pre>
        <div className="btn-row btn-row-tight">
          <button
            type="button"
            onClick={async () => {
              try {
                const list = await api.listModels();
                flash(list.ok ? "ok" : "err", list.stdout || list.stderr || "(empty)");
              } catch (e) {
                flash("err", String(e));
              }
            }}
          >
            列出全部 slug
          </button>
        </div>
      </Card>
    </div>
  );
}

function Card({
  title,
  tone,
  children,
}: {
  title: string;
  tone?: "ok" | "warn" | "bad";
  children: React.ReactNode;
}) {
  return (
    <div className={`card card-${tone ?? "neutral"}`}>
      <div className="card-title">{title}</div>
      <div className="card-body">{children}</div>
    </div>
  );
}

function KV({ k, v }: { k: string; v: string }) {
  return (
    <div className="kv">
      <span className="kv-k">{k}</span>
      <span className="kv-v" title={v}>
        {v}
      </span>
    </div>
  );
}
