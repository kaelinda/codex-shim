# Changelog

All notable changes to this project will be documented in this file.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/),
and this project does not yet follow semantic versioning (pre-1.0).

## Unreleased

### Added

- Allow custom OpenAI-compatible provider names such as `new-api` in both the
  Python shim and Tauri app. Any provider except `anthropic` now routes through
  `/chat/completions`.

## 0.4.0 — 2026-05-27

### 中文

#### 新增

- 新增轻量 Rust CLI 安装器与 `start.sh` 懒人安装流程，可从远程下载脚本、
  构建 `codex-shim-cli`、安装到 `~/.local/bin`，并在缺少配置时交互式引导填写
  provider、模型和 API Key。
- `codex-shim-cli` 新增 `test <name>`，可按 provider、slug、上游模型名或显示名
  测试已配置 provider。
- `codex-shim-cli` 新增 `export` / `import` 和 `config export` /
  `config import`，支持跨设备共享 `models.json`，导入前会自动备份当前配置。
- Tauri Models 页新增配置导入、导出和脱敏导出按钮。
- CLI 与 Tauri Settings 页新增版本更新能力：检查 GitHub Releases、展示 App
  下载资产，并可调用 `start.sh` 更新 `codex-shim-cli`。

#### 修复

- Rust CLI 与内嵌 shim 的 slug 生成现在保留模型名中的 `.`，例如
  `minimax-m2.7` 不再变成 `minimax-m2-7`。
- 自定义 OpenAI-compatible provider 会在 `codex-shim-cli list` 和 Tauri 模型列表中
  保留并显示原始 provider 名称。

#### 验证

- `python3.11 -m compileall codex_shim -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3.11 -m pytest -p pytest_asyncio.plugin tests/ -q`
- `cargo test --manifest-path cli/Cargo.toml`
- `cargo check --manifest-path tauri-app/src-tauri/Cargo.toml`
- `npm run build`
- `git diff --check`

### English

#### Added

- Added the lightweight Rust CLI installer and `start.sh` quick-install flow.
  It can download the script remotely, build `codex-shim-cli`, install it into
  `~/.local/bin`, and guide first-time provider/model/API-key setup
  interactively.
- Added `codex-shim-cli test <name>` for checking a configured provider, slug,
  upstream model name, or display name.
- Added `codex-shim-cli export` / `import` plus `config export` /
  `config import` for sharing `models.json` across devices, with automatic
  backup before import.
- Added import, export, and redacted export actions to the Tauri Models page.
- Added version-update support to both the CLI and Tauri Settings page:
  GitHub Releases checks, App download assets, and `start.sh`-based CLI update.

#### Fixed

- Rust CLI and embedded shim slug generation now preserves `.` in model names,
  so `minimax-m2.7` no longer becomes `minimax-m2-7`.
- Custom OpenAI-compatible providers are preserved and displayed by
  `codex-shim-cli list` and the Tauri model list.

#### Verified

- `python3.11 -m compileall codex_shim -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3.11 -m pytest -p pytest_asyncio.plugin tests/ -q`
- `cargo test --manifest-path cli/Cargo.toml`
- `cargo check --manifest-path tauri-app/src-tauri/Cargo.toml`
- `npm run build`
- `git diff --check`

## 0.3.0 — 2026-05-27

### 中文

#### 新增

- 增加小米 MiMo provider 支持，默认 `base_url` 为
  `https://token-plan-cn.xiaomimimo.com/v1`，并内置 `mimo-v2.5-pro` /
  `mimo-v2.5` 模型预设。
- Tauri 控制台现在内置 Rust shim 服务，可直接提供 `/health`、`/v1/models`、
  `/v1/responses` 和 `/v1/chat/completions`。

#### 优化

- Tauri app 的 Start / Stop / Restart / Generate / Enable / Disable /
  Active model / Codex launch 流程改为 Rust 原生实现，不再依赖 Python 项目或
  `codex-shim` CLI。
- 内置 Rust shim 覆盖 OpenAI-compatible、Anthropic Messages 和 ChatGPT
  passthrough 主路径，支持流式 Responses 转换、tool call、reasoning/thinking
  片段和 MiniMax `reasoning_details[]`。
- macOS picker patch / restore 改为 Tauri Rust 命令执行，仍使用本机 `npx asar`
  和 `codesign`。
- README 与 Tauri app README 补充独立桌面应用、MiMo 和内置服务说明。

#### 验证

- `cargo check --offline`
- `cargo test --offline`
- `npm run build`
- `npm run tauri:build`
- `git diff --check`

### English

#### Added

- Xiaomi MiMo provider support with the default base URL
  `https://token-plan-cn.xiaomimimo.com/v1` and built-in presets for
  `mimo-v2.5-pro` / `mimo-v2.5`.
- The Tauri control app now embeds a Rust shim service serving `/health`,
  `/v1/models`, `/v1/responses`, and `/v1/chat/completions`.

#### Changed

- Tauri Start / Stop / Restart / Generate / Enable / Disable / Active model /
  Codex launch flows now run through native Rust code instead of depending on
  the Python project or `codex-shim` CLI.
- The embedded Rust shim covers the main OpenAI-compatible, Anthropic Messages,
  and ChatGPT passthrough paths, including streaming Responses conversion, tool
  calls, reasoning/thinking chunks, and MiniMax `reasoning_details[]`.
- macOS picker patch / restore now run from Tauri Rust commands while still
  using local `npx asar` and `codesign`.
- Root and Tauri app READMEs now document the standalone desktop app, MiMo, and
  embedded service behavior.

#### Verified

- `cargo check --offline`
- `cargo test --offline`
- `npm run build`
- `npm run tauri:build`
- `git diff --check`

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
