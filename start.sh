#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLI_DIR="$ROOT_DIR/cli"
REPO_URL="${CODEX_SHIM_REPO:-https://github.com/0xSero/codex-shim.git}"
REPO_REF="${CODEX_SHIM_REF:-main}"
SOURCE_DIR="${CODEX_SHIM_SOURCE_DIR:-$HOME/.codex-shim/source}"
INSTALL_DIR="${CODEX_SHIM_INSTALL_DIR:-$HOME/.local/bin}"
BIN_NAME="codex-shim-cli"
TARGET_BIN="$CLI_DIR/target/release/$BIN_NAME"
INSTALLED_BIN="$INSTALL_DIR/$BIN_NAME"
PORT="${CODEX_SHIM_PORT:-8765}"

print_env_help() {
  cat <<'EOF'
codex-shim CLI environment:
  required: git, cargo/rustc
  install Rust:
    macOS/Linux: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  install Git:
    macOS: xcode-select --install
    Debian/Ubuntu: sudo apt-get update && sudo apt-get install -y git
    Fedora: sudo dnf install -y git
EOF
}

print_provider_help() {
  cat <<'EOF'
Provider quick reference:
  openai      base_url: https://api.openai.com/v1
  anthropic  base_url: https://api.anthropic.com/v1
  deepseek   base_url: https://api.deepseek.com
  moonshot   base_url: https://api.moonshot.cn/v1
  dashscope  base_url: https://dashscope.aliyuncs.com/compatible-mode/v1
  volcengine base_url: https://ark.cn-beijing.volces.com/api/v3
  custom     any OpenAI-compatible /v1 chat-completions gateway
EOF
}

echo "== codex-shim lightweight CLI installer =="
echo
print_env_help
echo
echo "Detected environment:"
if command -v git >/dev/null 2>&1; then
  echo "  git:   $(git --version)"
else
  echo "  git:   missing"
fi
if command -v cargo >/dev/null 2>&1; then
  echo "  cargo: $(cargo --version)"
else
  echo "  cargo: missing"
fi
if command -v rustc >/dev/null 2>&1; then
  echo "  rustc: $(rustc --version)"
else
  echo "  rustc: missing"
fi
echo

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required to build the Rust CLI. Install Rust, restart your shell, then rerun this script." >&2
  exit 1
fi

if [ ! -f "$CLI_DIR/Cargo.toml" ]; then
  if ! command -v git >/dev/null 2>&1; then
    echo "git is required when start.sh is downloaded without the full repository." >&2
    exit 1
  fi

  if [ -f "$SOURCE_DIR/cli/Cargo.toml" ]; then
    ROOT_DIR="$SOURCE_DIR"
    CLI_DIR="$ROOT_DIR/cli"
  else
    echo "No local cli/ checkout found. Downloading codex-shim source..."
    TMP_SOURCE="$SOURCE_DIR.tmp.$$"
    rm -rf "$TMP_SOURCE"
    mkdir -p "$(dirname "$SOURCE_DIR")"
    if ! git clone --depth 1 --branch "$REPO_REF" "$REPO_URL" "$TMP_SOURCE"; then
      if [ "$REPO_REF" != "feature/cli" ]; then
        echo "Retrying with feature/cli..."
        rm -rf "$TMP_SOURCE"
        git clone --depth 1 --branch "feature/cli" "$REPO_URL" "$TMP_SOURCE"
      else
        exit 1
      fi
    fi
    if [ ! -f "$TMP_SOURCE/cli/Cargo.toml" ]; then
      echo "Downloaded source does not contain cli/Cargo.toml. Set CODEX_SHIM_REF to a branch or tag with the Rust CLI." >&2
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

echo "Building $BIN_NAME..."
cargo build --manifest-path "$CLI_DIR/Cargo.toml" --release

mkdir -p "$INSTALL_DIR"
cp "$TARGET_BIN" "$INSTALLED_BIN"
chmod +x "$INSTALLED_BIN"

echo "Installed $BIN_NAME to $INSTALLED_BIN"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo "Note: $INSTALL_DIR is not on PATH. Add this to your shell profile:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac

SETTINGS_PATH="${CODEX_SHIM_SETTINGS:-$HOME/.codex-shim/models.json}"
if [ ! -s "$SETTINGS_PATH" ]; then
  if [ -t 0 ]; then
    echo
    echo "No model config found at $SETTINGS_PATH."
    print_provider_help
    echo
    read -r -p "Configure an API key now? [Y/n] " answer
    case "${answer:-Y}" in
      [Yy]*)
        "$INSTALLED_BIN" --settings "$SETTINGS_PATH" configure
        ;;
      *)
        echo "Skipped model configuration. Run '$BIN_NAME configure' later."
        ;;
    esac
  else
    echo "No model config found at $SETTINGS_PATH. Run '$BIN_NAME configure' in a terminal."
  fi
fi

echo
echo "Starting codex-shim..."
"$INSTALLED_BIN" --settings "$SETTINGS_PATH" --port "$PORT" start
echo
echo "Next commands:"
echo "  $BIN_NAME list"
echo "  $BIN_NAME enable        # write the managed ~/.codex/config.toml block"
echo "  $BIN_NAME model use <slug>"
