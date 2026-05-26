import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import Icon from "./Icon";
import { useAdaptivePolling } from "../hooks/useAdaptivePolling";

interface Props {
  logPath: string;
}

const SIZE_OPTIONS = [
  { label: "8 KB", value: 8 * 1024 },
  { label: "32 KB", value: 32 * 1024 },
  { label: "128 KB", value: 128 * 1024 },
  { label: "512 KB", value: 512 * 1024 },
];

export default function LogViewer({ logPath }: Props) {
  const [text, setText] = useState("");
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [maxBytes, setMaxBytes] = useState(32 * 1024);
  const [err, setErr] = useState<string | null>(null);
  const preRef = useRef<HTMLPreElement | null>(null);

  const refresh = useCallback(async () => {
    try {
      const next = await api.tailLog(maxBytes);
      setText(next);
      setErr(null);
    } catch (e) {
      setErr(String(e));
    }
  }, [maxBytes]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useAdaptivePolling(refresh, { enabled: autoRefresh, intervalMs: 2000 });

  useEffect(() => {
    if (preRef.current) {
      preRef.current.scrollTop = preRef.current.scrollHeight;
    }
  }, [text]);

  return (
    <div className="logs">
      <div className="toolbar">
        <span className="hint" title={logPath}><Icon name="file" /> {logPath}</span>
        <button type="button" onClick={refresh}><Icon name="refresh" />刷新</button>
        <label className="toggle">
          <input
            type="checkbox"
            checked={autoRefresh}
            onChange={(e) => setAutoRefresh(e.target.checked)}
          />
          每 2s 自动刷新
        </label>
        <label className="toggle">
          tail 大小
          <select
            value={maxBytes}
            onChange={(e) => setMaxBytes(Number(e.target.value))}
          >
            {SIZE_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>{opt.label}</option>
            ))}
          </select>
        </label>
      </div>
      {err && <div className="error-card error-card-inline">读取失败: {err}</div>}
      <pre ref={preRef} className="log-pre">{text || "(empty)"}</pre>
    </div>
  );
}
