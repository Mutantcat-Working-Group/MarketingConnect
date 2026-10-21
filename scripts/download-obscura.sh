#!/bin/bash
# 下载 Obscura sidecar 二进制（按当前平台自动选择）
#
# 用法:
#   macOS / Linux:  ./scripts/download-obscura.sh
#   Windows (Git Bash / WSL): ./scripts/download-obscura.sh
#   Windows (PowerShell):      .\scripts\download-obscura.ps1
#
# 下载完成后二进制位于 src-tauri/binaries/obscura-<target-triple>[.exe]
# Tauri 的 externalBin 配置会自动识别带目标三元组的文件。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BIN_DIR="$PROJECT_DIR/src-tauri/binaries"
RELEASE_BASE="https://github.com/h4ckf0r0day/obscura/releases/latest/download"

# 检测平台（含 Windows 的 Git Bash / MSYS / Cygwin 环境）
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64)  TRIPLE="aarch64-apple-darwin"; ASSET="obscura-aarch64-macos.tar.gz"; EXT="" ;;
      x86_64) TRIPLE="x86_64-apple-darwin";  ASSET="obscura-x86_64-macos.tar.gz";  EXT="" ;;
      *) echo "不支持的架构: $ARCH"; exit 1 ;;
    esac
    ;;
  Linux)
    case "$ARCH" in
      x86_64)  TRIPLE="x86_64-unknown-linux-gnu";  ASSET="obscura-x86_64-linux.tar.gz";  EXT="" ;;
      aarch64) TRIPLE="aarch64-unknown-linux-gnu"; ASSET="obscura-aarch64-linux.tar.gz"; EXT="" ;;
      *) echo "不支持的架构: $ARCH"; exit 1 ;;
    esac
    ;;
  MINGW*|MSYS*|CYGWIN*|Windows_NT)
    # Git Bash / MSYS2 / Cygwin on Windows
    TRIPLE="x86_64-pc-windows-msvc"
    ASSET="obscura-x86_64-windows.zip"
    EXT=".exe"
    ;;
  *)
    echo "不支持的操作系统: $OS"
    exit 1
    ;;
esac

TARGET="$BIN_DIR/obscura-$TRIPLE$EXT"

if [ -x "$TARGET" ]; then
  echo "已存在: $TARGET，跳过下载"
  exit 0
fi

mkdir -p "$BIN_DIR"
TMP_ASSET="$BIN_DIR/$ASSET"

echo "下载 Obscura ($ASSET) ..."
curl -fL --retry 3 -o "$TMP_ASSET" "$RELEASE_BASE/$ASSET"

echo "解压 ..."
case "$ASSET" in
  *.tar.gz)
    tar xzf "$TMP_ASSET" -C "$BIN_DIR"
    ;;
  *.zip)
    # Windows: 优先 unzip，其次 PowerShell Expand-Archive，最后 tar（Win10 1803+ 自带）
    if command -v unzip >/dev/null 2>&1; then
      unzip -o "$TMP_ASSET" -d "$BIN_DIR"
    elif command -v powershell.exe >/dev/null 2>&1; then
      powershell.exe -NoProfile -Command "Expand-Archive -Force -LiteralPath '$TMP_ASSET' -DestinationPath '$BIN_DIR'"
    else
      tar xf "$TMP_ASSET" -C "$BIN_DIR"
    fi
    ;;
esac

# 按 Tauri sidecar 约定重命名（Windows 加 .exe 后缀）
if [ -f "$BIN_DIR/obscura" ]; then
  mv "$BIN_DIR/obscura" "$TARGET"
fi
if [ -f "$BIN_DIR/obscura.exe" ]; then
  mv "$BIN_DIR/obscura.exe" "$TARGET"
fi
chmod +x "$TARGET" 2>/dev/null || true

# worker 仅 scrape 命令需要，serve 模式用不到，保留但不强制
if [ -f "$BIN_DIR/obscura-worker" ]; then
  mv "$BIN_DIR/obscura-worker" "$BIN_DIR/obscura-worker-$TRIPLE$EXT"
  chmod +x "$BIN_DIR/obscura-worker-$TRIPLE$EXT" 2>/dev/null || true
fi
if [ -f "$BIN_DIR/obscura-worker.exe" ]; then
  mv "$BIN_DIR/obscura-worker.exe" "$BIN_DIR/obscura-worker-$TRIPLE.exe"
fi

rm -f "$TMP_ASSET"
echo "完成: $TARGET"
"$TARGET" --version 2>/dev/null || echo "(版本查询失败，但文件已就位)"
