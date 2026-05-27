# Changelog

All notable changes to this project will be documented in this file.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/),
and this project does not yet follow semantic versioning (pre-1.0).

## Unreleased

## 0.2.0 — 2026-05-27

### 中文

#### 新增

- 增加 MiniMax、Kimi/Moonshot、阿里云百炼/DashScope、火山方舟 provider 的一等支持。
- Tauri 控制台的 Models 表单补齐国内平台默认 `base_url` 和常用模型预设。
- README 与 Tauri app README 增加国内 OpenAI-compatible 平台配置示例。

#### 优化

- OpenAI-compatible 请求转换现在会按 provider/model 控制 `thinking` 字段：
  DeepSeek 使用 `{"type":"enabled"}`，`kimi-*` 使用
  `{"type":"enabled","keep":"all"}`，普通 `moonshot-v1-*` 不再收到未知
  `thinking` 字段。
- MiniMax `reasoning_details[]` 会转换为 Codex `reasoning` 输出项，流式和非流式都保留推理摘要。
- Tauri Rust 写入校验与前端 provider 列表保持一致，避免 UI 能选但后端拒绝写入。

#### 验证

- `python3.11 -m compileall codex_shim -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3.11 -m pytest -p pytest_asyncio.plugin tests/ -q`
- `npm run build`
- `git diff --check`

### English

#### Added

- First-class provider support for MiniMax, Kimi/Moonshot, Alibaba Cloud
  Bailian/DashScope, and Volcengine Ark.
- Default base URLs and popular model presets for these providers in the Tauri
  control panel.
- Provider configuration examples in both the root README and Tauri app README.

#### Changed

- OpenAI-compatible request translation now gates `thinking` by provider/model:
  DeepSeek receives `{"type":"enabled"}`, `kimi-*` receives
  `{"type":"enabled","keep":"all"}`, and plain `moonshot-v1-*` models no longer
  receive unsupported `thinking` payloads.
- MiniMax `reasoning_details[]` is normalized into Codex `reasoning` output
  items for both streaming and non-streaming paths.
- Tauri Rust validation now matches the frontend provider list.

#### Verified

- `python3.11 -m compileall codex_shim -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3.11 -m pytest -p pytest_asyncio.plugin tests/ -q`
- `npm run build`
- `git diff --check`

### Added

- GitHub Actions CI (`.github/workflows/ci.yml`) running pytest and
  `compileall` on Python 3.11 and 3.12.
- `[project.optional-dependencies] dev` in `pyproject.toml` so
  `pip install -e ".[dev]"` pulls `pytest` and `pytest-asyncio` in one step.
- `CONTRIBUTING.md` documenting the dev loop, what kinds of PRs are useful,
  and what to include in bug reports.
- `.github/ISSUE_TEMPLATE/` with structured bug and feature request templates.
- `CHANGELOG.md` (this file).

### Changed

- Reframed the project around a generic all-model Codex shim instead of any
  single upstream app or model store.
- Made `~/.codex-shim/models.json` the canonical default settings file.
- Renamed the generated Codex provider to `codex_shim` / "Codex Shim".
- Settings now prefer a generic top-level `models` array with snake_case keys,
  while still accepting `customModels` and camelCase aliases for existing
  exports.

## 2026-05-25 — Auth-gated ChatGPT passthrough + docs hardening

### Added

- `settings.chatgpt_passthrough_available()` checks `~/.codex/auth.json` for a
  usable `tokens.access_token`. The synthetic `gpt-5.5` slug is now only
  advertised in `/health`, `/v1/models`, `codex-shim list`, and the generated
  `custom_model_catalog.json` while that token is present.
- `_load_models()` in the CLI wraps model settings loading with actionable
  errors for missing files and invalid JSON.
- `_entrypoint()` in the CLI catches `BrokenPipeError` at the boundary so
  piping `codex-shim list` into `head`/`grep` exits cleanly instead of dumping
  a traceback.
- Regression tests covering auth-gating, CLI error UX, settings aliases, and
  catalog generation.

### Changed

- `/health` payload now includes `chatgpt_passthrough: bool` and reports the
  real model count instead of always-plus-one.
- `cli._resolve_model_slug("gpt-5.5", ...)` raises `SystemExit` telling the
  user to run `codex login` when auth.json is missing, instead of returning a
  slug that would 401 on first request.
- `default_model_slug` picks the first configured BYOK model when passthrough
  is not usable, instead of unconditionally returning `gpt-5.5`.
- README install section recommends `pip install -e .` as the primary path.
- README benchmarking section: replaced an unsupported "7x fewer input tokens
  / 5–10x faster" claim with honest anecdata and a note that no reproducible
  benchmark script ships with the repo yet.

### Fixed

- Codex Desktop picker / `/v1/models` no longer offers `gpt-5.5` when there's
  no Codex login, removing the misleading "select it to get a 401" footgun.

## 2026-05-25 — Initial public hardening

### Added

- Public-grade README rewrite covering install, ChatGPT passthrough, tool
  calls, computer use, prompt catching/proxy patterns, benchmarking, security,
  limitations, troubleshooting, and contributing.
- `pyproject.toml` build-system, `readme`, `license`, `authors`, `keywords`,
  classifiers, and project URLs.
