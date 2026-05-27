# Codex Shim Control

A Tauri v2 desktop GUI with an embedded Rust shim service. It lets you
start/stop the local shim, edit `~/.codex-shim/models.json`, generate the Codex
model catalog/config, switch the active Codex Desktop model, and tail
`shim.log`.

The app has been migrated away from the Python daemon path. Dashboard
Start/Stop/Restart, Generate, Enable/Disable, health checks, model listing,
active-model switching, OpenAI-compatible routing, Anthropic routing, ChatGPT
passthrough, Codex Desktop launch, and macOS picker patch/restore now use Rust
code inside the app.

---

## 它能做什么

- **Dashboard**：daemon 状态卡片、健康检查、Codex 登录态、当前 active model；
  一键 Start / Stop / Restart / Generate / Enable / Disable。Start/Enable
  启动的是 app 内置 Rust shim 服务，Generate 会写入 app runtime 下的 catalog/config。
  Codex Desktop launch 和 macOS picker patch / restore 也由 Rust 命令层直接执行。
- **Models**：直接读写父级仓库的 `~/.codex-shim/models.json`。支持表格 CRUD
  （新增、编辑、删除、上移、下移），也提供「直接编辑 JSON」开关。写入前会校验
  必填字段和 `provider` 是否在后端支持列表中，包括 OpenAI、Anthropic、DeepSeek、
  小米 MiMo、MiniMax、Kimi/Moonshot、阿里云百炼/DashScope、火山方舟等。
- **Active model**：从 Rust 侧读取所有 slug，点击即可由 app 写到
  `~/.codex/config.toml` 的 managed block。
- **Logs**：tail app runtime 下的 `~/.codex-shim/app/shim.log`，可调 tail 大小（8K~512K），
  可开 2s 自动刷新。
- **Settings**：覆盖默认 `models.json` 路径和端口。

## 最近的 UI / 稳定性改进

- **v0.4.0 / 2026-05-27**
  - 中文：Models 页新增 provider 配置导入、导出和脱敏导出；Settings 页新增
    GitHub Releases 检查、App 下载入口和 CLI 更新按钮。Rust CLI 同步新增
    `test`、`export`、`import`、`version`、`update` 命令，模型 slug 现在会保留
    `minimax-m2.7` 这类名称中的点号。
  - English: Models now supports provider config import, export, and redacted
    export. Settings can check GitHub Releases, open App downloads, and update
    the CLI helper. The Rust CLI also adds `test`, `export`, `import`,
    `version`, and `update`, and model slugs now preserve dots such as
    `minimax-m2.7`.
- **v0.3.0 / 2026-05-27**
  - 中文：Tauri app 已成为独立桌面控制台，内置 Rust shim 服务；Start /
    Restart / Generate / Enable / Disable / Active model / Codex launch 不再
    调用 Python 项目或 `codex-shim` CLI。新增小米 MiMo provider，默认
    `base_url` 为 `https://token-plan-cn.xiaomimimo.com/v1`，内置
    `MiMo-V2.5-Pro` 和 `MiMo-V2.5` 预设。
  - English: The Tauri app is now a standalone desktop control panel with an
    embedded Rust shim service. Start / Restart / Generate / Enable / Disable /
    Active model / Codex launch no longer call the Python project or
    `codex-shim` CLI. Xiaomi MiMo is supported with the default
    `https://token-plan-cn.xiaomimimo.com/v1` base URL and built-in
    `MiMo-V2.5-Pro` / `MiMo-V2.5` presets.
- **v0.2.0 / 2026-05-27**
  - 中文：Models 表单新增 MiniMax、Kimi/Moonshot、阿里云百炼/DashScope、
    火山方舟 provider；补齐默认 `base_url`、常用模型预设和 Rust 写入校验。
    后端同步优化 `thinking` 兼容策略，DeepSeek、`kimi-*` 和普通
    OpenAI-compatible 模型会使用不同的请求形态，MiniMax `reasoning_details[]`
    也会回传为 Codex reasoning。
  - English: Models now exposes MiniMax, Kimi/Moonshot, Alibaba Cloud
    Bailian/DashScope, and Volcengine Ark with default base URLs, presets, and
    matching Rust-side validation. The shim also gates `thinking` per
    provider/model and normalizes MiniMax `reasoning_details[]` into Codex
    reasoning output.
- Dashboard、Models、Active、Logs、Settings 统一了卡片层级、按钮图标、状态色
  token、focus ring、toast 和错误样式。
- Dashboard 移动端改为单列页面与稳定的两列操作区，按钮不再因中文换行被压成竖排。
- Models 弹窗补齐 `role="dialog"`、焦点陷阱、Esc 关闭、焦点恢复和字段 label 绑定。
- App 入口增加 error boundary，单个 tab 渲染失败时不会拖垮整个窗口。
- 健康检查和日志自动刷新改为自适应轮询：请求不重入，页面隐藏时暂停。
- 浏览器 Mock API 不再向 console 输出每次调用日志，方便前端调试时只看真正的警告和错误。

## 依赖与前置条件

- Node.js ≥ 18 + pnpm/npm（任选其一，本项目脚本写的是 `npm`）。
- Rust（稳定版）+ 平台对应的 Tauri 系统依赖，参考
  <https://v2.tauri.app/start/prerequisites/>。
- 不需要 Python 或 `codex-shim` CLI。Tauri app 内置 Rust shim 服务和所需的
  catalog/config/launch/patch 命令逻辑。
- Settings 页支持检查 GitHub Releases 更新，并可调用 `start.sh` 重新安装
  `codex-shim-cli`。App 安装包本身仍通过 Releases 页面下载更新。

## 安装与构建

```bash
cd tauri-app

# 1. 装前端依赖
npm install

# 2. 开发模式（自动 build vite + 启动 tauri 窗口）
npm run tauri:dev

# 3. 打 release 包
npm run tauri:build
```

> 如果是第一次跑 Tauri v2，`tauri-build` 会要求安装目标平台的 Rust toolchain
> 以及一些系统库（macOS：Xcode CLT；Linux：webkit2gtk-4.1, librsvg2-dev…；
> Windows：MSVC、WebView2 Runtime）。

Release 包默认写入 `src-tauri/target/release/bundle/`，例如 macOS 上会生成
`.dmg` 和 `.app.tar.gz` 等 bundle。

## 图标

`src-tauri/tauri.conf.json` 引用了 `src-tauri/icons/{32x32.png, 128x128.png,
128x128@2x.png, icon.icns, icon.ico}`。仓库已包含一组可构建图标；如果需要替换，
用一张 1024x1024 PNG 重新生成：

```bash
# 准备一张 1024x1024 的 PNG 放到 src-tauri/icons/source.png
cd src-tauri
npx @tauri-apps/cli icon icons/source.png
```

或者直接用项目自带的 skill：`generate-tauri-app-icon`。

## 与 codex-shim 仓库的关系

- 默认从 `~/.codex-shim/models.json` 读写模型清单（与 CLI 完全一致）。
- Dashboard Start 启动 app 内置 Rust HTTP 服务，监听 `127.0.0.1:<port>`，
  提供 `/health`、`/v1/models`、`/v1/responses`、`/v1/chat/completions`。
  OpenAI-compatible、Anthropic Messages 和 ChatGPT passthrough 都由内置服务处理。
- Generate 由 app 写入 `~/.codex-shim/app/custom_model_catalog.json` 和
  `~/.codex-shim/app/config.toml`，不再调用 Python CLI。
- Active model 直接由 app 写入 `~/.codex/config.toml` 的 managed block，
  指向内置服务的 `/v1` endpoint。
- ChatGPT passthrough 的状态来自直接读取 `~/.codex/auth.json`（只看
  `tokens.access_token` 是否非空），与 `chatgpt_passthrough_available` 同义。

## 已知限制

- 内置 Rust shim 当前覆盖 OpenAI-compatible、Anthropic 和 ChatGPT passthrough
  的主要 `/v1/responses` 路径。
- macOS picker patch / restore 只支持 macOS，并依赖系统里有 `npx asar` 运行能力
  和 `codesign`。
- 因为 Tauri v2 的 capability 系统比较新，如果你换了 Tauri 次版本，可能需要
  调整 `src-tauri/capabilities/default.json` 里的 permission 列表。

## 浏览器调试模式

在纯浏览器（非 Tauri webview）中运行时，会自动启用 Mock API 模式，方便前端调试：

```bash
npm run dev  # 启动 Vite dev server
# 浏览器打开 http://127.0.0.1:1420/
```

Mock 模式下所有 Rust 命令调用会被拦截并返回模拟数据，可以在 Console 中看到
Vite / React 自身的开发日志；Mock API 本身不会为每次调用刷日志。

## Provider 支持

内置以下 provider 建议，也可以直接输入自定义 provider：

| provider | 说明 | 默认 base_url |
|----------|------|---------------|
| `openai` | OpenAI 兼容 API | `https://api.openai.com/v1` |
| `anthropic` | Anthropic Messages API | `https://api.anthropic.com/v1` |
| `new-api` | New API / One API 等自建 OpenAI 兼容网关 | （无默认） |
| `deepseek` | DeepSeek API（兼容 OpenAI） | `https://api.deepseek.com` |
| `mimo` | 小米 MiMo API（兼容 OpenAI） | `https://token-plan-cn.xiaomimimo.com/v1` |
| `minimax` | MiniMax API（兼容 OpenAI） | `https://api.minimax.io/v1` |
| `moonshot` | Kimi / Moonshot API（兼容 OpenAI） | `https://api.moonshot.cn/v1` |
| `dashscope` | 阿里云百炼 / DashScope 兼容模式 | `https://dashscope.aliyuncs.com/compatible-mode/v1` |
| `volcengine` | 火山方舟 OpenAI 兼容模式 | `https://ark.cn-beijing.volces.com/api/v3` |
| `generic-chat-completion-api` | 通用 OpenAI 兼容 API | （无默认） |

除 `anthropic` 外，其他 provider（包括自定义 provider）都会按
OpenAI-compatible `/chat/completions` 路由。编辑模型时，内置 provider 会提供预设
模型下拉选择（如 GPT-4o、Claude Sonnet 4、DeepSeek V4 Pro、MiMo V2.5 Pro、
MiniMax M2、Kimi K2、Qwen Plus、Doubao Seed 等）。

## 目录结构

```
tauri-app/
├── index.html
├── package.json
├── vite.config.ts
├── tsconfig.json / tsconfig.node.json
├── src/                       # React + TS 前端
│   ├── main.tsx, App.tsx, styles.css
│   ├── api.ts, types.ts
│   └── components/
│       ├── Sidebar.tsx, StatusBar.tsx
│       ├── ErrorBoundary.tsx, Icon.tsx
│       ├── Dashboard.tsx
│       ├── ModelsManager.tsx, ModelForm.tsx
│       ├── ActiveModel.tsx
│       ├── LogViewer.tsx
│       └── SettingsPanel.tsx
│   └── hooks/
│       └── useAdaptivePolling.ts
└── src-tauri/                 # Rust 后端
    ├── Cargo.toml, build.rs, tauri.conf.json
    ├── capabilities/default.json
    ├── icons/                 # 占位目录（需要用户自行生成）
    └── src/
        ├── main.rs, lib.rs
        ├── commands.rs        # 所有 #[tauri::command]
        ├── embedded_shim.rs   # app 内置 Rust HTTP shim
        ├── catalog.rs         # Codex model catalog/config 生成
        ├── models.rs          # models.json 读写 + 校验
        ├── config.rs          # auth.json + config.toml + tail 日志
        ├── health.rs          # /health 探活
        ├── paths.rs           # 默认路径与 app runtime 路径
        └── error.rs           # 统一错误类型
```

## License

跟随父项目 MIT。
