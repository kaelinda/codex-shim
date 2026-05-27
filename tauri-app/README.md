# Codex Shim Control

A Tauri v2 desktop GUI that wraps the [`codex-shim`](../README.md) Python
daemon. It lets you start/stop the shim, edit `~/.codex-shim/models.json`,
switch the active Codex Desktop model, and tail `shim.log` — without leaving
the CLI behind, just on top of it.

The app is intentionally a thin local control panel: it delegates daemon
startup, catalog generation, active-model switching, and picker patching to the
same `codex-shim` CLI that powers the terminal workflow.

---

## 它能做什么

- **Dashboard**：daemon 状态卡片、健康检查、Codex 登录态、当前 active model；
  一键 Start / Stop / Restart / Generate / Enable / Disable；macOS 专属的
  picker patch / restore。
- **Models**：直接读写父级仓库的 `~/.codex-shim/models.json`。支持表格 CRUD
  （新增、编辑、删除、上移、下移），也提供「直接编辑 JSON」开关。写入前会校验
  必填字段和 `provider` 是否在后端支持列表中，包括 OpenAI、Anthropic、DeepSeek、
  小米 MiMo、MiniMax、Kimi/Moonshot、阿里云百炼/DashScope、火山方舟等。
- **Active model**：从 `codex-shim list` 拉取所有 slug，点击即可调用
  `codex-shim model use <slug>`，写到 `~/.codex/config.toml` 的 managed block。
- **Logs**：tail 父级仓库的 `.codex-shim/shim.log`，可调 tail 大小（8K~512K），
  可开 2s 自动刷新。
- **Settings**：覆盖默认 `models.json` 路径、端口、CLI 可执行文件、project root
  自动探测结果——这些会在所有命令调用里生效。

## 最近的 UI / 稳定性改进

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
- Python 3.11+ 且 `codex-shim` CLI 在 PATH，或在 Settings 里手动指定 CLI 路径。
  应用会按下面的顺序探测 CLI：
  1. Settings 里你填的「codex-shim CLI」路径；
  2. `which codex-shim`；
  3. `python3 -m codex_shim.cli`（fallback：在仓库根目录运行）。

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
- daemon log 默认取仓库根目录下的 `.codex-shim/shim.log`，App 会通过
  `cwd` 上溯定位含 `codex_shim/` 的目录，定位失败时也可以在 Settings 里手动指定。
- Active model 直接调用 `codex-shim model use <slug>`，不会自己写
  `~/.codex/config.toml`——所有副作用都委托给 CLI，保持单一来源。
- ChatGPT passthrough 的状态来自直接读取 `~/.codex/auth.json`（只看
  `tokens.access_token` 是否非空），与 `chatgpt_passthrough_available` 同义。

## 已知限制

- Windows 上的 macOS picker patch 按钮会被自动隐藏；其他平台 `patch-app /
  restore-app` 会原样转发到 CLI，CLI 会自己报错。
- App 完全是 CLI 的 UI 层，**不会在窗口里跑长连接代理**。daemon 仍然是
  `codex-shim start` 起的子进程。
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

支持以下 provider：

| provider | 说明 | 默认 base_url |
|----------|------|---------------|
| `openai` | OpenAI 兼容 API | `https://api.openai.com/v1` |
| `anthropic` | Anthropic Messages API | `https://api.anthropic.com/v1` |
| `deepseek` | DeepSeek API（兼容 OpenAI） | `https://api.deepseek.com` |
| `mimo` | 小米 MiMo API（兼容 OpenAI） | `https://token-plan-cn.xiaomimimo.com/v1` |
| `minimax` | MiniMax API（兼容 OpenAI） | `https://api.minimax.io/v1` |
| `moonshot` | Kimi / Moonshot API（兼容 OpenAI） | `https://api.moonshot.cn/v1` |
| `dashscope` | 阿里云百炼 / DashScope 兼容模式 | `https://dashscope.aliyuncs.com/compatible-mode/v1` |
| `volcengine` | 火山方舟 OpenAI 兼容模式 | `https://ark.cn-beijing.volces.com/api/v3` |
| `generic-chat-completion-api` | 通用 OpenAI 兼容 API | （无默认） |

编辑模型时，每个 provider 还提供预设模型下拉选择（如 GPT-4o、Claude Sonnet 4、
DeepSeek V4 Pro、MiMo V2.5 Pro、MiniMax M2、Kimi K2、Qwen Plus、Doubao Seed 等）。

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
        ├── shim.rs            # 调用 codex-shim CLI
        ├── models.rs          # models.json 读写 + 校验
        ├── config.rs          # auth.json + config.toml + tail 日志
        ├── health.rs          # /health 探活
        ├── paths.rs           # 默认路径、project root 探测
        └── error.rs           # 统一错误类型
```

## License

跟随父项目 MIT。
