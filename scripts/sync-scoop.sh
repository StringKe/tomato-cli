#!/bin/sh
# 按指定版本的 GitHub Release 资产更新 bucket/tomato.json 的版本、下载地址和 sha256
# 用法：scripts/sync-scoop.sh 0.1.1
set -eu

REPO="StringKe/tomato-cli"
MANIFEST="$(cd "$(dirname "$0")/.." && pwd)/bucket/tomato.json"
version="${1:?需要版本号，例如 0.1.1}"
base="https://github.com/${REPO}/releases/download/v${version}"

if ! command -v jq >/dev/null 2>&1; then
  echo "需要 jq" >&2
  exit 1
fi

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

sha() {
  curl -fsSL --retry 3 --retry-delay 2 "$base/$1" -o "$tmpdir/$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$tmpdir/$1" | cut -c1-64
  else
    shasum -a 256 "$tmpdir/$1" | cut -c1-64
  fi
}

x64=$(sha tomato-x86_64-pc-windows-msvc.zip)
arm64=$(sha tomato-aarch64-pc-windows-msvc.zip)

jq --indent 4 \
  --arg v "$version" \
  --arg x64_url "$base/tomato-x86_64-pc-windows-msvc.zip" --arg x64 "$x64" \
  --arg arm64_url "$base/tomato-aarch64-pc-windows-msvc.zip" --arg arm64 "$arm64" \
  '.version = $v
   | .architecture["64bit"].url = $x64_url | .architecture["64bit"].hash = $x64
   | .architecture.arm64.url = $arm64_url | .architecture.arm64.hash = $arm64' \
  "$MANIFEST" > "$tmpdir/manifest.json"
mv "$tmpdir/manifest.json" "$MANIFEST"
echo "已更新 $MANIFEST -> $version"
