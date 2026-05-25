import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type { AuthSnapshot, HealthSnapshot } from "../types";

interface Props {
  health: HealthSnapshot | null;
  auth: AuthSnapshot | null;
  activeModel: string | null;
  onUseModel: (slug: string) => Promise<void>;
  flash: (kind: "ok" | "err", text: string) => void;
}

interface SlugRow {
  slug: string;
  display: string;
  upstream: string;
  provider: string;
}

export default function ActiveModel({ activeModel, auth, health, onUseModel, flash }: Props) {
  const [rows, setRows] = useState<SlugRow[]>([]);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    setBusy(true);
    try {
      const cli = await api.listModels();
      if (!cli.ok) {
        flash("err", cli.stderr || cli.stdout || "list 失败");
        setRows([]);
        return;
      }
      setRows(parseListing(cli.stdout));
    } catch (e) {
      flash("err", String(e));
    } finally {
      setBusy(false);
    }
  }, [flash]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const hint = useMemo(() => {
    if (!health?.ok) return "daemon 未启动，先去 Dashboard 启动。";
    if (rows.length === 0) return "没有可用模型；检查 ~/.codex-shim/models.json 或登录 codex。";
    return null;
  }, [health, rows]);

  return (
    <div className="active-model">
      <div className="card card-neutral">
        <div className="card-title">当前 Codex Desktop 默认 model</div>
        <div className="card-body">
          <div className="big-slug">{activeModel ?? "未设置"}</div>
          <div className="hint">
            点击下方任意一行即可写入 <code>~/.codex/config.toml</code> 的 managed block。
          </div>
        </div>
      </div>

      <div className="toolbar">
        <button type="button" onClick={refresh} disabled={busy}>↻ 重新拉取</button>
        {!auth?.passthrough_available && (
          <span className="hint hint-warn">
            未检测到 ChatGPT 登录态，gpt-5.5 passthrough 不可用。
          </span>
        )}
      </div>

      {hint && <div className="empty">{hint}</div>}

      <table className="models-table">
        <thead>
          <tr>
            <th>slug</th>
            <th>display</th>
            <th>upstream model</th>
            <th>provider</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.slug} className={row.slug === activeModel ? "row-active" : ""}>
              <td><code>{row.slug}</code></td>
              <td>{row.display}</td>
              <td>{row.upstream}</td>
              <td>{row.provider}</td>
              <td>
                <button
                  type="button"
                  className="primary"
                  onClick={() => onUseModel(row.slug)}
                  disabled={busy || row.slug === activeModel}
                >
                  {row.slug === activeModel ? "当前" : "设为默认"}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/**
 * `codex-shim list` prints `<slug>  <display>  ->  <model> (<provider>)`.
 * Reuse that format for the picker instead of re-implementing slug resolution
 * here — keeps GUI/CLI behavior in lockstep.
 */
function parseListing(stdout: string): SlugRow[] {
  return stdout
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line): SlugRow | null => {
      const arrowIdx = line.indexOf("->");
      if (arrowIdx === -1) return null;
      const left = line.slice(0, arrowIdx).trim();
      const right = line.slice(arrowIdx + 2).trim();
      const tokens = left.split(/\s{2,}/);
      const slug = tokens[0] ?? left;
      const display = tokens.slice(1).join("  ") || slug;
      const match = right.match(/^(.+?)\s*\(([^)]+)\)$/);
      const row: SlugRow = {
        slug,
        display,
        upstream: match ? match[1] : right,
        provider: match ? match[2] : "—",
      };
      return row;
    })
    .filter((row): row is SlugRow => row !== null);
}
