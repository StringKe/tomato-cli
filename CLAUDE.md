# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目

番茄小说终端阅读器，Rust 2024 edition，单 crate，二进制名 `tomato`。TUI 基于 ratatui 0.30（crossterm 通过 `ratatui::crossterm` 复用），HTTP 基于 reqwest + rustls，异步运行时 tokio。正文按章在线拉取，读过和预读的章节缓存在配置目录，不整本下载。用户可见文案全部是简体中文。

## 常用命令

```sh
cargo build                      # 调试构建
cargo run                        # 启动 TUI
cargo run -- check               # 子命令：检查新版本
cargo run -- fontmap             # 子命令：查看字表状态
cargo test                       # 全部单元测试（都是模块内 #[cfg(test)]，没有 tests/ 目录）
cargo test wraps_cjk_to_width    # 按名字跑单个测试
cargo test font::                # 跑某个模块下的测试
cargo clippy -- -D warnings      # CI 要求零 warning
cargo build --release            # release 开启 lto、codegen-units=1、strip
```

CI（`.github/workflows/ci.yml`）只跑 `cargo test` 和 `cargo clippy -- -D warnings`。提交前两者都要通过。

发版：推 `v*` tag 触发 `release.yml`，在 8 个原生 runner 上构建各自 target（macOS、Linux gnu、Linux musl、Windows 各 x86_64 与 aarch64；musl 用 `musl-tools` + `CC=musl-gcc` 静态链接）并上传到 GitHub Releases，`self_update` 按 `tomato-<target>.tar.gz|zip` 命名下载。构建完成后 `packages` job 用 `scripts/sync-scoop.sh` 重算 `bucket/tomato.json`（Scoop manifest，仓库本身就是 bucket）的哈希并以 github-actions 身份推一个 commit 到 main，所以发版后本地要先 `git pull --ff-only`。Homebrew formula 在独立仓库 https://github.com/StringKe/homebrew-tap ，那边的 `sync.yml` 每小时按最新 release 重新生成；仓库 secret `TAP_TOKEN`（对 tap 仓库有写权限的 PAT）存在时 `packages` job 会立即触发它。版本号只改 `Cargo.toml`，运行时用 `env!("CARGO_PKG_VERSION")` 读。发版用 `scripts/release.sh 0.2.0`：改版本号、`cargo check` 刷新 lock、`git-cliff --tag` 重写 `CHANGELOG.md`、提交 `chore(release): v0.2.0` 并打 tag，然后手动 `git push origin main v0.2.0`。changelog 分组规则在 `cliff.toml`（按 commit type 分「新功能 / 修复 / 性能 / 重构 / 文档 / 构建与发版 / 其他」，`chore(release)` 和 `chore(scoop)` 不进 changelog），GitHub Release 的正文由 `packages` job 用 `git cliff --latest` 生成，所以 commit 说明要写给用户看。

更新按安装方式分流：`update.rs::InstallKind::detect` 看 `current_exe` 的真实路径，`Cellar/tomato-cli` 判 Homebrew、`scoop/apps/tomato` 判 Scoop，其余是普通二进制。前两者 `tomato update` 转去执行 `brew upgrade StringKe/tap/tomato-cli` / `scoop update tomato`，只有普通二进制走 `self_update` 替换自身；`tomato check`、TUI 页脚的新版本提示都用 `upgrade_command()` 给对应命令。

TLS 全进程只用 rustls + ring：`reqwest` 开 `rustls-no-provider`，`self_update` 不开 `rustls` feature，`main.rs` 启动时 `install_default` ring provider。不要给任何依赖开 `aws-lc-rs`，它需要 cmake，musl 交叉构建会断。

## 架构

### 事件循环与后台任务

`app::run` 建 `App`，进入 `App::loop_ui`（`src/app/nav.rs`）。循环体每帧：`drain_worker` 收后台消息 -> `tick_auto_page` / `tick_cover_note` / `tick_cover_anim` -> `dirty` 为真才重绘（包在终端的同步刷新序列里，整屏一起换不撕裂） -> `event::poll(poll_timeout())` 读一个输入后把队列里已积压的事件（上限 `EVENT_BATCH`）一次处理完再进下一帧，按住键或触控板滚动时一次手势只画一帧。`poll_timeout` 在有后台任务（`busy`、`hydrating`、`cover_inflight`、`prefetching`、`organizing`）或定时动画（自动翻页、伪装提示）时是 `POLL_ACTIVE` 16ms，否则 `POLL_IDLE` 250ms，空闲时不再每秒醒 60 次；后台消息只在循环醒来时处理，所以新增会让用户等结果的任务要置 `busy` 或加进 `poll_timeout` 的判断。

终端后端是 `app/backend.rs::WideBackend`，包着 ratatui 的 crossterm 后端，只改了 `draw`：按字符显示宽度推算光标位置，汉字连着画时不再每个字发一条 MoveTo（上游按 x + 1 判断，整屏中文一帧输出是必要量的三倍，远端 ssh / tmux 滚动才感觉得到）。`run` 里 `init_terminal` 自己做 raw mode、备用屏和 panic hook，其余与 `ratatui::init` 相同；`ratatui::restore` 照常用。

后台网络请求不在主线程做。`src/app/workers.rs` 里每个 `spawn_*` 方法克隆 `Client` 和 `tx`，丢进 `self.rt.spawn`，完成后发 `WorkerMsg` 变体，`drain_worker` 里统一 match 并更新 `App` 状态。新增异步操作按同样模式：加 `WorkerMsg` 变体、加 `spawn_xxx`、在 `drain_worker` 里处理。

所有状态变更后要调 `self.mark()` 置脏，否则不重绘。需要落盘的调 `self.persist()`（同步 cookies 并写 `state.json`）。

### 屏幕与浮层

`Screen` 是栈（`App::screens`），`push` / `back` 管理导航；`Overlay` 是单值，压在当前屏幕上。按键分发在 `nav.rs::handle_key`：有 overlay 先给 overlay，否则按 `Screen` 分到 `keys.rs` / `keys_more.rs` 的 `*_key` 方法。鼠标在 `mouse.rs`，靠 ui 绘制时回写到 `App` 的命中区域（`list_area`、`menu_hits`、`folder_tabs`、`help_hit`、`profile_hit`、`overlay_area`）做点击判定，所以改布局时要同步更新这些 Rect。

`ui/` 只读 `App` 并绘制：`mod.rs` 分 header/body/footer 并分发；`stage.rs` 是首页和登录页（无 header 的居中布局），`workspace.rs` 是书架/搜索/榜单/目录/阅读，`overlay.rs` 是所有浮层，`layout.rs` 是共用的居中、弹窗、菜单、kv 行、按键提示等 helper，`disguise/` 是伪装态的整帧绘制（`mod.rs` 画帧，`skin.rs` 放皮肤常量和文案生成）。

### 按键提示与状态消息

按键提示是数据，不是字符串。`app/hints.rs` 定义 `Hint { key, label }`，`App::hints()` 按「浮层优先于屏幕」返回当前上下文的提示切片，`HELP_SECTIONS` 是帮助浮层的完整表。渲染统一走 `layout.rs` 的 `draw_hints`（一行内用 `Layout::horizontal` + `spacing` 排布，放不下的整条丢弃）和 `bottom_hints`（画在浮层底边）。页脚左侧是 `hints()`，右侧是 `status`、新版本提示和忙碌标记。

`App::status` 只放一次性消息（「找到 3 本」「已登录 xxx」「章节不存在」），切屏时由 `push` / `back` 清空。不要把按键说明写进 `status`、`login_msg` 或任何字符串，也不要用连续空格在字符串里排版；需要多段并排就用 `Layout` 分区。CLI 输出（`font::print_status`）同样用 `print_kv` 按显示宽度对齐。

### 登录页二维码

`stage.rs::plan_qr` 决定二维码怎么放：纠错级别 L、关闭 widget 静区改由自己补 2 模块留白、黑白固定配色不随主题。高度不够时按 `Chrome::LEVELS` 依次去掉方法菜单和字标，还放不下才报「需要 W×H 的窗口」。`qr_size` / `qr_scale` 是纯函数，改尺寸规则先跑它们的测试。

设置项是位置耦合的：`app/mod.rs` 的 `SETTINGS_LEN`、`keys_more.rs::nudge_setting` 的 match 分支、`App::settings_pairs` 的行顺序必须一致。增删设置项三处一起改。当前 16 行：主题、伪装布局、正文宽度、边距、行距、换行整理、段首缩进、段间空行、自动翻页、启动检查更新、书架排序、自动整理、弃读判定、书籍封面、提前缓存、字表。

### 输入处理

`nav.rs::fold_ime` 把全角字符折成 ASCII（中文输入法下 `？` `／` `【` 也能触发快捷键），非输入态时字母统一转小写。判定是否处于输入态看 `App::typing`。新增文本输入框要把对应 `Overlay` 加进 `typing`，并在 `handle_event` 的 `Event::Paste` 分支接收粘贴。

### 阅读器

`reader::WrapCache` 按终端宽度和 `WrapOpts`（行距、换行整理、段首缩进、段间空行、伪装戏）预折行，宽度和选项都不变时复用缓存，滚动只改 `ReaderSession::offset`。换了正文调 `cache.set_text`；改设置不用做任何事，下一帧 `prepare_wrap` 比较 `WrapOpts` 后自己重折并按正文位置回到原处（不要在设置变动时强制失效缓存，主题、排序这类改动会白白重折整章）。正文位置是 `WrapCache::anchors`（该行之前的非空白字符数，折行、整理、缩进、假对话行都只增删空白或非正文内容，所以在任何参数下指向同一处），`ReaderSession::prepare_wrap` 在每次重折后按它换算回行偏移，终端改宽也一样；`Progress.anchor` 用它保存进度，`line` 只给旧记录和远端记录兜底。窗口计算（prepare_wrap、回写 view_height、夹偏移、算百分比）抽在 `workspace.rs::reader_window`，原生阅读页和伪装帧共用。`draw_reader` 每帧回写 `ReaderSession::view_height`，翻页步长、进度百分比（最后一行可见即 100%）和 `at_end()` 都靠它。到底后再按一次向下翻页才切下一章，`actions.rs::reader_page` 用 `PAGE_DEBOUNCE` 把连按挡在本章末尾；到底时页脚提示切成 `hints.rs::READER_END`。`demo_book` 提供内置三章演示书（`book_id == "demo"`），不走网络，多处以此判断跳过远端同步。

`app::filtered_chapters` 返回 `(原始序号, &Chapter)`，键盘、鼠标和 ui 共用它算可见列表长度；展示格式在 `layout.rs::chapter_item`。

### 段落整理

`reflow.rs` 把一章正文切成 `Para`（Heading / Body / Verse，带 `block_start` 标记 `<p>` 边界还是 `<br>` 单换行），`reader.rs::wrap_lines` 再按段折行。三个互不相干的设置：`Reflow`（自动 / 原样 / 合并）决定哪些源行拼成一段；「段首缩进」（`para_indent`，默认开）和「段间空行」（`para_blank`，默认关）是渲染层的两个独立开关，用户明确要求缩进和空行分开，且「行距 0」时段落之间不能有空行。

合并（`reflow`）的目标是把「跟诗歌一样每句一行」的章节并成正常长度的段落，规则在 `breaks_before` / `join`：停在半句上（`，、：` 收尾）的行一定接着并；句子收尾后，累积到 `PARA_TARGET_CELLS` 才另起一段；本来就超过 `FRAG_MAX_CELLS` 的源段是真段落，两边都不并短行；累积够 `PARA_MIN_CELLS` 后遇到引号开头的行才让对话另起一段，否则「“嗯。”他转身走了」并在一起；`***` 一类没有文字的分隔符自己一段。两个都没标点的中文句子在 `<p>` 边界相接时补一个逗号，`<br>` 硬折行直接相连，英文补空格。标题和连续 4 行以上等宽短行（诗词）永远自己一段。`Reflow::Auto` 用 `detect` 统计非标题、非诗行、非冒号结尾行里句子没有收尾（逗号结尾或没有标点）的比例，≥ 25% 且 ≥ 8 行判碎行，设置行显示本章实际落到的结果（`keys_more.rs::reflow_label`）。阅读页 `r`（`actions.rs::reader_cycle_reflow`）循环三种模式并写 status；伪装态改在皮肤状态行放一条英文短提示（`App::cover_note`，`tick_cover_note` 到时撤掉）。

### 伪装布局

`Settings.layout`（`LayoutId`：Native / Claude / Codex / GitLog / Man / Grok）只影响外观，与 `ThemeId` 无关；`App::cover` 是运行时状态不落盘，`covered()` = cover 且 layout 非 Native。反引号是老板键（`nav.rs::handle_key` 在浮层分发之前拦截，`fold_ime` 把中文标点模式的 `·` 折成反引号），`keys_more.rs::toggle_cover` 翻转 cover 并关掉 Toc / Jump 以外的浮层。进入阅读页第一帧就伪装（`apply_chapter` 按 layout 置 cover），切章不改 cover。

伪装态下 `ui/mod.rs::draw` 直接交给 `ui/disguise/mod.rs::draw_cover` 整帧绘制，不画 header / footer / status / 浮层，配色固定 `palette(ThemeId::Auto)` 不涂整帧背景；非阅读屏幕画成空闲会话（Codex 的会话头贴顶，Grok 有固定 header，其余只有底栏）。皮肤文案、字形、颜色、行数全部在 `disguise/skin.rs`（常量、`skin()` 表、文案生成函数），值来自对本机 Claude Code 2.1.273 / Codex 0.154.0 / Grok Build 1.0.30 二进制与源码、macOS git + less + mandoc 的取证，被模仿的 CLI 改版时只改这个文件。顶栏只在 `offset == 0` 时画（滚动后自然「滚出屏幕」），Grok 的 header 行例外（`Skin.top_fixed`）；glyph 单独画在固定列的 Rect 里避免 Ambiguous 宽度错位。需要底色的元素（Claude 用户消息带、Grok 用户输入带）按 `theme::terminal_is_dark` 选深浅色，探测失败时不铺底色。

伪装是内容层的，不只是外框：`WrapOpts.deco`（`reader::Deco`，每种皮肤一个变体）让折行结果带上每行的身份（`LineKind`），伪装态强制不缩进、段间空行，并按皮肤套戏。agent 皮肤在章首和每隔 `CHAT_TURN_EVERY` 段插一轮对话（`reader::turn_block` 定行序，皮肤在绘制时按轮次序号生成文案）：Claude 是 `✻ Churned for 36s` / `❯ 继续` / `✻ Thinking…` / `⏺ Read(docs/ch-0012.md)` + `⎿  Read 214 lines`，Codex 是 `› 继续` / `• Explored` + `└ Read ...` / dim italic 推理摘要，Grok 是 `Worked for 4.2s` / 带底色带的 `❯ 继续` + 时间戳 / `◆ Read docs/ch-0012.md (1-214)` / `◆ Thought for 2.0s`。回复首行 `ReplyStart` 画回复 glyph（Grok 没有），章节标题 `Heading` 画成各自的 markdown 一级标题；工具调用轮换 Read / Bash / Search，思考行隔轮出现。git log 皮肤画成 `git log -p`：commit 头（`HEAD` 青、`main` 绿）之后是新文件 diff 头六行，每一行正文前一列 `+` 且整行绿色。man 皮肤是 mandoc 的样子：标题行、NAME、DESCRIPTION、5 列缩进，`Deco::Man` 在正文末尾追加空行和页脚（`LineKind::Footer`），底行与 git log 一样是 less 的 `:` / `(END)`。插入的行都不计入 `WrapCache::anchors`（`LineKind::is_content`），所以伪装和原生之间切换、进度保存都不受影响。busy 时 agent 皮肤画工作行（Claude 往返 spinner + 随机动词 + 耗时 + token，Codex `• Working (3s • esc to interrupt)`，Grok 盲文 spinner + `Responding…` + 右侧 `12s ⇣1.2k [stop]`），`actions.rs::tick_cover_anim` 每 `ANIM_TICK` 置脏换帧并记 `App::cover_work_since`。目录和跳号在伪装帧内联绘制（章节显示为 `docs/ch-0012.md`，`toc_items` 在伪装态按序号前缀过滤，`cover_toc_accepts` 只放行数字和导航键；pager 皮肤过滤用 `/`、跳号用 `:`），`list_area` / `overlay_area` 照常回写供鼠标用。`cover_key` 是白名单：阅读页只放行翻页、切章、`t`、`g`、`r`、Home / End，其他屏幕全吞；`mouse.rs` 在伪装态只保留阅读页的滚轮和左键，右键只关目录。`hints()` 在伪装态返回皮肤自己的提示（`cover_hints`），终端标题由 `window_title` 走 `cover_window_title`。

### 章节缓存与预读

`cache.rs` 把 `ChapterBody` 按 `item_id` 存成配置目录 `chapters/{item_id}.json`，总量超过 `CHAPTER_CAP`（64 MB）时按写入时间删最旧的；演示书不缓存。封面原图存 `covers/`（`COVER_CAP` 32 MB），整理书架用的精简目录 `cache::Toc`（目录顺序的 `(item_id, 发布时间秒)`）按 `book_id` 存 `tocs/{book_id}.json`（`TOC_CAP` 16 MB），淘汰方式相同。`spawn_chapter` 先查演示书和缓存，命中就不走网络；要的章正在预读时记进 `App::pending_open` 等预读落盘直接用，不重复发同一个整页请求（`back` 离开阅读页时清掉，防止迟到的预读把用户拉回去）。`schedule_prefetch`（`workers.rs`）在每次 `apply_chapter` 后从当前章往后取「提前缓存」设置的章数，跳过已缓存、`App::prefetching` 里在拉的和 `App::prefetch_failed` 里失败过的（否则登录墙后的章会被无限重试；每次切章、换书、登录后清空），一次只跑一个顺序任务，每章结束发 `WorkerMsg::Prefetched { item_id, ok }` 再续排。预读和书架补全用 `Client::background()` 的克隆：`throttle` 里后台请求看到有前台请求在等就让路，用户切章不排在预读后面，服务端看到的最小间隔不变。CLI `tomato cache` 分别显示章节、封面和目录缓存，`tomato cache clear`（`cache::clear`）把三个目录一起删掉。

### 书架与搜索结果的卡片列表

书架和搜索结果是三行一项的卡片（`workspace.rs::card`：完整标题、两行元数据），项与项之间空一行，由 `draw_cards` 自己排版而不是用 `List`，因为高亮只能盖住卡片本身。卡片按下标惰性构造（`build` 闭包），只为画得下的几张拼文本；封面表由 `take_card_view` 从 `App` 里 take 出来画完放回，闭包才能同时借用 `App`。步距常量在 `app/mod.rs`（`CARD_H` / `CARD_GAP` / `CARD_STRIDE`），滚动偏移存 `ListState.offset`，鼠标命中用 `card_index_at` 按 `CARD_STRIDE` 换算。元数据行统一用 `meta_line` 拼：字段之间只用空白，不加符号，也不给字段单独上色，因为 Auto 主题的高亮是 REVERSED，任何单独的前景色在高亮行里都会变成一块底色。

### 书架整理

判定规则在 `organize.rs`，全是纯函数，测试在同文件。拉数据、移动和同步在 `app/shelf_org.rs`。

`measure` 用目录 `(item_id, 发布时间秒)`、服务端已读列表和本地阅读记录算出 `model::ReadStats`。判读完看读到的最远一章，不看已读章数，因为跳着读、换设备读都会让已读列表偏少。阅读时间 `read_ms` 取书架最后操作时间（`ShelfItem::operated_ms`，来自书架接口的 `last_operate_time`）、本地记录时间和最远一章发布时间的最大值。最远一章之后的章按发布时间是否晚于 `read_ms` 分成跳过的 `skipped` 和读完后才出的 `new_after`。`last_item` 由调用方填书架上的 `last_read_item_id`。

`classify` 按顺序判定，命中即停，目录为空时不判定：
- 已读不超过 1 章：没开始，去默认。
- `skipped` 和 `new_after` 都不超过容差：读完，去 `read_ms` 那年的文件夹（`year_of`，按北京时间）；连载中（`creation_status == 1`）的书算在读，留在默认。
- `skipped` 不超过容差、`new_after` 超过：有更新，去 `UPDATED`「有更新」。
- 其余是没读完：距 `read_ms` 不超过 `abandon_days` 天算在读，去默认；超过算弃读，去 `ABANDONED`「弃读」。

容差 `tolerance` 是 3 章和总章数 1% 里较大的那个，照顾完本感言和番外。书架卡片上的「新 N 章」用 `new_chapters`。

`plan` 对整个书架出 `Move { book_id, title, from, to, new }` 清单，分组名空串是默认。只看 `is_managed` 的文件夹：默认、有更新、弃读、年份、「完毕」（手机上用户自建的读完分组，里面的书按读完年份拆开）。用户自建的其他文件夹和演示书不动。`pinned` 的书在当前状态等于 `pinned_kind` 时不动，状态变了就解除标记交回规则。`folder_order` 定书架标签顺序：默认、有更新、年份从新到旧、弃读、其他文件夹按名字。

一键整理：书架按 `z` 进 `start_organize`，后台逐本拉 `Client::read_items` 和目录。目录先查 `cache::toc_get`，缓存的 `last_item()` 和书架的 `last_chapter_item_id` 一致才用，否则拉 `Client::directory` 再 `cache::toc_put`。每本书发一条 `WorkerMsg::OrganizeStat` 写回 `read_stats` 并推进 `organize_progress`，全部结束发 `WorkerMsg::OrganizeDone`，用 `plan` 算出 `organize_plan` 并打开 `Overlay::Organize` 预览。`Enter` 走 `apply_organize`：`store::apply_moves` 落到本地，`State.organized_ms` 为 0 时记下当前时间，然后 `persist` 和 `push_groups`。`Esc` 走 `cancel_organize`。`apply_moves` 不把目标写进 `State.folders`，整理出来的文件夹空了就不再显示。

自动整理的条件是 `Settings.auto_organize` 为真（默认开）且 `State.organized_ms > 0`。第二个条件防止升级后第一次刷新就大批移动书，用户至少确认过一次一键整理才生效。每次刷新书架后，只给阅读情况可能变了的书重新拉数据：
- 没有 `read_stats`。
- `read_stats.toc_last` 和书架的 `last_chapter_item_id` 不同，说明又更新了。
- `read_stats.last_item` 和书架的 `last_read_item_id` 不同，说明又读过了。
- `operated_ms` 或本地阅读记录的 `updated_ms` 晚于 `read_stats.fetched_ms`。
- `read_stats` 超过 `STATS_TTL_MS`（7 天），服务端已读列表可能在书架字段不变时变化。

没有要重拉的书时也用已有的 `read_stats` 跑一次 `plan`，因为弃读是随时间变的。自动整理的结果直接 `apply_moves`、`persist`、`push_groups`，不弹预览；整理预览开着时不跑，避免同一批移动发两次。阅读页读到最后一章末尾再按下一章时，`archive_finished` 当场对这本书做一次自动整理，读完的书马上归档，同样要满足上面两个条件；`read_stats.last_item` 已经是这一章的不再重拉，自动翻页停在全书末尾时不会每个周期都请求。

手动优先：「移动到」浮层确认走 `move_selected_to`，依次 `store::move_to_folder`、`organize::pin`、`persist`、`push_groups`。`pin` 记下当时的状态到 `pinned_kind`。移动时还没有 `read_stats` 的书，`pinned_kind` 留空，由下一次 `plan` 以那次的状态为准。

分组同步：`push_groups` 把 `(book_id, 分组名)` 交给 `Client::move_groups`，按 `MOVE_BATCH` 分批发，分组名空串是默认。一键整理、自动整理、手动移动、`store::rename_folder`、`store::delete_folder`（书移回默认）都走它，后两个返回受影响的 `book_id`。只在登录状态下发，只发 `App::remote_ids` 里的书（上次远端书架出现过的 `book_id`，为空时不过滤），演示书不发。服务端按分组名移动，没有就新建，分组清空后仍保留。接口的实测结论见 `api/mod.rs` 模块文档。

### 封面

`ratatui-image` + `image` crate。`App::picker` 在进入备用屏之后由 `app::probe_picker` 探测终端图片协议（查询超时 `PICKER_QUERY_TIMEOUT` 500ms，库默认 2 秒会让不回应的终端首帧空等）；tmux / screen 里直接用构造 `App` 时那个半块 Picker，因为探测线程在没有回应时会阻塞在 stdin 上吞掉用户的第一个按键，而且 `Picker::halfblocks()` 在 tmux 里每次都要 spawn 一次 `tmux set`。封面按 `book_id` 缓存在 `App::covers`（`StatefulProtocol`），`spawn_cover` 下载后在 `spawn_blocking` 里解码；失败的进 `App::cover_failed` 本次会话不再自动重试（否则离线时可见卡片每帧都重新发请求），刷新书架或重开「书籍封面」设置时清空。设置项「书籍封面」关掉时清空缓存。书籍页只在 `thumb_url` 非空且宽度足够时预留 18×12 的封面区。

浮层盖住封面图片后关闭或挪动时要整屏重画。iTerm2 / Kitty / Sixel 协议的图片只在锚点格（左上角，Kitty 是每行行首）输出转义序列，其余格标记为 Skip。浮层关闭或挪动后，差分刷新不重写这些格，锚点格没变图片也不重发，浮层文字就留在图片上。处理方式：`ui::draw` 开头清空 `App::image_rects`，画图片时追加区域；`loop_ui` 画完一帧后，如果上一帧的浮层区域 `App::shown_overlay` 和这一帧的图片区域相交且浮层区域变了，就在同一个同步刷新块里 `terminal.clear()` 再画一次。

### API 与鉴权

`api::Client` 可 `Clone`，cookies 放 `Arc<Mutex<BTreeMap>>` 共享，每次响应自动收 `Set-Cookie`。所有请求走 `Client::request`，带固定 UA / Referer / Origin。接口路径常量集中在 `api/mod.rs` 顶部，JSON 解析 helper 在 `api/parse.rs`（`json_str` 接受多个候选 key，因为番茄接口驼峰和下划线混用；数字常以字符串下发，用 `json_u64` / `json_i64`）。

数据来源分两类，`api/mod.rs` 的模块文档列出了实测可用的路径：
- 网页 SSR：书籍页 `/page/{book_id}` 和阅读页 `/reader/{item_id}` 的 `window.__INITIAL_STATE__`，由 `api/ssr.rs::initial_state` 取出。`book_page` 一次拿到元数据和目录，`chapter` 先走阅读页再用 `/api/reader/full` 兜底，`reader_meta` 给书架补作者、最新章标题和连载状态。
- JSON 接口：书架列表（只有 id、分组和 `last_operate_time`）+ `POST /api/bookshelf/multidetail`（书名、封面、章数、最近阅读章）、目录（每章带 `firstPassTime`，存进 `Chapter::published`）、用户、进度、已读章节 `read_item_list`（`Client::read_items`）、移动分组 `bookshelf/update`（`Client::move_groups`）。
- 不登录不签名的接口：搜索走 `novel.snssdk.com` 的 `search/v1`（全字段明文，`has_more` + `offset` 翻页，备用域名 `api-lf.fanqiesdk.com`）；榜单走 `/api/rank/category/list`，分类列表来自 `/rank` 页面 SSR，男频分类只在 `gender=1` 下有数据、女频只在 `gender=0` 下有数据（`app/rank.rs`、`Screen::Rank`，书架按 `b` 进入）。
- 需要 `a_bogus` 签名的接口：网页搜索 `/api/author/search/search_book/v1`（只在 snssdk 两个域名都失败时兜底，书名带另一套字体的 PUA，现有字表会解出错字）和章节 `/api/reader/full`（阅读页 SSR 失败时兜底）。签名由 `api/abogus.rs` 生成（SM3 + 改造 RC4 的清洁室移植，测试与 Python 蓝本逐字符比对），走 `Client::signed_get`。签名绑定 a_bogus 之前的整段 query 串和 `UA` 常量，不绑定时间戳、cookie、msToken；服务端拒签时返回 HTTP 200 空 body，`request` 把它报成 `EMPTY_BODY`。UA 常量写死 mac Chrome，Linux / curl 的 UA 会被拒，不要按平台生成。
- 登录墙：目录里前 10 章之外几乎都是 `isChapterLock=true`，未登录时阅读页 SSR 只给几百字节的截断预览，`parse_chapter` 把它读进 `ChapterBody::locked`，`chapter` 据此报 `LOGIN_WALL`。`needPay` 才是付费墙。所有请求共用 `REQUEST_GAP` 节流；书籍页对出版付费书返回 404。
- 用户也可以直接输入 book_id 或书籍页链接打开书（`workers.rs::book_id_from_input`）。

番茄的 `creation_status` / `creationStatus`：0 已完结，1 连载中（用书籍页的「已完结」标签比对确认过）。模型里存 `Option<i64>`，接口没给时显示空。

书架条目字段来自多个来源，合并规则统一用 `ShelfItem::fill_missing_from`（只补空位不覆盖）：`store::upsert_shelf` 新值优先、`store::merge_remote_shelf` 远端优先。分组是例外：`merge_remote_shelf` 直接用远端的 `group_name`，空串也不从本地补，否则在手机上移回默认的书在本地还留在原文件夹。`spawn_hydrate_shelf` 在书架刷新后按 `needs_meta()` 逐本请求阅读页补齐，逐条发 `WorkerMsg::ShelfMeta`，结束发 `ShelfMetaDone` 落盘。

登录两条路：`auth.rs` 的扫码流程（`start_qr` -> `poll_qr` 轮询 180 秒 -> `finalize` 访问 redirect 建立会话），或直接粘贴 Cookie（`Client::login_with_cookie`）。登录成功与否都以 `user_info` 返回非空为准。

### 字表（PUA 还原）

番茄正文用自定义字体把常用字映射到 PUA 区。`font/` 的思路：官方 WOFF2 的 cmap + CFF charset 给出 `PUA 码点 -> 虚拟 gid`，内置 `font/fontmap.json`（`include_str!`）只当 `虚拟 gid -> 汉字` 字典。生成结果缓存到配置目录 `fontmap.json` 和 `font-url.txt`，运行时先读缓存，没有才联网。`discover.rs` 从公开章节页的 `@font-face` 发现字体地址，明确屏蔽 github / jsdelivr 等第三方源，不要写死字体 URL。`html.rs::html_to_text` 在 HTML 转文本后调 `decode_pua`。

### 持久化

`store.rs` 的 `State` 序列化为配置目录下的 `state.json`（`directories::ProjectDirs::from("com", "stringke", "tomato-cli").config_dir()`：macOS 是 `~/Library/Application Support/com.stringke.tomato-cli/`，Linux 是 `~/.config/tomato-cli/`），用临时文件 + `persist` 原子写。新增字段要加 `#[serde(default)]` 保证旧文件可读；需要迁移用 `config_rev` 递增。`Settings` 里的可选值用固定数组循环（`cycle_*`）。

文件夹名空串视为 `UNGROUPED`（显示为「默认」）。刷新书架时 `store::merge_remote_shelf` 用远端书架替换本地书架：分组以远端为准，其他字段只补空位，远端没有的书保留为本地条目。书架顶部标签是 `store::shelf_tabs`：`store::all_folders` 按 `organize::folder_order` 排成默认、有更新、年份从新到旧、弃读、其他文件夹，最后加 `ALL_BOOKS`「全部」；「默认」只放没进文件夹的书。`App::folder_idx` 指这个标签列表，「移动到」浮层用单独的 `folder_pick_idx` 指 `store::all_folders`（不含「全部」），`current_folder()` 只在真实文件夹上返回名字，重命名和删除都走它。`ShelfItem::added_ms` 来自书架接口的 `add_shelf_time`（本地加入时由 `upsert_shelf` 填当前时间），「最近」排序取它和阅读进度时间的较大值，手机上刚加的书才会排在最前。`State.organized_ms` 记第一次确认一键整理的时间，`ShelfItem` 的 `operated_ms` / `read_stats` / `pinned` / `pinned_kind` 是整理用的字段（见「书架整理」）。

### 主题

`theme.rs` 的 `ThemeId::Auto` 不覆盖终端背景（`Palette::inherit`），高亮用 `REVERSED` 而不是固定色。深浅色探测在进入 TUI 前由 `probe_terminal` 完成（OSC 10/11，超时 250ms，回退 `COLORFGBG`），结果放 `OnceLock`。

## 代码约定

- 不按列宽折行，一条链式调用或 `format!` 可以很长，与现有文件保持一致。
- 错误用 `anyhow`，给用户看的错误文案是中文，后台任务把 `Err` 转成 `String` 塞进 `App::status`。
- 注释只写 WHY。
- 测试与被测代码同文件（`#[cfg(test)] mod tests`），新增纯函数的测试沿用这个位置。
