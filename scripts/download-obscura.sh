#!/bin/bash
# 下载 Obscura sidecar 二进制（按当前平台自动选择）
#
# 用法: ./scripts/download-obscura.sh
#
# 下载完成后二进制位于 src-tauri/binaries/obscura-<target-triple>
# Tauri 的 externalBin 配置会自动识别带目标三元组的文件。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BIN_DIR="$PROJECT_DIR/src-tauri/binaries"
RELEASE_BASE="https://github.com/h4ckf0r0day/obscura/releases/latest/download"

# 检测平台
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64)  TRIPLE="aarch64-apple-darwin"; ASSET="obscura-aarch64-macos.tar.gz" ;;
      x86_64) TRIPLE="x86_64-apple-darwin";  ASSET="obscura-x86_64-macos.tar.gz" ;;
      *) echo "不支持的架构: $ARCH"; exit 1 ;;
    esac
    ;;
  Linux)
    case "$ARCH" in
      x86_64)  TRIPLE="x86_64-unknown-linux-gnu";  ASSET="obscura-x86_64-linux.tar.gz" ;;
      aarch64) TRIPLE="aarch64-unknown-linux-gnu"; ASSET="obscura-aarch64-linux.tar.gz" ;;
      *) echo "不支持的架构: $ARCH"; exit 1 ;;
    esac
    ;;
  *)
    echo "不支持的操作系统: $OS（Windows 请手动从 releases 页面下载 .zip）"
    exit 1
    ;;
esac

TARGET="$BIN_DIR/obscura-$TRIPLE"

if [ -x "$TARGET" ]; then
  echo "已存在: $TARGET，跳过下载"
  exit 0
fi

mkdir -p "$BIN_DIR"
echo "下载 Obscura ($ASSET) ..."
TMP_TAR="$BIN_DIR/$ASSET"
curl -fL --retry 3 -o "$TMP_TAR" "$RELEASE_BASE/$ASSET"

echo "解压 ..."
tar xzf "$TMP_TAR" -C "$BIN_DIR"

# obscura 与 obscura-worker 需按目标三元组命名（Tauri sidecar 约定）
if [ -f "$BIN_DIR/obscura" ]; then
  mv "$BIN_DIR/obscura" "$TARGET"
  chmod +x "$TARGET"
fi
if [ -f "$BIN_DIR/obscura-worker" ]; then
  mv "$BIN_DIR/obscura-worker" "$BIN_DIR/obscura-worker-$TRIPLE"
  chmod +x "$BIN_DIR/obscura-worker-$TRIPLE"
fi

rm -f "$TMP_TAR"
echo "完成: $TARGET"
"$TARGET" --version 2>/dev/null || echo "(版本查询失败，但文件已就位)"
