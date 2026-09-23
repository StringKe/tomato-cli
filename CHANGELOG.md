# Changelog

按 commit 记录生成，分组依据是 commit 的 type。

## [0.2.0](https://github.com/StringKe/tomato-cli/compare/v0.1.2...v0.2.0) - 2026-09-23

### 新功能

- shelf：一键整理书架并同步分组到番茄，标签新增「全部」，新加入的书排在最前（b22c6bc）

### 修复

- release：中文标点前的变量名加花括号（40a4e63）
- ui：关闭浮层后封面图片上不再残留浮层文字（8d958e2）

### 文档

- claude：记录书架整理、分组同步与封面重画规则（1f13266）

## [0.1.2](https://github.com/StringKe/tomato-cli/compare/v0.1.1...v0.1.2) - 2026-09-16

### 性能

- ui：卡片列表只构造画得下的几张（c9b52e6）
- api：前台请求优先于后台节流队列，建连限时 8 秒（ea80f83）
- app：宽字符感知后端、空闲降频轮询、预读与封面失败不再重试、改设置不重折（a30a5c3）

### 文档

- claude：记录事件循环、终端后端与预读的新规则（0731590）

### 构建与发版

- changelog：接入 git-cliff 生成 CHANGELOG 与 Release 说明（614e2d8）

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
