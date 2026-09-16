#!/bin/sh
# tomato-cli 安装脚本（macOS / Linux）
# curl -fsSL https://raw.githubusercontent.com/StringKe/tomato-cli/main/scripts/install.sh | sh
set -eu

REPO="StringKe/tomato-cli"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)

case "$os" in
  linux)
    # 非 glibc 系统（Alpine 等）用 musl 静态包；TOMATO_LIBC=musl 可强制
    libc="${TOMATO_LIBC:-}"
    if [ -z "$libc" ]; then
      case "$(ldd --version 2>&1 || true)" in
        *GLIBC* | *glibc* | *"GNU libc"*) libc="gnu" ;;
        *) libc="musl" ;;
      esac
    fi
    os_tag="unknown-linux-${libc}"
    ;;
  darwin) os_tag="apple-darwin" ;;
  *)
    echo "不支持的系统: $os" >&2
    exit 1
    ;;
esac

case "$arch" in
  x86_64 | amd64) arch_tag="x86_64" ;;
  arm64 | aarch64) arch_tag="aarch64" ;;
  *)
    echo "不支持的架构: $arch" >&2
    exit 1
    ;;
esac

target="${arch_tag}-${os_tag}"
url="https://github.com/${REPO}/releases/latest/download/tomato-${target}.tar.gz"

echo "安装 tomato (${target})"
echo "来源 ${url}"

if ! command -v curl >/dev/null 2>&1; then
  echo "需要 curl" >&2
  exit 1
fi

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

curl -fsSL --retry 3 --retry-delay 1 "$url" -o "$tmpdir/tomato.tar.gz"
tar -xzf "$tmpdir/tomato.tar.gz" -C "$tmpdir"

if [ ! -f "$tmpdir/tomato" ]; then
  echo "压缩包里没有 tomato 可执行文件" >&2
  exit 1
fi

mkdir -p "$BIN_DIR"
chmod 755 "$tmpdir/tomato"
mv "$tmpdir/tomato" "$BIN_DIR/tomato"

echo "已安装到 $BIN_DIR/tomato"
"$BIN_DIR/tomato" --version

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    echo "把 $BIN_DIR 加入 PATH 后即可运行 tomato"
    echo "更新：tomato update"
    ;;
esac
