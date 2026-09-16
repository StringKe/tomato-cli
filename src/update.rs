use anyhow::{Context, Result};
use self_update::backends::github::Update;
use self_update::cargo_crate_version;

const OWNER: &str = "StringKe";
const REPO: &str = "tomato-cli";

#[cfg(windows)]
const BIN: &str = "tomato.exe";
#[cfg(not(windows))]
const BIN: &str = "tomato";

fn builder() -> Result<self_update::backends::github::UpdateBuilder> {
    let mut b = Update::configure();
    b.repo_owner(OWNER).repo_name(REPO).bin_name(BIN).current_version(cargo_crate_version!());
    Ok(b)
}

pub fn install(no_confirm: bool) -> Result<()> {
    let status = builder()?.show_download_progress(true).no_confirm(no_confirm).build().context("构造更新器")?.update().context("执行更新")?;
    println!("当前版本 {}，更新结果：{status}", cargo_crate_version!());
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
}
