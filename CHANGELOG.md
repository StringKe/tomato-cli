# Changelog

按 commit 记录生成，分组依据是 commit 的 type。

## [0.1.1](https://github.com/StringKe/tomato-cli/compare/v0.1.0...v0.1.1) - 2026-09-16

### 新功能

- update：按安装方式选择更新命令（43da464）

### 文档

- readme：Homebrew、Scoop 安装与发版流程（62ffd82）

### 构建与发版

- deps：reqwest 升到 0.13，TLS 统一用 rustls ring，版本 0.1.1（ff78c4e）
- release：增加 Linux musl 静态包、Scoop manifest 同步与静默安装脚本（db52e01）

## [0.1.0](https://github.com/StringKe/tomato-cli/releases/tag/v0.1.0) - 2026-09-16

### 新功能

- app：番茄小说终端阅读器首个版本（ce6e008）

### 构建与发版

- release：x86_64 macOS 改用 macos-15-intel runner（05ccc81）
