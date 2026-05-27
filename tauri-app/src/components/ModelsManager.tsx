import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import type { ModelRow, ModelsFile } from "../types";
import Icon from "./Icon";
import ModelForm from "./ModelForm";

interface Props {
  flash: (kind: "ok" | "err", text: string) => void;
}

const EMPTY_ROW: ModelRow = {
  model: "",
  provider: "openai",
  base_url: "",
  api_key: "",
  display_name: null,
  max_context_limit: null,
  max_output_tokens: null,
  no_image_support: false,
  extra_headers: null,
};

export default function ModelsManager({ flash }: Props) {
  const dialogTitleId = `${useId().replace(/:/g, "")}-model-dialog-title`;
  const [file, setFile] = useState<ModelsFile | null>(null);
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [draft, setDraft] = useState<ModelRow | null>(null);
  const [rawMode, setRawMode] = useState(false);
  const [rawText, setRawText] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const modalRef = useRef<HTMLDivElement | null>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const busyRef = useRef(busy);
  const dialogOpen = draft !== null;
  const models = Array.isArray(file?.models) ? file.models : [];

  const load = useCallback(async () => {
    setBusy(true);
    try {
      const next = normalizeModelsFile(await api.readModels());
      setFile(next);
      setRawText(JSON.stringify(next, null, 2));
    } catch (e) {
      flash("err", `读取 models.json 失败: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  }, [flash]);

  useEffect(() => {
    load();
  }, [load]);

  const startEdit = (idx: number | null) => {
    if (idx === null) {
      setDraft({ ...EMPTY_ROW });
    } else if (file) {
      setDraft({ ...EMPTY_ROW, ...models[idx] });
    }
    setEditingIndex(idx);
  };

  const cancelEdit = useCallback(() => {
    setDraft(null);
    setEditingIndex(null);
  }, []);

  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);

  useEffect(() => {
    if (!dialogOpen) return;
    previousFocusRef.current = document.activeElement as HTMLElement | null;

    const focusableSelector = [
      "button:not([disabled])",
      "input:not([disabled])",
      "select:not([disabled])",
      "textarea:not([disabled])",
      "[href]",
      "[tabindex]:not([tabindex='-1'])",
    ].join(",");

    const focusFirst = () => {
      const first = modalRef.current?.querySelector<HTMLElement>(focusableSelector);
      (first ?? modalRef.current)?.focus();
    };

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (!busyRef.current) cancelEdit();
        return;
      }
      if (event.key !== "Tab") return;

      const focusable = Array.from(
        modalRef.current?.querySelectorAll<HTMLElement>(focusableSelector) ?? [],
      ).filter((el) => el.offsetParent !== null || el === document.activeElement);
      if (focusable.length === 0) {
        event.preventDefault();
        modalRef.current?.focus();
        return;
      }

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    window.setTimeout(focusFirst, 0);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      previousFocusRef.current?.focus?.();
    };
  }, [cancelEdit, dialogOpen]);

  const persist = async (next: ModelsFile) => {
    setBusy(true);
    try {
      const saved = normalizeModelsFile(await api.writeModels(next));
      setFile(saved);
      setRawText(JSON.stringify(saved, null, 2));
      flash("ok", "已写入 models.json");
      cancelEdit();
    } catch (e) {
      flash("err", `写入失败: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const saveDraft = async () => {
    if (!draft || !file) return;
    const next: ModelsFile = { ...file, models: [...models] };
    if (editingIndex === null) {
      next.models.push(draft);
    } else {
      next.models[editingIndex] = draft;
    }
    await persist(next);
  };

  const removeRow = async (idx: number) => {
    if (!file) return;
    const row = models[idx];
    if (!row) return;
    if (!window.confirm(`删除第 ${idx + 1} 行 (${row.model})？`)) return;
    const next: ModelsFile = { ...file, models: models.filter((_, i) => i !== idx) };
    await persist(next);
  };

  const moveRow = async (idx: number, dir: -1 | 1) => {
    if (!file) return;
    const j = idx + dir;
    if (j < 0 || j >= models.length) return;
    const next: ModelsFile = { ...file, models: [...models] };
    [next.models[idx], next.models[j]] = [next.models[j], next.models[idx]];
    await persist(next);
  };

  const saveRaw = async () => {
    try {
      const parsed = JSON.parse(rawText);
      if (!parsed || typeof parsed !== "object" || !Array.isArray(parsed.models)) {
        throw new Error("根对象必须包含 models: [] 数组");
      }
      await persist(parsed as ModelsFile);
    } catch (e) {
      flash("err", `JSON 无效: ${String(e)}`);
    }
  };

  const exportConfig = async (withoutKeys: boolean) => {
    const picked = await save({
      defaultPath: withoutKeys ? "codex-shim-models.redacted.json" : "codex-shim-models.json",
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!picked) return;
    setBusy(true);
    try {
      const result = await api.exportModels(picked, withoutKeys);
      flash(
        "ok",
        withoutKeys
          ? `已导出脱敏配置：${result.model_count} 个模型`
          : `已导出配置：${result.model_count} 个模型，文件包含 API Key`,
      );
    } catch (e) {
      flash("err", `导出失败: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const importConfig = async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (typeof picked !== "string") return;
    if (!window.confirm("导入会覆盖当前 models.json，并自动生成备份。继续？")) return;
    setBusy(true);
    try {
      const result = await api.importModels(picked);
      await load();
      flash(
        "ok",
        result.backup_path
          ? `已导入 ${result.model_count} 个模型，已备份当前配置`
          : `已导入 ${result.model_count} 个模型`,
      );
    } catch (e) {
      flash("err", `导入失败: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const empty = useMemo(() => file !== null && models.length === 0, [file, models.length]);

  return (
    <div className="models">
      <div className="toolbar">
        <button type="button" onClick={() => startEdit(null)} disabled={busy || rawMode}>
          <Icon name="add" />新增模型
        </button>
        <button type="button" onClick={() => load()} disabled={busy}>
          <Icon name="refresh" />重新读取
        </button>
        <button type="button" onClick={() => exportConfig(false)} disabled={busy || !file}>
          <Icon name="export" />导出配置
        </button>
        <button type="button" onClick={() => exportConfig(true)} disabled={busy || !file}>
          <Icon name="export" />导出脱敏
        </button>
        <button type="button" onClick={importConfig} disabled={busy}>
          <Icon name="import" />导入配置
        </button>
        <label className="toggle">
          <input
            type="checkbox"
            checked={rawMode}
            onChange={(e) => setRawMode(e.target.checked)}
          />
          直接编辑 JSON
        </label>
      </div>

      {!file && <div className="spinner" role="status">读取 models.json…</div>}

      {file && !rawMode && (
        <table className="models-table">
          <thead>
            <tr>
              <th>#</th>
              <th>display_name</th>
              <th>model</th>
              <th>provider</th>
              <th>base_url</th>
              <th>api_key</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {empty && (
              <tr>
                <td colSpan={7} className="empty">
                  当前 models.json 没有任何模型，点击「新增模型」开始添加。
                </td>
              </tr>
            )}
            {models.map((row, idx) => (
              <tr key={`${row.model}-${idx}`}>
                <td>{idx + 1}</td>
                <td>{row.display_name || <em className="muted">{row.model}</em>}</td>
                <td><code>{row.model}</code></td>
                <td>{row.provider}</td>
                <td className="truncate" title={row.base_url}>{row.base_url}</td>
                <td>{row.api_key ? "•••" : <em className="muted">空</em>}</td>
                <td className="row-actions">
                  <button type="button" className="icon-button" onClick={() => moveRow(idx, -1)} disabled={busy || idx === 0} aria-label={`上移第 ${idx + 1} 行`}><Icon name="up" /></button>
                  <button type="button" className="icon-button" onClick={() => moveRow(idx, 1)} disabled={busy || idx === models.length - 1} aria-label={`下移第 ${idx + 1} 行`}><Icon name="down" /></button>
                  <button type="button" onClick={() => startEdit(idx)} disabled={busy}><Icon name="edit" />编辑</button>
                  <button type="button" className="danger" onClick={() => removeRow(idx)} disabled={busy}><Icon name="trash" />删除</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {file && rawMode && (
        <div className="raw-edit">
          <textarea
            aria-label="直接编辑 models.json JSON"
            value={rawText}
            onChange={(e) => setRawText(e.target.value)}
            spellCheck={false}
            rows={24}
          />
          <div className="btn-row">
            <button type="button" onClick={saveRaw} disabled={busy}><Icon name="save" />保存 JSON</button>
            <button type="button" onClick={() => setRawText(JSON.stringify(file, null, 2))} disabled={busy}>
              <Icon name="reset" />重置
            </button>
          </div>
        </div>
      )}

      {draft && (
        <div className="modal-backdrop" onClick={cancelEdit}>
          <div
            ref={modalRef}
            className="modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby={dialogTitleId}
            tabIndex={-1}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="modal-title" id={dialogTitleId}>
              {editingIndex === null ? "新增模型" : `编辑第 ${editingIndex + 1} 行`}
            </div>
            <ModelForm value={draft} onChange={setDraft} />
            <div className="btn-row modal-actions">
              <button type="button" onClick={cancelEdit} disabled={busy}>取消</button>
              <button type="button" className="primary" onClick={saveDraft} disabled={busy}>
                <Icon name="save" />保存
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function normalizeModelsFile(value: unknown): ModelsFile {
  if (!value || typeof value !== "object") return { models: [] };
  const maybe = value as Partial<ModelsFile>;
  if (!Array.isArray(maybe.models)) return { ...maybe, models: [] } as ModelsFile;
  return maybe as ModelsFile;
}
