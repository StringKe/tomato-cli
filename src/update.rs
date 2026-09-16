use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use self_update::backends::github::Update;
use self_update::cargo_crate_version;

const OWNER: &str = "StringKe";
const REPO: &str = "tomato-cli";

#[cfg(windows)]
const BIN: &str = "tomato.exe";
#[cfg(not(windows))]
const BIN: &str = "tomato";

/// 可执行文件是怎么装上的。包管理器装的版本由包管理器升级，自己替换二进制会让它记录的版本和实际对不上。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    Homebrew,
    Scoop,
    Binary,
}

impl InstallKind {
    pub fn detect() -> Self {
        std::env::current_exe().ok().and_then(|p| p.canonicalize().ok()).map(|p| Self::from_path(&p)).unwrap_or(Self::Binary)
    }

    /// Homebrew 装在 `<prefix>/Cellar/tomato-cli/<version>/bin/tomato`，Scoop 装在 `scoop/apps/tomato/<version>/tomato.exe`，两者的真实路径都能从目录名认出来。
    fn from_path(path: &Path) -> Self {
        let parts: Vec<String> = path.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
        let has = |name: &str| parts.iter().any(|p| p == name);
        if has("cellar") && has("tomato-cli") {
            Self::Homebrew
        } else if has("scoop") && has("apps") && has("tomato") {
            Self::Scoop
        } else {
            Self::Binary
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Homebrew => "Homebrew",
            Self::Scoop => "Scoop",
            Self::Binary => "二进制",
        }
    }

    /// 用户应当执行的升级命令。
    pub fn upgrade_command(self) -> &'static str {
        match self {
            Self::Homebrew => "brew upgrade StringKe/tap/tomato-cli",
            Self::Scoop => "scoop update tomato",
            Self::Binary => "tomato update",
        }
    }
}

fn builder() -> Result<self_update::backends::github::UpdateBuilder> {
    let mut b = Update::configure();
    b.repo_owner(OWNER).repo_name(REPO).bin_name(BIN).current_version(cargo_crate_version!());
    Ok(b)
}

/// 按安装方式更新：包管理器装的转交给包管理器，其余从 GitHub Releases 下载替换自身。
pub fn install(no_confirm: bool) -> Result<()> {
    let kind = InstallKind::detect();
    match kind {
        InstallKind::Homebrew => run_manager(kind, "brew", &["upgrade", "StringKe/tap/tomato-cli"]),
        InstallKind::Scoop => run_manager(kind, "cmd", &["/C", "scoop", "update", "tomato"]),
        InstallKind::Binary => {
            let status = builder()?.show_download_progress(true).no_confirm(no_confirm).build().context("构造更新器")?.update().context("执行更新")?;
            println!("当前版本 {}，更新结果：{status}", cargo_crate_version!());
            Ok(())
        }
    }
}

fn run_manager(kind: InstallKind, program: &str, args: &[&str]) -> Result<()> {
    println!("检测到 {} 安装，执行 {}", kind.label(), kind.upgrade_command());
    let status = Command::new(program).args(args).status().with_context(|| format!("运行 {}", kind.upgrade_command()))?;
    if !status.success() {
        bail!("{} 退出码 {}", kind.upgrade_command(), status.code().map(|c| c.to_string()).unwrap_or_else(|| "未知".into()));
    }
    Ok(())
}

pub fn check_newer() -> Result<Option<String>> {
    let updater = builder()?.show_download_progress(false).no_confirm(true).build().context("构造更新器")?;
    let newer = updater.is_update_available().context("查询 GitHub Releases")?;
    Ok(newer.map(|rel| rel.version().to_string()))
}

#[cfg(test)]
fn is_newer(remote: &str, current: &str) -> bool {
    let remote = remote.trim_start_matches('v');
    let current = current.trim_start_matches('v');
    match (semver::Version::parse(remote), semver::Version::parse(current)) {
        (Ok(r), Ok(c)) => r > c,
        _ => remote != current && !remote.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(is_newer("v1.0.0", "0.9.0"));
    }

    #[test]
    fn detects_install_kind_from_real_path() {
        assert_eq!(InstallKind::from_path(Path::new("/opt/homebrew/Cellar/tomato-cli/0.1.1/bin/tomato")), InstallKind::Homebrew);
        assert_eq!(InstallKind::from_path(Path::new("/home/linuxbrew/.linuxbrew/Cellar/tomato-cli/0.1.1/bin/tomato")), InstallKind::Homebrew);
        #[cfg(windows)]
        assert_eq!(InstallKind::from_path(Path::new(r"\\?\C:\Users\me\scoop\apps\tomato\0.1.1\tomato.exe")), InstallKind::Scoop);
        assert_eq!(InstallKind::from_path(Path::new("/Users/me/scoop/apps/tomato/current/tomato.exe")), InstallKind::Scoop);
        assert_eq!(InstallKind::from_path(Path::new("/Users/me/.local/bin/tomato")), InstallKind::Binary);
        assert_eq!(InstallKind::from_path(Path::new("/Users/me/.cargo/bin/tomato")), InstallKind::Binary);
        assert_eq!(InstallKind::from_path(Path::new("/Users/me/Code/tomato-cli/target/release/tomato")), InstallKind::Binary);
    }
}
