# tomato-cli

番茄小说终端阅读器。按章在线阅读，不整本下载。

仓库：https://github.com/StringKe/tomato-cli

## 安装

macOS / Linux：

```sh
curl -fsSL https://raw.githubusercontent.com/StringKe/tomato-cli/main/scripts/install.sh | sh
```

Windows PowerShell：

```powershell
irm https://raw.githubusercontent.com/StringKe/tomato-cli/main/scripts/install.ps1 | iex
```

默认安装位置：

- macOS / Linux：`$HOME/.local/bin/tomato`
- Windows：`%LOCALAPPDATA%\tomato-cli\tomato.exe`

从源码：

```sh
cargo install --path .
```

## 使用

```sh
tomato              # 打开阅读器
tomato update       # 安装最新版本
tomato update --yes # 不询问直接更新
tomato check        # 只检查是否有新版本
tomato fontmap        # 查看字表状态
tomato fontmap update # 从官方字体重新生成字表
tomato cache          # 查看章节缓存大小
tomato cache clear    # 清空章节缓存
```

`tomato update` 会替换当前可执行文件。请把二进制装在用户可写目录。不要用 `sudo` 装到系统目录。

字表用于还原正文里的 PUA 字符。配置目录还没有缓存时，首次启动会从官方字体生成。设置最后一项「字表」可以重新生成。也可运行 `tomato fontmap update`。不必为更新字表发版 CLI。

## 界面

- 书架：按官方/本地分组文件夹浏览，每本书显示完整书名、作者、连载状态、章数、读到哪章和最新章；移动、新建、重命名、删除文件夹
- 搜索：按书名搜索，结果可翻页；也可输入 book_id 或书籍页链接（`https://fanqienovel.com/page/<book_id>`）直接打开
- 榜单：不登录也能按男频 / 女频和分类浏览榜单，到末尾自动加载下一页
- 目录：封面（终端支持图片协议时显示原图，否则用半块字符）、简介、章节列表、过滤、跳号、加入书架
- 阅读：预折行滚动、切章、目录跳转、进度保存；翻到底后再按空格进下一章，连按不会冲过头；终端窗口标题跟随当前书和章节。进度按正文位置记，改宽度、行距、整理模式或切换伪装后仍停在同一处
- 缓存：打开一章后自动提前拉取后面几章（设置里可选 关 / 3 / 5 / 10 章），读过的章节存在本地，再次打开不走网络，总量超过 64 MB 自动淘汰最旧的
- 首页（未登录）：扫码、粘贴 Cookie、搜索、演示
- 设置：主题默认 Auto。启动时用 OSC 10/11 询问终端配色。没有响应再读 `COLORFGBG`。不覆盖终端背景。可改夜间/日间等。可关闭封面下载。最后一项重新生成字表
  - 伪装布局：在设置里选皮肤（Claude Code、Codex、git log、man、Grok Build），阅读页按反引号切换伪装，非阅读页按反引号显示空闲会话。皮肤按各工具当前版本的真实界面还原：正文排成多轮对话，穿插用户输入、工具调用（Read / Bash / Search）、结果行、思考行和回合耗时，输入框、状态行、spinner 和终端标题都是对应工具的文案；git log 是 `git log -p` 的 diff，man 是 mandoc 手册页加 less 底行
  - 换行整理：自动 / 原样 / 合并。合并把「每句一行」的碎行章节并成正常长度的段落，半句一定接着并，句子收尾且够长才分段，对话在累积够长后另起一段；自动模式按本章统计（句子没收尾的行占比）决定用不用合并，设置行会显示本章实际落到的结果
  - 段首缩进（默认开）和段间空行（默认关）是两个独立开关
- 个人资料：账号、VIP、书架统计、登录退出、检查更新
- 登录：终端二维码或粘贴 Cookie

## 快捷键

未登录打开首页。登录支持扫码（抖音/番茄 App）或粘贴浏览器 Cookie。

主题默认 Auto。跟随终端配色，不覆盖终端背景。鼠标可用，操作以键盘为准。

全局：`Esc` 返回，`q` 退出，`?` 帮助，`s` 设置，`p` 资料

书架：`Enter` 继续阅读，`i` 目录，`[` `]` 切换文件夹，`n` 新建，`e` 重命名，`D` 删除文件夹，`m` 移动，`x` 移出，`o` 排序，`/` 搜索，`b` 榜单，`l` 登录，`d` 演示

榜单：`[` `]` 切换分类，`Tab` 切换男频 / 女频，`r` 刷新，`Enter` 打开

目录：`Enter` 阅读，`t` 过滤目录，`g` 跳号，`a` 加入书架

阅读：`j/k` 滚行，空格 / 退格翻页，`h/l` 或 `[` `]` 切章，`t` 目录，`g` 跳号，`r` 切换换行整理（自动 / 原样 / 合并），反引号切换伪装布局

伪装布局下只响应翻页、切章、`t`、`g`、`r` 和反引号，其余按键和鼠标右键都被吞掉；在书架等其他页面按反引号会显示一个空闲的假会话

设置：`h/l` 修改当前项

资料：`l` 登录，`o` 退出登录，`u` 检查更新

## 数据

配置目录按系统约定放置，资料页会显示实际路径：

- macOS：`~/Library/Application Support/com.stringke.tomato-cli/`
- Linux：`~/.config/tomato-cli/`
- Windows：`%APPDATA%\stringke\tomato-cli\config\`

目录里的 `state.json` 保存 cookie、本地书架、文件夹、阅读进度和设置。章节缓存在同一目录的 `chapters/` 下，封面缓存在 `covers/`，字表缓存也写在这里。

## 说明

正文和目录来自番茄小说网页版页面数据，书架和进度来自网页接口。PUA 字符用官方字体生成的字表还原。付费章节需在官方购买。

只供个人阅读，不要批量抓取或转载。
