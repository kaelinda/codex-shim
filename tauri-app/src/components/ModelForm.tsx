import { useMemo } from "react";
import { PROVIDERS } from "../types";
import type { ModelRow, Provider } from "../types";

interface Props {
  value: ModelRow;
  onChange: (next: ModelRow) => void;
}

const BASE_URL_DEFAULTS: Record<Provider, string> = {
  openai: "https://api.openai.com/v1",
  anthropic: "https://api.anthropic.com/v1",
  "generic-chat-completion-api": "",
  deepseek: "https://api.deepseek.com",
};

// Popular model presets per provider
const MODEL_PRESETS: Record<Provider, { model: string; display: string }[]> = {
  openai: [
    { model: "gpt-4o", display: "GPT-4o" },
    { model: "gpt-4o-mini", display: "GPT-4o mini" },
    { model: "o3", display: "o3" },
    { model: "o4-mini", display: "o4-mini" },
  ],
  anthropic: [
    { model: "claude-sonnet-4-20250514", display: "Claude Sonnet 4" },
    { model: "claude-opus-4-7-20251109", display: "Claude Opus 4.7" },
    { model: "claude-3-5-sonnet-20241022", display: "Claude 3.5 Sonnet" },
    { model: "claude-3-5-haiku-20241022", display: "Claude 3.5 Haiku" },
  ],
  deepseek: [
    { model: "deepseek-chat", display: "DeepSeek Chat" },
    { model: "deepseek-v4-pro", display: "DeepSeek V4 Pro" },
    { model: "deepseek-coder", display: "DeepSeek Coder" },
  ],
  "generic-chat-completion-api": [],
};

export default function ModelForm({ value, onChange }: Props) {
  const headersText = useMemo(() => {
    if (!value.extra_headers) return "";
    try {
      return JSON.stringify(value.extra_headers, null, 2);
    } catch {
      return "";
    }
  }, [value.extra_headers]);

  const presets = MODEL_PRESETS[value.provider as Provider] || [];

  const patch = (delta: Partial<ModelRow>) => onChange({ ...value, ...delta });

  return (
    <div className="form-grid">
      <Field label="display_name" hint="picker 上显示的名字，可留空（默认用 model 名）">
        <input
          type="text"
          value={value.display_name ?? ""}
          onChange={(e) => patch({ display_name: e.target.value || null })}
        />
      </Field>

      <Field label="model *">
        <div style={{ display: "flex", gap: "8px" }}>
          <input
            type="text"
            value={value.model}
            onChange={(e) => patch({ model: e.target.value })}
            required
            style={{ flex: 1 }}
          />
          {presets.length > 0 && (
            <select
              onChange={(e) => {
                const preset = presets.find((p) => p.model === e.target.value);
                if (preset) {
                  patch({ model: preset.model, display_name: preset.display });
                }
                e.target.value = "";
              }}
              style={{ width: "auto" }}
            >
              <option value="">选择预设…</option>
              {presets.map((p) => (
                <option key={p.model} value={p.model}>{p.display}</option>
              ))}
            </select>
          )}
        </div>
      </Field>

      <Field label="provider *">
        <select
          value={value.provider}
          onChange={(e) => {
            const next = e.target.value as Provider;
            patch({
              provider: next,
              base_url: value.base_url || BASE_URL_DEFAULTS[next] || value.base_url,
            });
          }}
        >
          {PROVIDERS.map((p) => (
            <option key={p} value={p}>{p}</option>
          ))}
        </select>
      </Field>

      <Field label="base_url *" hint="不要带尾部 /chat/completions 之类">
        <input
          type="text"
          value={value.base_url}
          onChange={(e) => patch({ base_url: e.target.value })}
          placeholder={BASE_URL_DEFAULTS[value.provider as Provider] || "https://..."}
          required
        />
      </Field>

      <Field label="api_key" hint="保存在本地 models.json，不会上传到 catalog">
        <input
          type="password"
          autoComplete="off"
          value={value.api_key ?? ""}
          onChange={(e) => patch({ api_key: e.target.value })}
        />
      </Field>

      <Field label="max_context_limit">
        <input
          type="number"
          value={value.max_context_limit ?? ""}
          onChange={(e) =>
            patch({
              max_context_limit: e.target.value === "" ? null : Number(e.target.value),
            })
          }
        />
      </Field>

      <Field label="max_output_tokens">
        <input
          type="number"
          value={value.max_output_tokens ?? ""}
          onChange={(e) =>
            patch({
              max_output_tokens: e.target.value === "" ? null : Number(e.target.value),
            })
          }
        />
      </Field>

      <Field label="text-only model">
        <label className="toggle">
          <input
            type="checkbox"
            checked={!!value.no_image_support}
            onChange={(e) => patch({ no_image_support: e.target.checked })}
          />
          关闭图像输入（no_image_support）
        </label>
      </Field>

      <Field label="extra_headers (JSON)" hint='可选；例: {"Anthropic-Beta": "..."}' span={2}>
        <textarea
          rows={4}
          spellCheck={false}
          value={headersText}
          onChange={(e) => {
            const text = e.target.value.trim();
            if (!text) {
              patch({ extra_headers: null });
              return;
            }
            try {
              const parsed = JSON.parse(text);
              if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
                patch({ extra_headers: parsed });
              }
            } catch {
              // ignore until valid JSON; user keeps typing
            }
          }}
        />
      </Field>
    </div>
  );
}

function Field({
  label,
  hint,
  span,
  children,
}: {
  label: string;
  hint?: string;
  span?: number;
  children: React.ReactNode;
}) {
  return (
    <div className={`form-field ${span === 2 ? "form-field-wide" : ""}`}>
      <label className="form-label">{label}</label>
      {children}
      {hint && <div className="form-hint">{hint}</div>}
    </div>
  );
}
