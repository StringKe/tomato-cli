#!/bin/sh
# 发版准备：改 Cargo.toml 版本号、刷新 Cargo.lock、用 git-cliff 生成 CHANGELOG.md、提交并打 tag
# 用法：scripts/release.sh 0.2.0，然后 git push origin main v0.2.0
set -eu

version="${1:?需要版本号，例如 0.2.0}"
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

for tool in git-cliff cargo; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "需要 $tool" >&2
    exit 1
  fi
done

if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "工作区有未提交的改动，先处理干净" >&2
  exit 1
fi

if git rev-parse "v$version" >/dev/null 2>&1; then
  echo "tag v$version 已存在" >&2
  exit 1
fi

# [package] 的 version 是 Cargo.toml 里唯一顶格的 version 行，依赖表里的都在 { } 内
sed -i.bak -e "s/^version = \"[^\"]*\"/version = \"$version\"/" Cargo.toml
rm -f Cargo.toml.bak
cargo check -q
git-cliff --tag "v$version" -o CHANGELOG.md

git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -q -m "chore(release): v$version"
git tag -a "v$version" -m "v$version"
echo "已提交并打 tag v$version，推送："
echo "  git push origin main v$version"
