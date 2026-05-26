import { useState } from "react";
import { api } from "../api";
import Icon from "./Icon";
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
  const [showRawHealth, setShowRawHealth] = useState(false);

  const wrap = (label: string, fn: () => Promise<unknown>) => async () => {
    setBusy(label);
    try {
      await onAction(label, fn);
    } finally {
      setBusy(null);
    }
  };

  const daemonRunning = health?.ok === true;

  return (
    <div className="dashboard">
      {/* Status overview */}
      <div className="card-grid">
        <StatusCard
          title="Daemon"
          statusLabel={daemonRunning ? "运行中" : "已停止"}
          tone={daemonRunning ? "ok" : "bad"}
        >
          <KV k="端口" v={String(settings.port)} />
          <KV k="模型数" v={health?.models?.toString() ?? "—"} />
          {health?.error && <KV k="错误" v={health.error} />}
        </StatusCard>

        <StatusCard
          title="Codex 登录"
          statusLabel={auth?.passthrough_available ? "已登录" : "未登录"}
          tone={auth?.passthrough_available ? "ok" : "warn"}
        >
          <KV k="Passthrough" v={auth?.passthrough_available ? "可用" : "不可用"} />
          <KV k="auth.json" v={auth?.exists ? "存在" : "缺失"} />
          {auth?.email && <KV k="邮箱" v={auth.email} />}
          {auth?.plan && <KV k="订阅" v={auth.plan} />}
        </StatusCard>

        <StatusCard
          title="当前模型"
          statusLabel={activeModel ?? "未设置"}
          tone={activeModel ? "ok" : "warn"}
        >
          <KV k="配置文件" v={runtime.codex_config_path} />
        </StatusCard>

        <StatusCard
          title="项目"
          statusLabel={runtime.detected_project_root ? "已检测" : "未检测到"}
          tone={runtime.detected_project_root ? "ok" : "warn"}
        >
          <KV k="项目根目录" v={runtime.detected_project_root ?? "—"} />
          <KV k="日志路径" v={runtime.log_path} />
        </StatusCard>
      </div>

      {/* Daemon controls */}
      <Card title="Daemon 控制">
        <div className="control-groups">
          <div className="control-group">
            <span className="control-group-label">生命周期</span>
            <div className="btn-row">
              <button
                className="primary"
                type="button"
                onClick={wrap("Start", api.start)}
                disabled={!!busy || daemonRunning}
              >
                <Icon name="play" />启动
              </button>
              <button
                type="button"
                onClick={wrap("Stop", api.stop)}
                disabled={!!busy || !daemonRunning}
              >
                <Icon name="stop" />停止
              </button>
              <button
                type="button"
                onClick={wrap("Restart", api.restart)}
                disabled={!!busy}
              >
                <Icon name="refresh" />重启
              </button>
            </div>
          </div>
          <div className="control-group">
            <span className="control-group-label">配置管理</span>
            <div className="btn-row">
              <button
                type="button"
                onClick={wrap("Generate catalog", api.generate)}
                disabled={!!busy}
              >
                <Icon name="refresh" />生成
              </button>
              <button
                type="button"
                onClick={wrap("Enable codex config", api.enable)}
                disabled={!!busy}
              >
                <Icon name="check" />启用
              </button>
              <button
                type="button"
                onClick={wrap("Disable codex config", api.disable)}
                disabled={!!busy}
              >
                <Icon name="close" />禁用
              </button>
            </div>
          </div>
        </div>
        {busy && (
          <div className="busy-hint" role="status" aria-live="polite">
            正在执行: {busy}…
          </div>
        )}
      </Card>

      {/* Codex Desktop */}
      <Card title="Codex Desktop">
        <div className="launch-row">
          <label className="row-label" htmlFor="dashboard-launch-path">项目路径</label>
          <input
            id="dashboard-launch-path"
            type="text"
            value={launchPath}
            onChange={(e) => setLaunchPath(e.target.value)}
            placeholder="输入项目路径，默认当前目录"
          />
          <button
            className="primary"
            type="button"
            onClick={wrap(`launch app (${launchPath})`, () =>
              api.launchApp(launchPath),
            )}
            disabled={!!busy}
          >
            <Icon name="launch" />启动 Codex
          </button>
        </div>
        {runtime.platform === "macos" && (
          <div className="patch-section">
            <span className="control-group-label">Picker 补丁 (macOS)</span>
            <div className="btn-row">
              <button
                className="danger"
                type="button"
                onClick={wrap("Patch macOS picker", api.patchApp)}
                disabled={!!busy}
              >
                <Icon name="patch" />应用
              </button>
              <button
                type="button"
                onClick={wrap("Restore picker", api.restoreApp)}
                disabled={!!busy}
              >
                <Icon name="restore" />恢复原始
              </button>
            </div>
            <span className="hint">
              需要 npx、osascript、codesign，会修改 /Applications/Codex.app
            </span>
          </div>
        )}
      </Card>

      {/* Health check */}
      <Card title="健康检查">
        {health ? (
          <>
            <div className="health-summary">
              <span className={`health-dot ${daemonRunning ? "health-dot-ok" : "health-dot-bad"}`} />
              <span className="health-url">{health.url ?? `http://127.0.0.1:${settings.port}/health`}</span>
              <span className="health-meta">
                {health.models ?? 0} 个模型
              </span>
            </div>
            <div className="btn-row btn-row-tight">
              <button
                type="button"
                onClick={() => setShowRawHealth((p) => !p)}
              >
                {showRawHealth ? "收起原始数据" : "查看原始数据"}
              </button>
              <button
                type="button"
                onClick={async () => {
                  try {
                    const list = await api.listModels();
                    flash(
                      list.ok ? "ok" : "err",
                      list.stdout || list.stderr || "(empty)",
                    );
                  } catch (e) {
                    flash("err", String(e));
                  }
                }}
              >
                列出全部 Slug
              </button>
            </div>
            {showRawHealth && (
              <pre className="code-block">
                {JSON.stringify(health, null, 2)}
              </pre>
            )}
          </>
        ) : (
          <div className="empty">无法获取健康检查信息</div>
        )}
      </Card>
    </div>
  );
}

function StatusCard({
  title,
  statusLabel,
  tone,
  children,
}: {
  title: string;
  statusLabel: string;
  tone: "ok" | "warn" | "bad";
  children: React.ReactNode;
}) {
  return (
    <div className={`status-card status-card-${tone}`}>
      <div className="status-card-header">
        <span className="status-card-title">{title}</span>
        <span className={`status-badge status-badge-${tone}`}>
          <span className="status-dot" />
          {statusLabel}
        </span>
      </div>
      <div className="status-card-body">{children}</div>
    </div>
  );
}

function Card({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="card">
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
