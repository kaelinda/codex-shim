import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type { ModelRow, ModelsFile } from "../types";
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
  const [file, setFile] = useState<ModelsFile | null>(null);
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [draft, setDraft] = useState<ModelRow | null>(null);
  const [rawMode, setRawMode] = useState(false);
  const [rawText, setRawText] = useState<string>("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setBusy(true);
    try {
      const next = await api.readModels();
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
      setDraft({ ...EMPTY_ROW, ...file.models[idx] });
    }
    setEditingIndex(idx);
  };

  const cancelEdit = () => {
    setDraft(null);
    setEditingIndex(null);
  };

  const persist = async (next: ModelsFile) => {
    setBusy(true);
    try {
      const saved = await api.writeModels(next);
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
    const next: ModelsFile = { ...file, models: [...file.models] };
    if (editingIndex === null) {
      next.models.push(draft);
    } else {
      next.models[editingIndex] = draft;
    }
    await persist(next);
  };

  const removeRow = async (idx: number) => {
    if (!file) return;
    if (!window.confirm(`删除第 ${idx + 1} 行 (${file.models[idx].model})？`)) return;
    const next: ModelsFile = { ...file, models: file.models.filter((_, i) => i !== idx) };
    await persist(next);
  };

  const moveRow = async (idx: number, dir: -1 | 1) => {
    if (!file) return;
    const j = idx + dir;
    if (j < 0 || j >= file.models.length) return;
    const next: ModelsFile = { ...file, models: [...file.models] };
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

  const empty = useMemo(() => file && file.models.length === 0, [file]);

  return (
    <div className="models">
      <div className="toolbar">
        <button type="button" onClick={() => startEdit(null)} disabled={busy || rawMode}>
          ＋ 新增模型
        </button>
        <button type="button" onClick={() => load()} disabled={busy}>
          ↻ 重新读取
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

      {!file && <div className="spinner">读取 models.json…</div>}

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
            {file.models.map((row, idx) => (
              <tr key={`${row.model}-${idx}`}>
                <td>{idx + 1}</td>
                <td>{row.display_name || <em className="muted">{row.model}</em>}</td>
                <td><code>{row.model}</code></td>
                <td>{row.provider}</td>
                <td className="truncate" title={row.base_url}>{row.base_url}</td>
                <td>{row.api_key ? "•••" : <em className="muted">空</em>}</td>
                <td className="row-actions">
                  <button type="button" onClick={() => moveRow(idx, -1)} disabled={busy || idx === 0}>↑</button>
                  <button type="button" onClick={() => moveRow(idx, 1)} disabled={busy || idx === file.models.length - 1}>↓</button>
                  <button type="button" onClick={() => startEdit(idx)} disabled={busy}>编辑</button>
                  <button type="button" className="danger" onClick={() => removeRow(idx)} disabled={busy}>删除</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {file && rawMode && (
        <div className="raw-edit">
          <textarea
            value={rawText}
            onChange={(e) => setRawText(e.target.value)}
            spellCheck={false}
            rows={24}
          />
          <div className="btn-row">
            <button type="button" onClick={saveRaw} disabled={busy}>保存 JSON</button>
            <button type="button" onClick={() => setRawText(JSON.stringify(file, null, 2))} disabled={busy}>
              重置
            </button>
          </div>
        </div>
      )}

      {draft && (
        <div className="modal-backdrop" onClick={cancelEdit}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-title">
              {editingIndex === null ? "新增模型" : `编辑第 ${editingIndex + 1} 行`}
            </div>
            <ModelForm value={draft} onChange={setDraft} />
            <div className="btn-row modal-actions">
              <button type="button" onClick={cancelEdit} disabled={busy}>取消</button>
              <button type="button" className="primary" onClick={saveDraft} disabled={busy}>
                保存
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
