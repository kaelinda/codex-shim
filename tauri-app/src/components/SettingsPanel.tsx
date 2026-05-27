import { useId, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import type { AppSettingsDto, RuntimeInfo } from "../types";
import Icon from "./Icon";

interface Props {
  runtime: RuntimeInfo;
  settings: AppSettingsDto;
  onUpdated: (next: AppSettingsDto) => Promise<void> | void;
  flash: (kind: "ok" | "err", text: string) => void;
}

export default function SettingsPanel({ runtime, settings, onUpdated, flash }: Props) {
  const idPrefix = useId().replace(/:/g, "");
  const [draft, setDraft] = useState<AppSettingsDto>(settings);
  const [busy, setBusy] = useState(false);
  const ids = {
    settingsPath: `${idPrefix}-settings-path`,
    port: `${idPrefix}-port`,
  };

  const save = async () => {
    setBusy(true);
    try {
      const next = await api.updateAppSettings({
        settings_path: draft.settings_path,
        port: draft.port,
      });
      setDraft(next);
      await onUpdated(next);
      flash("ok", "已更新偏好");
    } catch (e) {
      flash("err", `保存失败: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const pickFile = async (key: "settings_path") => {
    const picked = await open({ multiple: false, directory: false });
    if (typeof picked === "string") {
      setDraft({ ...draft, [key]: picked });
    }
  };

  return (
    <div className="settings-panel">
      <Section title="路径与端口">
        <Row label="models.json" htmlFor={ids.settingsPath}>
          <input
            id={ids.settingsPath}
            type="text"
            value={draft.settings_path}
            onChange={(e) => setDraft({ ...draft, settings_path: e.target.value })}
          />
          <button type="button" onClick={() => pickFile("settings_path")}><Icon name="file" />选择</button>
        </Row>
        <Row label="port" htmlFor={ids.port}>
          <input
            id={ids.port}
            type="number"
            min={1}
            max={65535}
            value={draft.port}
            onChange={(e) => setDraft({ ...draft, port: Number(e.target.value) || 0 })}
          />
        </Row>
        <div className="btn-row">
          <button type="button" className="primary" onClick={save} disabled={busy}>
            <Icon name="save" />保存
          </button>
          <button
            type="button"
            onClick={() => setDraft(settings)}
            disabled={busy}
          >
            <Icon name="reset" />重置
          </button>
        </div>
      </Section>

      <Section title="运行环境">
        <KV k="platform" v={runtime.platform} />
        <KV k="home" v={runtime.home_dir} />
        <KV k="default models.json" v={runtime.default_settings_path} />
        <KV k="codex auth.json" v={runtime.codex_auth_path} />
        <KV k="codex config.toml" v={runtime.codex_config_path} />
        <KV k="log path" v={runtime.log_path} />
        <KV k="default port" v={String(runtime.default_port)} />
      </Section>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="card card-neutral">
      <div className="card-title">{title}</div>
      <div className="card-body">{children}</div>
    </div>
  );
}

function Row({
  label,
  htmlFor,
  children,
}: {
  label: string;
  htmlFor: string;
  children: React.ReactNode;
}) {
  return (
    <div className="row">
      <label className="row-label" htmlFor={htmlFor}>{label}</label>
      {children}
    </div>
  );
}

function KV({ k, v }: { k: string; v: string }) {
  return (
    <div className="kv">
      <span className="kv-k">{k}</span>
      <span className="kv-v" title={v}>{v}</span>
    </div>
  );
}
