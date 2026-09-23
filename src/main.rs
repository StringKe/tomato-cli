mod api;
mod app;
mod auth;
mod cache;
mod font;
mod html;
mod model;
mod organize;
mod reader;
mod reflow;
mod store;
mod theme;
mod ui;
mod update;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tomato", version, about = "番茄小说终端阅读器")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// 更新到最新版本：Homebrew / Scoop 安装的交给包管理器，其余从 GitHub Releases 下载替换
    Update {
        /// 不询问，直接安装
        #[arg(long)]
        yes: bool,
    },
    /// 查询是否有新版本
    Check,
    /// 查看或从官方字体生成 PUA 字表
    Fontmap {
        #[command(subcommand)]
        action: Option<FontmapAction>,
    },
    /// 查看或清空章节缓存
    Cache {
        #[command(subcommand)]
        action: Option<CacheAction>,
    },
}

#[derive(Subcommand)]
enum FontmapAction {
    /// 拉取官方 WOFF2 并重新生成字表
    Update,
}

#[derive(Subcommand)]
enum CacheAction {
    /// 删除全部已缓存的章节、封面和目录
    Clear,
}

fn main() -> Result<()> {
    // 整个进程只带 ring 一套 TLS 实现，reqwest 与 self_update 共用，避免 aws-lc 的 cmake 依赖，musl 静态构建才不需要额外工具链
    rustls::crypto::ring::default_provider().install_default().ok();
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Update { yes }) => update::install(yes),
        Some(Command::Check) => match update::check_newer()? {
            Some(v) => {
                println!("有新版本 {v}，当前 {}。运行 {} 安装。", env!("CARGO_PKG_VERSION"), update::InstallKind::detect().upgrade_command());
                Ok(())
            }
            None => {
                println!("已是最新版本 {}", env!("CARGO_PKG_VERSION"));
                Ok(())
            }
        },
        Some(Command::Fontmap { action }) => match action {
            Some(FontmapAction::Update) => font::print_update(),
            None => font::print_status(),
        },
        Some(Command::Cache { action }) => match action {
            Some(CacheAction::Clear) => {
                cache::clear()?;
                println!("章节、封面和目录缓存已清空");
                Ok(())
            }
            None => {
                let chapters = cache::dir()?;
                let covers = cache::cover_dir()?;
                let tocs = cache::toc_dir()?;
                let (n, bytes) = cache::stats(&chapters);
                let (m, cover_bytes) = cache::stats(&covers);
                let (k, toc_bytes) = cache::stats(&tocs);
                println!("章节缓存：{n} 章，{}，位于 {}", cache::size_label(bytes), chapters.display());
                println!("封面缓存：{m} 张，{}，位于 {}", cache::size_label(cover_bytes), covers.display());
                println!("目录缓存：{k} 本，{}，位于 {}", cache::size_label(toc_bytes), tocs.display());
                Ok(())
            }
        },
        None => app::run(),
    }
}

