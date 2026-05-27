import { useId, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../api";
import { describeCli } from "../api";
import type { AppSettingsDto, RuntimeInfo, UpdateInfo } from "../types";
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
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [updateOutput, setUpdateOutput] = useState<string | null>(null);
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

  const checkUpdate = async () => {
    setUpdateBusy(true);
    setUpdateOutput(null);
    try {
      const info = await api.checkUpdate();
      setUpdateInfo(info);
      flash("ok", info.update_available ? "发现新版本" : "当前已是最新版本");
    } catch (e) {
      flash("err", `检查更新失败: ${String(e)}`);
    } finally {
      setUpdateBusy(false);
    }
  };

  const installCliUpdate = async () => {
    const refName = updateInfo?.install_ref;
    setUpdateBusy(true);
    setUpdateOutput(null);
    try {
      const output = await api.installCliUpdate(refName);
      setUpdateOutput(describeCli(output));
      flash(output.ok ? "ok" : "err", output.ok ? "CLI 更新完成" : "CLI 更新失败");
    } catch (e) {
      flash("err", `CLI 更新失败: ${String(e)}`);
    } finally {
      setUpdateBusy(false);
    }
  };

  const appAssets = updateInfo?.assets.filter((asset) =>
    /\.(dmg|msi|exe|app\.tar\.gz|appimage|deb|rpm|zip)$/i.test(asset.name),
  ) ?? [];

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
        <KV k="app version" v={runtime.app_version} />
        <KV k="platform" v={runtime.platform} />
        <KV k="home" v={runtime.home_dir} />
        <KV k="default models.json" v={runtime.default_settings_path} />
        <KV k="codex auth.json" v={runtime.codex_auth_path} />
        <KV k="codex config.toml" v={runtime.codex_config_path} />
        <KV k="log path" v={runtime.log_path} />
        <KV k="default port" v={String(runtime.default_port)} />
      </Section>

      <Section title="版本更新">
        <KV k="当前版本" v={updateInfo?.current_version ?? runtime.app_version} />
        <KV
          k="最新版本"
          v={
            updateInfo
              ? updateInfo.latest_version
                ? `${updateInfo.latest_version} (${updateInfo.latest_tag})`
                : "未获取"
              : "尚未检查"
          }
        />
        {updateInfo && (
          <>
            <KV k="仓库" v={updateInfo.repo} />
            <KV k="发布页" v={updateInfo.release_url} />
            <div className={`update-banner ${updateInfo.update_available ? "update-banner-new" : ""}`}>
              {updateInfo.update_available ? "发现新版本" : "当前已是最新版本"}
            </div>
            {appAssets.length > 0 && (
              <div className="asset-list">
                {appAssets.map((asset) => (
                  <button
                    key={asset.download_url}
                    type="button"
                    onClick={() => openUrl(asset.download_url)}
                    disabled={updateBusy}
                  >
                    <Icon name="down" />{asset.name}
                  </button>
                ))}
              </div>
            )}
            <pre className="code-block update-command">{updateInfo.install_command}</pre>
          </>
        )}
        <div className="btn-row">
          <button type="button" onClick={checkUpdate} disabled={updateBusy}>
            <Icon name="refresh" />检查更新
          </button>
          <button
            type="button"
            onClick={() => openUrl(updateInfo?.release_url ?? "https://github.com/kaelinda/codex-shim/releases")}
            disabled={updateBusy}
          >
            <Icon name="launch" />更新 App
          </button>
          <button
            type="button"
            className="primary"
            onClick={installCliUpdate}
            disabled={updateBusy}
          >
            <Icon name="update" />更新 CLI
          </button>
        </div>
        <span className="hint">
          App 更新会打开 GitHub Releases 下载页；CLI 更新会执行 start.sh 重新安装 codex-shim-cli。
        </span>
        {updateOutput && <pre className="code-block">{updateOutput}</pre>}
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
