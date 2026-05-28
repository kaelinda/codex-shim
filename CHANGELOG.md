# Changelog

All notable changes to this project will be documented in this file.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/),
and this project does not yet follow semantic versioning (pre-1.0).

## Unreleased

## 0.6.0 — 2026-05-29

### 中文

#### 新增

- Tauri 控制台新增完整视觉改版：Dashboard 首屏改为非对称工作台布局，包含
  daemon 路由状态、endpoint、active model、模型数量和当前连接信息。
- 前端图标系统切换到 `@radix-ui/react-icons`，替换原有字符图标，并新增
  SVG favicon，避免浏览器调试时出现 favicon 404。

#### 优化

- 重写全局界面样式，统一 Sidebar、Topbar、按钮、卡片、表格、表单、弹窗、
  toast、空态、错误态、focus ring 和深色模式 token。
- Dashboard 控制区重新分组为主连接信息和右侧 daemon 操作栏，移动端折叠为
  单列布局，390px 宽度下无横向溢出。
- Models 表格、模型表单、日志页和设置页继承新的视觉层级，减少默认管理台感。

#### 验证

- `npm run build`
- `git diff --check`
- Vite + Playwright 浏览器检查：Dashboard 桌面视口、390px 移动视口、Models
  弹窗均能渲染，移动视口 `scrollWidth === clientWidth`。

### English

#### Added

- Added a full visual redesign for the Tauri control app. The Dashboard now has
  an asymmetric workstation layout with daemon route state, endpoint, active
  model, model count, and current connection details.
- Switched the frontend icon system to `@radix-ui/react-icons`, replacing the
  previous text-symbol icons, and added an SVG favicon to remove browser
  favicon 404 noise during debugging.

#### Changed

- Reworked the global UI system across Sidebar, Topbar, buttons, cards, tables,
  forms, modals, toast, empty states, error states, focus rings, and dark-mode
  tokens.
- Reorganized Dashboard controls into primary connection details plus a right
  daemon action rail. Mobile collapses to a single-column layout with no
  horizontal overflow at 390px.
- Models tables, model forms, Logs, and Settings now inherit the new visual
  hierarchy instead of the previous default admin-console feel.

#### Verified

- `npm run build`
- `git diff --check`
- Vite + Playwright browser checks: Dashboard desktop viewport, 390px mobile
  viewport, and Models modal rendered correctly; mobile
  `scrollWidth === clientWidth`.

## 0.5.0 — 2026-05-28

### 中文

#### 新增

- `codex-shim-cli` 新增 `patch-app` / `restore-app`，远程 `start.sh` 安装后的
  Rust CLI 现在可以直接给 macOS Codex Desktop 模型选择器打补丁，也可以撤销补丁并
  恢复原始 Codex Desktop bundle 文件。
- `patch-app` 会保存原始 `app.asar` 和 `Info.plist` 到 `~/.codex-shim/cli/`；
  `restore-app` 会从备份恢复原始文件并重新签名 `Codex.app`。如果 Tauri 控制台已经
  生成过 `~/.codex-shim/app/` 备份，CLI 会复用该备份。
- `start.sh` 支持自动安装 Rust：当缺少 `cargo` / `rustc` 时，会在交互式终端中引导
  运行 `rustup` 安装，降低首次安装 `codex-shim-cli` 的成本。
- 刷新 macOS app icon：源图改为符合 Apple HIG 交付预期的 1024×1024 不透明正方形，
  并重新生成 Tauri 全套图标资源。

#### 修复

- 当 CLI 检测到 Codex Desktop 已经打过 picker 补丁但找不到原始备份时，会拒绝继续
  覆盖备份，避免把已修改的 `app.asar` 误当作原始文件。

#### 验证

- `python3.11 -m compileall codex_shim -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3.11 -m pytest -p pytest_asyncio.plugin tests/ -q`
- `cargo check --offline`（`tauri-app/src-tauri`）
- `cargo test --offline`（`tauri-app/src-tauri`）
- `cargo check`（`cli`）
- `cargo test`（`cli`）
- `apple-app-icon-hig` 图标自检：源图与基础 PNG 均为完整不透明正方形，`.icns` 包含
  16×16 到 1024×1024 标准尺寸。
- `npm run build`
- `npm run tauri:build`
- `git diff --check`

### English

#### Added

- Added `codex-shim-cli patch-app` / `restore-app`, so the Rust CLI installed
  by the remote `start.sh` flow can patch the macOS Codex Desktop model picker
  and undo that patch by restoring the original Codex Desktop bundle files.
- `patch-app` saves the original `app.asar` and `Info.plist` under
  `~/.codex-shim/cli/`; `restore-app` restores them and re-signs `Codex.app`.
  If the Tauri control app already created backups under `~/.codex-shim/app/`,
  the CLI reuses them.
- `start.sh` can now install Rust automatically: when `cargo` / `rustc` are
  missing, it guides interactive terminals through `rustup` installation before
  building `codex-shim-cli`.
- Refreshed the macOS app icon: the source is now a 1024×1024 opaque square
  suitable for Apple HIG handoff, and the full Tauri icon set was regenerated.

#### Fixed

- If the CLI detects an already-patched Codex Desktop picker without an original
  backup, it refuses to overwrite the backup, avoiding a modified `app.asar`
  being treated as the original file.

#### Verified

- `python3.11 -m compileall codex_shim -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3.11 -m pytest -p pytest_asyncio.plugin tests/ -q`
- `cargo check --offline` (`tauri-app/src-tauri`)
- `cargo test --offline` (`tauri-app/src-tauri`)
- `cargo check` (`cli`)
- `cargo test` (`cli`)
- `apple-app-icon-hig` icon audit: source and base PNG files are full opaque
  squares, and `.icns` includes standard 16×16 through 1024×1024 sizes.
- `npm run build`
- `npm run tauri:build`
- `git diff --check`

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
