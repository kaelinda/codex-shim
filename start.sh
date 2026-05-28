#!/usr/bin/env bash
set -euo pipefail

SCRIPT_PATH="${BASH_SOURCE[0]:-}"
if [ -n "$SCRIPT_PATH" ] && [ -f "$SCRIPT_PATH" ]; then
  ROOT_DIR="$(cd "$(dirname "$SCRIPT_PATH")" && pwd)"
else
  ROOT_DIR="$(pwd)"
fi
CLI_DIR="$ROOT_DIR/cli"
REPO_URL="${CODEX_SHIM_REPO:-https://github.com/kaelinda/codex-shim.git}"
REPO_REF="${CODEX_SHIM_REF:-main}"
SOURCE_DIR="${CODEX_SHIM_SOURCE_DIR:-$HOME/.codex-shim/source}"
INSTALL_DIR="${CODEX_SHIM_INSTALL_DIR:-$HOME/.local/bin}"
BIN_NAME="codex-shim-cli"
TARGET_BIN="$CLI_DIR/target/release/$BIN_NAME"
INSTALLED_BIN="$INSTALL_DIR/$BIN_NAME"
PORT="${CODEX_SHIM_PORT:-8765}"

print_env_help() {
  cat <<'EOF'
codex-shim CLI 环境要求：
  必需工具：git、curl、cargo/rustc
  Rust 未安装时，本脚本会自动使用 rustup 安装：
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  安装 Git：
    macOS: xcode-select --install
    Debian/Ubuntu: sudo apt-get update && sudo apt-get install -y git
    Fedora: sudo dnf install -y git
EOF
}

print_provider_help() {
  cat <<'EOF'
Provider 快速参考：
  openai      base_url: https://api.openai.com/v1
  anthropic  base_url: https://api.anthropic.com/v1
  deepseek   base_url: https://api.deepseek.com
  moonshot   base_url: https://api.moonshot.cn/v1
  dashscope  base_url: https://dashscope.aliyuncs.com/compatible-mode/v1
  volcengine base_url: https://ark.cn-beijing.volces.com/api/v3
  custom     任意兼容 OpenAI /v1 chat-completions 的网关
EOF
}

echo "== codex-shim 轻量 CLI 安装器 =="
echo
print_env_help
echo
echo "当前环境检测："
if command -v git >/dev/null 2>&1; then
  echo "  git:   $(git --version)"
else
  echo "  git:   未安装"
fi
if command -v curl >/dev/null 2>&1; then
  echo "  curl:  $(curl --version | head -n 1)"
else
  echo "  curl:  未安装"
fi
if command -v cargo >/dev/null 2>&1; then
  echo "  cargo: $(cargo --version)"
else
  echo "  cargo: 未安装"
fi
if command -v rustc >/dev/null 2>&1; then
  echo "  rustc: $(rustc --version)"
else
  echo "  rustc: 未安装"
fi
echo

if ! command -v curl >/dev/null 2>&1; then
  echo "缺少 curl，无法自动安装 Rust 或远程下载安装脚本。请先安装 curl 后重新运行。" >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "未检测到 cargo，准备自动安装 Rust 工具链。"
  echo "安装方式：rustup 官方脚本，默认参数 -y。"
  if [ "${CODEX_SHIM_AUTO_INSTALL_RUST:-1}" = "0" ]; then
    echo "已设置 CODEX_SHIM_AUTO_INSTALL_RUST=0，跳过自动安装 Rust。" >&2
    echo "请先安装 Rust 后重新运行本脚本。" >&2
    exit 1
  fi
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
  else
    export PATH="$HOME/.cargo/bin:$PATH"
  fi
  if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust 安装后仍未找到 cargo。请重启终端后重新运行本脚本。" >&2
    exit 1
  fi
  echo "Rust 安装完成：$(cargo --version)"
fi

if [ ! -f "$CLI_DIR/Cargo.toml" ]; then
  if ! command -v git >/dev/null 2>&1; then
    echo "当前是单独下载 start.sh 的安装方式，需要 git 拉取源码。请先安装 git。" >&2
    exit 1
  fi

  if [ -f "$SOURCE_DIR/cli/Cargo.toml" ]; then
    echo "检测到已有源码目录：$SOURCE_DIR"
    ROOT_DIR="$SOURCE_DIR"
    CLI_DIR="$ROOT_DIR/cli"
  else
    echo "未在脚本目录找到 cli/ 源码，准备下载 codex-shim 源码。"
    echo "源码仓库：$REPO_URL"
    echo "源码分支：$REPO_REF"
    echo "保存目录：$SOURCE_DIR"
    TMP_SOURCE="$SOURCE_DIR.tmp.$$"
    rm -rf "$TMP_SOURCE"
    mkdir -p "$(dirname "$SOURCE_DIR")"
    if ! git clone --depth 1 --branch "$REPO_REF" "$REPO_URL" "$TMP_SOURCE"; then
      if [ "$REPO_REF" != "main" ]; then
        echo "下载指定分支失败，改用 main 重试。"
        rm -rf "$TMP_SOURCE"
        git clone --depth 1 --branch "main" "$REPO_URL" "$TMP_SOURCE"
      else
        exit 1
      fi
    fi
    if [ ! -f "$TMP_SOURCE/cli/Cargo.toml" ]; then
      echo "下载的源码中没有 cli/Cargo.toml。请将 CODEX_SHIM_REF 设置为包含 Rust CLI 的分支或 tag。" >&2
      rm -rf "$TMP_SOURCE"
      exit 1
    fi
    rm -rf "$SOURCE_DIR"
    mv "$TMP_SOURCE" "$SOURCE_DIR"
    ROOT_DIR="$SOURCE_DIR"
    CLI_DIR="$ROOT_DIR/cli"
  fi
fi

TARGET_BIN="$CLI_DIR/target/release/$BIN_NAME"

echo "开始构建 $BIN_NAME..."
cargo build --manifest-path "$CLI_DIR/Cargo.toml" --release

mkdir -p "$INSTALL_DIR"
cp "$TARGET_BIN" "$INSTALLED_BIN"
chmod +x "$INSTALLED_BIN"

echo "已安装 $BIN_NAME 到：$INSTALLED_BIN"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo "提示：$INSTALL_DIR 当前不在 PATH 中。可以把下面这行加入你的 shell 配置："
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac

SETTINGS_PATH="${CODEX_SHIM_SETTINGS:-$HOME/.codex-shim/models.json}"
if [ ! -s "$SETTINGS_PATH" ]; then
  if [ -t 0 ]; then
    echo
    echo "未找到模型配置文件：$SETTINGS_PATH"
    echo "接下来可以按提示填写 provider、base_url、model 和 API Key。"
    print_provider_help
    echo
    read -r -p "现在开始配置 API Key 吗？[Y/n] " answer
    case "${answer:-Y}" in
      [Yy]*)
        "$INSTALLED_BIN" --settings "$SETTINGS_PATH" configure
        ;;
      *)
        echo "已跳过模型配置。稍后可运行 '$BIN_NAME configure' 继续配置。"
        ;;
    esac
  else
    echo "未找到模型配置文件：$SETTINGS_PATH。请在交互式终端运行 '$BIN_NAME configure'。"
  fi
fi

echo
echo "正在启动 codex-shim..."
"$INSTALLED_BIN" --settings "$SETTINGS_PATH" --port "$PORT" start
echo
echo "后续常用命令："
echo "  $BIN_NAME list"
echo "  $BIN_NAME enable        # 写入 ~/.codex/config.toml 托管配置"
echo "  $BIN_NAME model use <slug>"
