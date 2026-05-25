# Codex Shim Control

A Tauri v2 desktop GUI that wraps the [`codex-shim`](../README.md) Python
daemon. It lets you start/stop the shim, edit `~/.codex-shim/models.json`,
switch the active Codex Desktop model, and tail `shim.log` — without leaving
the CLI behind, just on top of it.

> 这个目录里的所有代码都尚未在本机执行过。请按下面的步骤自行安装依赖并构建。

---

## 它能做什么

- **Dashboard**：daemon 状态卡片、健康检查、Codex 登录态、当前 active model；
  一键 Start / Stop / Restart / Generate / Enable / Disable；macOS 专属的
  picker patch / restore。
- **Models**：直接读写父级仓库的 `~/.codex-shim/models.json`。支持表格 CRUD
  （含上移/下移/复制），也提供「直接编辑 JSON」开关。写入前会校验必填字段和
  `provider` 是否在 `openai / anthropic / generic-chat-completion-api` 中。
- **Active model**：从 `codex-shim list` 拉取所有 slug，点击即可调用
  `codex-shim model use <slug>`，写到 `~/.codex/config.toml` 的 managed block。
- **Logs**：tail 父级仓库的 `.codex-shim/shim.log`，可调 tail 大小（8K~512K），
  可开 2s 自动刷新。
- **Settings**：覆盖默认 `models.json` 路径、端口、CLI 可执行文件、project root
  自动探测结果——这些会在所有命令调用里生效。

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

## 图标占位

`src-tauri/tauri.conf.json` 引用了 `src-tauri/icons/{32x32.png, 128x128.png,
128x128@2x.png, icon.icns, icon.ico}`，但仓库里没有附二进制图标。生成方式：

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
- 没有内置图标二进制；首次 `tauri:build` 会因找不到图标失败，请先按上面的步骤
  生成。
- 因为 Tauri v2 的 capability 系统比较新，如果你换了 Tauri 次版本，可能需要
  调整 `src-tauri/capabilities/default.json` 里的 permission 列表。

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
│       ├── Dashboard.tsx
│       ├── ModelsManager.tsx, ModelForm.tsx
│       ├── ActiveModel.tsx
│       ├── LogViewer.tsx
│       └── SettingsPanel.tsx
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
