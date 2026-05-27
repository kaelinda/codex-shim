import { useId, useMemo } from "react";
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
  minimax: "https://api.minimax.io/v1",
  moonshot: "https://api.moonshot.cn/v1",
  dashscope: "https://dashscope.aliyuncs.com/compatible-mode/v1",
  volcengine: "https://ark.cn-beijing.volces.com/api/v3",
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
    { model: "deepseek-v4-pro", display: "DeepSeek V4 Pro" },
    { model: "deepseek-v4-flash", display: "DeepSeek V4 Flash" },
    { model: "deepseek-chat", display: "DeepSeek Chat (legacy)" },
    { model: "deepseek-reasoner", display: "DeepSeek Reasoner (legacy)" },
  ],
  minimax: [
    { model: "MiniMax-M2", display: "MiniMax M2" },
    { model: "MiniMax-M2.7", display: "MiniMax M2.7" },
  ],
  moonshot: [
    { model: "kimi-k2.6", display: "Kimi K2.6" },
    { model: "kimi-k2.5", display: "Kimi K2.5" },
    { model: "kimi-k2-thinking", display: "Kimi K2 Thinking" },
    { model: "kimi-k2-thinking-turbo", display: "Kimi K2 Thinking Turbo" },
    { model: "kimi-k2-0905-preview", display: "Kimi K2 0905 Preview" },
    { model: "kimi-k2-0711-preview", display: "Kimi K2 0711 Preview" },
    { model: "moonshot-v1-8k", display: "Moonshot v1 8K" },
    { model: "moonshot-v1-32k", display: "Moonshot v1 32K" },
    { model: "moonshot-v1-128k", display: "Moonshot v1 128K" },
  ],
  dashscope: [
    { model: "qwen3.6-plus", display: "Qwen3.6 Plus" },
    { model: "qwen-plus", display: "Qwen Plus" },
    { model: "deepseek-v4-pro", display: "DeepSeek V4 Pro on Bailian" },
    { model: "qwen3-coder-plus", display: "Qwen3 Coder Plus" },
  ],
  volcengine: [
    { model: "doubao-seed-1-6", display: "Doubao Seed 1.6" },
    { model: "deepseek-v3", display: "DeepSeek V3 on Ark" },
    { model: "deepseek-r1", display: "DeepSeek R1 on Ark" },
  ],
  "generic-chat-completion-api": [],
};

export default function ModelForm({ value, onChange }: Props) {
  const idPrefix = useId().replace(/:/g, "");
  const ids = {
    displayName: `${idPrefix}-display-name`,
    model: `${idPrefix}-model`,
    modelPreset: `${idPrefix}-model-preset`,
    provider: `${idPrefix}-provider`,
    baseUrl: `${idPrefix}-base-url`,
    apiKey: `${idPrefix}-api-key`,
    maxContext: `${idPrefix}-max-context`,
    maxOutput: `${idPrefix}-max-output`,
    noImage: `${idPrefix}-no-image`,
    extraHeaders: `${idPrefix}-extra-headers`,
  };

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
      <Field id={ids.displayName} label="display_name" hint="picker 上显示的名字，可留空（默认用 model 名）">
        <input
          id={ids.displayName}
          type="text"
          value={value.display_name ?? ""}
          onChange={(e) => patch({ display_name: e.target.value || null })}
          aria-describedby={`${ids.displayName}-hint`}
        />
      </Field>

      <Field id={ids.model} label="model *">
        <div className="inline-control">
          <input
            id={ids.model}
            type="text"
            value={value.model}
            onChange={(e) => patch({ model: e.target.value })}
            required
            style={{ flex: 1 }}
          />
          {presets.length > 0 && (
            <select
              id={ids.modelPreset}
              aria-label="选择模型预设"
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

      <Field id={ids.provider} label="provider *">
        <select
          id={ids.provider}
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

      <Field id={ids.baseUrl} label="base_url *" hint="不要带尾部 /chat/completions 之类">
        <input
          id={ids.baseUrl}
          type="text"
          value={value.base_url}
          onChange={(e) => patch({ base_url: e.target.value })}
          placeholder={BASE_URL_DEFAULTS[value.provider as Provider] || "https://..."}
          required
          aria-describedby={`${ids.baseUrl}-hint`}
        />
      </Field>

      <Field id={ids.apiKey} label="api_key" hint="保存在本地 models.json，不会上传到 catalog">
        <input
          id={ids.apiKey}
          type="password"
          autoComplete="off"
          value={value.api_key ?? ""}
          onChange={(e) => patch({ api_key: e.target.value })}
          aria-describedby={`${ids.apiKey}-hint`}
        />
      </Field>

      <Field id={ids.maxContext} label="max_context_limit">
        <input
          id={ids.maxContext}
          type="number"
          value={value.max_context_limit ?? ""}
          onChange={(e) =>
            patch({
              max_context_limit: e.target.value === "" ? null : Number(e.target.value),
            })
          }
        />
      </Field>

      <Field id={ids.maxOutput} label="max_output_tokens">
        <input
          id={ids.maxOutput}
          type="number"
          value={value.max_output_tokens ?? ""}
          onChange={(e) =>
            patch({
              max_output_tokens: e.target.value === "" ? null : Number(e.target.value),
            })
          }
        />
      </Field>

      <Field id={ids.noImage} label="text-only model">
        <label className="toggle">
          <input
            id={ids.noImage}
            type="checkbox"
            checked={!!value.no_image_support}
            onChange={(e) => patch({ no_image_support: e.target.checked })}
          />
          关闭图像输入（no_image_support）
        </label>
      </Field>

      <Field id={ids.extraHeaders} label="extra_headers (JSON)" hint='可选；例: {"Anthropic-Beta": "..."}' span={2}>
        <textarea
          id={ids.extraHeaders}
          rows={4}
          spellCheck={false}
          value={headersText}
          aria-describedby={`${ids.extraHeaders}-hint`}
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
  id,
  label,
  hint,
  span,
  children,
}: {
  id: string;
  label: string;
  hint?: string;
  span?: number;
  children: React.ReactNode;
}) {
  return (
    <div className={`form-field ${span === 2 ? "form-field-wide" : ""}`}>
      <label className="form-label" htmlFor={id}>{label}</label>
      {children}
      {hint && <div className="form-hint" id={`${id}-hint`}>{hint}</div>}
    </div>
  );
}
