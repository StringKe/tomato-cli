//! 按键提示的数据源。文案只描述「键 + 动作」，排布交给 ui::layout::draw_hints，不在字符串里用空格对齐。

use crate::model::LayoutId;

use super::{App, Overlay, Screen};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hint {
    pub key: &'static str,
    pub label: &'static str,
}

pub const fn hint(key: &'static str, label: &'static str) -> Hint {
    Hint { key, label }
}

pub const ESC_BACK: Hint = hint("esc", "返回");
pub const ESC_CLOSE: Hint = hint("esc", "关闭");
pub const QUIT: Hint = hint("q", "退出");
pub const HELP: Hint = hint("?", "帮助");
pub const SETTINGS: Hint = hint("s", "设置");
pub const PROFILE: Hint = hint("p", "资料");
pub const COVER: Hint = hint("`", "伪装");

pub const HOME_ITEMS: [Hint; 7] = [hint("l", "扫码登录"), hint("c", "粘贴 Cookie"), hint("/", "搜索"), hint("b", "榜单"), hint("d", "演示"), hint("s", "设置"), hint("q", "退出")];
pub const LOGIN_ITEMS: [Hint; 3] = [hint("1", "扫码登录"), hint("2", "粘贴 Cookie"), hint("3", "刷新二维码")];
pub const PROFILE_ITEMS: [Hint; 3] = [hint("l", "登录"), hint("o", "退出登录"), hint("u", "检查更新")];

const HOME: &[Hint] = &[hint("enter", "确认"), hint("j/k", "移动"), HELP, QUIT];
const LOGIN: &[Hint] = &[hint("enter", "确认"), hint("c", "粘贴 Cookie"), hint("r", "刷新二维码"), ESC_BACK];
const SHELF: &[Hint] = &[hint("enter", "继续阅读"), hint("i", "目录"), hint("/", "搜索"), hint("b", "榜单"), hint("[ ]", "文件夹"), hint("n", "新建"), hint("e", "重命名"), hint("D", "删除文件夹"), hint("m", "移动"), hint("x", "移出"), hint("o", "排序"), hint("r", "刷新"), hint("d", "演示"), SETTINGS, PROFILE, HELP, QUIT];
const SHELF_GUEST: &[Hint] = &[hint("enter", "继续阅读"), hint("i", "目录"), hint("/", "搜索"), hint("b", "榜单"), hint("l", "登录"), hint("[ ]", "文件夹"), hint("n", "新建"), hint("m", "移动"), hint("x", "移出"), hint("o", "排序"), hint("d", "演示"), SETTINGS, HELP, QUIT];
const RANK: &[Hint] = &[hint("enter", "打开"), hint("[ ]", "分类"), hint("tab", "男频/女频"), hint("j/k", "移动"), hint("r", "刷新"), ESC_BACK];
const SEARCH_INPUT: &[Hint] = &[hint("enter", "搜索"), ESC_BACK];
const SEARCH_RESULTS: &[Hint] = &[hint("enter", "打开"), hint("上/下", "选择"), ESC_BACK];
/// 还有下一页时提示：光标停在最后一条再向下就加载。
const SEARCH_RESULTS_MORE: &[Hint] = &[hint("enter", "打开"), hint("上/下", "选择"), hint("末尾再向下", "加载更多"), ESC_BACK];
const BOOK: &[Hint] = &[hint("enter", "阅读"), hint("t", "目录"), hint("g", "跳号"), hint("a", "加入书架"), SETTINGS, ESC_BACK];
const READER: &[Hint] = &[hint("j/k", "滚行"), hint("空格", "翻页"), hint("h/l", "上一章/下一章"), hint("t", "目录"), hint("g", "跳号"), hint("r", "整理"), COVER, SETTINGS, ESC_BACK];
/// 本章最后一行已在屏幕内时的提示：空格的含义变成下一章。
const READER_END: &[Hint] = &[hint("空格", "下一章"), hint("h", "上一章"), hint("k", "回看"), hint("t", "目录"), hint("g", "跳号"), hint("r", "整理"), COVER, SETTINGS, ESC_BACK];

/// 伪装态的状态行提示是被模仿的 CLI 自己的文案，不是本程序的按键说明；pager 皮肤没有状态行提示。
const COVER_CLAUDE: &[Hint] = &[hint("?", "for shortcuts")];
const COVER_CODEX: &[Hint] = &[hint("?", "for shortcuts")];
const COVER_GROK: &[Hint] = &[hint("?", "for shortcuts")];
const COVER_PAGER: &[Hint] = &[];

fn cover_hints(layout: LayoutId) -> &'static [Hint] {
    match layout {
        LayoutId::Claude => COVER_CLAUDE,
        LayoutId::Codex => COVER_CODEX,
        LayoutId::Grok => COVER_GROK,
        LayoutId::GitLog | LayoutId::Man | LayoutId::Native => COVER_PAGER,
    }
}

const OVERLAY_HELP: &[Hint] = &[ESC_CLOSE];
const OVERLAY_TOC: &[Hint] = &[hint("输入", "过滤"), hint("enter", "跳转"), ESC_CLOSE];
const OVERLAY_JUMP: &[Hint] = &[hint("数字", "章节序号"), hint("enter", "跳转"), ESC_CLOSE];
const OVERLAY_FOLDER_PICK: &[Hint] = &[hint("enter", "移动到此"), ESC_CLOSE];
const OVERLAY_FOLDER_INPUT: &[Hint] = &[hint("enter", "确认"), ESC_CLOSE];
const OVERLAY_COOKIE: &[Hint] = &[hint("enter", "登录"), ESC_CLOSE];
const OVERLAY_SETTINGS: &[Hint] = &[hint("h/l", "修改"), hint("j/k", "移动"), ESC_CLOSE];
const OVERLAY_PROFILE: &[Hint] = &[hint("enter", "确认"), hint("j/k", "移动"), ESC_CLOSE];

/// 帮助浮层按区域列出完整快捷键。
pub const HELP_SECTIONS: &[(&str, &[Hint])] = &[
    ("全局", &[ESC_BACK, QUIT, HELP, SETTINGS, PROFILE, COVER]),
    ("首页", &[hint("enter", "确认"), hint("l", "扫码登录"), hint("c", "粘贴 Cookie"), hint("/", "搜索"), hint("b", "榜单"), hint("d", "演示")]),
    ("书架", &[hint("enter", "继续阅读"), hint("i", "目录"), hint("/", "搜索"), hint("b", "榜单"), hint("[ ]", "文件夹"), hint("n", "新建"), hint("e", "重命名"), hint("D", "删除文件夹"), hint("m", "移动"), hint("x", "移出"), hint("o", "排序"), hint("r", "刷新")]),
    ("榜单", &[hint("enter", "打开"), hint("[ ]", "分类"), hint("tab", "男频/女频"), hint("r", "刷新")]),
    ("目录", &[hint("enter", "阅读"), hint("t", "目录"), hint("g", "跳号"), hint("a", "加入书架")]),
    ("阅读", &[hint("j/k", "滚行"), hint("空格", "翻页"), hint("退格", "上一页"), hint("h/l", "上一章/下一章"), hint("[ ]", "上一章/下一章"), hint("t", "目录"), hint("/", "过滤目录"), hint("g", "跳号"), hint("r", "整理")]),
    ("设置", &[hint("h/l", "修改"), hint("j/k", "移动")]),
    ("资料", &[hint("l", "登录"), hint("o", "退出登录"), hint("u", "检查更新")]),
];

impl App {
    /// 当前上下文的按键提示。浮层优先于屏幕。
    pub(crate) fn hints(&self) -> &'static [Hint] {
        // 伪装态下浮层也不能露出中文提示，只返回皮肤自己的文案。
        if self.covered() {
            return cover_hints(self.state.settings.layout);
        }
        match self.overlay {
            Overlay::Help => OVERLAY_HELP,
            Overlay::Toc => OVERLAY_TOC,
            Overlay::Jump => OVERLAY_JUMP,
            Overlay::FolderPick => OVERLAY_FOLDER_PICK,
            Overlay::FolderInput => OVERLAY_FOLDER_INPUT,
            Overlay::Cookie => OVERLAY_COOKIE,
            Overlay::Settings => OVERLAY_SETTINGS,
            Overlay::Profile => OVERLAY_PROFILE,
            Overlay::None => match self.screen() {
                Screen::Home => HOME,
                Screen::Login => LOGIN,
                Screen::Shelf if self.state.user.is_some() => SHELF,
                Screen::Shelf => SHELF_GUEST,
                Screen::Search if self.search_results.is_empty() => SEARCH_INPUT,
                Screen::Search if self.search_has_more => SEARCH_RESULTS_MORE,
                Screen::Search => SEARCH_RESULTS,
                Screen::Rank => RANK,
                Screen::Book => BOOK,
                Screen::Reader if self.reader.as_ref().is_some_and(|r| r.view_height > 0 && r.at_end()) => READER_END,
                Screen::Reader => READER,
            },
        }
    }
}
