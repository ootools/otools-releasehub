use super::models::{Project, ReleaseAsset};
use std::cmp::Ordering;

pub fn version(value: &str) -> Option<semver::Version> {
    let value = value.trim().trim_start_matches(['v', 'V']);
    if value.len() > 200 {
        return None;
    }
    if let Ok(v) = semver::Version::parse(value) {
        return Some(v);
    }
    let boundary = value.find(['-', '+']).unwrap_or(value.len());
    let (base, suffix) = value.split_at(boundary);
    let mut pieces: Vec<&str> = base.split('.').collect();
    if pieces.is_empty()
        || pieces.len() > 3
        || pieces
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    while pieces.len() < 3 {
        pieces.push("0");
    }
    // Release tags often contain padded numeric components; these are numbers, not lexical strings.
    let numbers: Option<Vec<String>> = pieces
        .iter()
        .map(|p| p.parse::<u64>().ok().map(|n| n.to_string()))
        .collect();
    semver::Version::parse(&format!("{}{}", numbers?.join("."), suffix)).ok()
}
pub fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    Some(version(a)?.cmp_precedence(&version(b)?))
}
fn token(name: &str, choices: &[&str]) -> bool {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|p| choices.contains(&p))
}
pub fn wildcard(pattern: &str, name: &str) -> bool {
    let p = pattern.to_ascii_lowercase();
    let s = name.to_ascii_lowercase();
    if !p.contains(['*', '?']) {
        return s.contains(&p);
    }
    let (p, s) = (p.as_bytes(), s.as_bytes());
    let (mut i, mut j) = (0, 0);
    let (mut star, mut retry) = (None, 0);
    while j < s.len() {
        if i < p.len() && (p[i] == b'?' || p[i] == s[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == b'*' {
            star = Some(i);
            i += 1;
            retry = j;
        } else if let Some(k) = star {
            retry += 1;
            j = retry;
            i = k + 1;
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == b'*' {
        i += 1;
    }
    i == p.len()
}
fn classify_windows_x64(asset: &mut ReleaseAsset, preference: &str, pattern: Option<&str>) {
    let name = asset.name.to_ascii_lowercase();
    let exe = name.ends_with(".exe") || name.ends_with(".msi");
    let zip = name.ends_with(".zip");
    asset.kind = if exe {
        "installer"
    } else if zip {
        "portable"
    } else {
        "other"
    }
    .into();
    asset.compatible = false;
    asset.score = 0;
    if !exe && !zip {
        asset.reason = "不是支持的 Windows 安装器或 ZIP".into();
        return;
    }
    if token(
        &name,
        &[
            "arm", "arm64", "aarch64", "armv7", "armv8", "mac", "macos", "osx", "darwin", "linux",
            "ubuntu", "android", "ios", "i386", "i686", "win32", "32bit",
        ],
    ) || (token(&name, &["x86"]) && !name.contains("x86_64") && !name.contains("x86-64"))
        || name.contains("windows-arm")
        || name.contains("windows_arm")
    {
        asset.reason = "不适用于 Windows x64".into();
        return;
    }
    if token(
        &name,
        &[
            "debug",
            "symbols",
            "pdb",
            "source",
            "sources",
            "checksum",
            "checksums",
            "sha256",
            "sha512",
            "sdk",
        ],
    ) || name.contains("-dbg")
    {
        asset.reason = "调试、源码或校验附属文件".into();
        return;
    }
    if asset.size == 0 || asset.size > super::network::MAX_DOWNLOAD_BYTES {
        asset.reason = "文件为空或超过下载大小限制".into();
        return;
    }
    if super::network::trusted_download_url(&asset.url).is_err() {
        asset.reason = "下载地址不在可信 GitHub 域名内".into();
        return;
    }
    if let Some(p) = pattern {
        if !wildcard(p, &name) {
            asset.reason = "不符合指定文件名规则".into();
            return;
        }
    }
    asset.compatible = true;
    let windows = token(&name, &["windows", "win", "win64", "win10", "win11"])
        || name.contains("windows-")
        || name.contains("windows_");
    let x64 = token(&name, &["x64", "amd64", "win64", "64bit"])
        || name.contains("x86_64")
        || name.contains("x86-64");
    asset.score = 20 + if windows { 35 } else { 0 } + if x64 { 25 } else { 0 };
    if exe {
        asset.score += 15;
    }
    if name.contains("portable") || name.contains("freeui") {
        asset.score += 10;
    }
    if (preference == "portable" && zip) || (preference == "installer" && exe) {
        asset.score += 100;
    }
    if pattern.is_some() {
        asset.score += 50;
    }
    asset.reason = if windows && x64 {
        "适配 Windows x64"
    } else if exe {
        "Windows 安装器，架构未明确标注"
    } else if x64 {
        "x64 ZIP，平台未明确标注"
    } else {
        "ZIP 未标平台，需确认内容"
    }
    .into();
}
pub fn classify(asset: &mut ReleaseAsset, preference: &str, pattern: Option<&str>) {
    classify_for_platform(
        asset,
        preference,
        pattern,
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
}

pub fn classify_for_platform(
    asset: &mut ReleaseAsset,
    preference: &str,
    pattern: Option<&str>,
    os: &str,
    arch: &str,
) {
    if os == "windows" && arch == "x86_64" {
        classify_windows_x64(asset, preference, pattern);
        return;
    }
    let name = asset.name.to_ascii_lowercase();
    let windows = token(
        &name,
        &["windows", "win", "win64", "win10", "win11", "win32"],
    );
    let mac = token(&name, &["mac", "macos", "osx", "darwin"]);
    let linux = token(&name, &["linux", "ubuntu", "debian", "fedora"]);
    let arm = token(&name, &["arm64", "aarch64"]);
    let x64 = token(&name, &["x64", "amd64", "win64", "64bit"])
        || name.contains("x86_64")
        || name.contains("x86-64");
    let universal = token(&name, &["universal", "all"]);
    let installer = match os {
        "windows" => name.ends_with(".exe") || name.ends_with(".msi"),
        "macos" => name.ends_with(".dmg") || name.ends_with(".pkg"),
        "linux" => name.ends_with(".deb") || name.ends_with(".rpm"),
        _ => false,
    };
    let portable = name.ends_with(".zip")
        || name.ends_with(".tar.gz")
        || name.ends_with(".tgz")
        || (os == "linux" && name.ends_with(".appimage"));
    asset.kind = if installer {
        "installer"
    } else if portable {
        "portable"
    } else {
        "other"
    }
    .into();
    asset.compatible = false;
    asset.score = 0;
    if !installer && !portable {
        asset.reason = "不是支持的安装器或软件压缩包".into();
        return;
    }
    if (windows && os != "windows")
        || (mac && os != "macos")
        || (linux && os != "linux")
        || token(
            &name,
            &[
                "android", "ios", "i386", "i686", "win32", "32bit", "armv7", "armv6",
            ],
        )
        || (arm && arch != "aarch64")
        || (x64 && arch != "x86_64")
        || (token(&name, &["x86"]) && !x64)
    {
        asset.reason = "不适用于当前系统与架构".into();
        return;
    }
    if token(
        &name,
        &[
            "debug",
            "symbols",
            "pdb",
            "source",
            "sources",
            "checksum",
            "checksums",
            "sha256",
            "sha512",
            "sdk",
        ],
    ) {
        asset.reason = "调试、源码或校验附属文件".into();
        return;
    }
    if asset.size == 0 || asset.size > super::network::MAX_DOWNLOAD_BYTES {
        asset.reason = "文件为空或超过下载大小限制".into();
        return;
    }
    if super::network::trusted_download_url(&asset.url).is_err() {
        asset.reason = "下载地址不在可信 GitHub 域名内".into();
        return;
    }
    if pattern.is_some_and(|pattern| !wildcard(pattern, &name)) {
        asset.reason = "不符合指定文件名规则".into();
        return;
    }
    let platform = match os {
        "windows" => windows,
        "macos" => mac || installer,
        "linux" => linux || installer || name.ends_with(".appimage"),
        _ => false,
    };
    asset.compatible = true;
    asset.score = 20
        + if platform { 35 } else { 0 }
        + if (arm && arch == "aarch64") || (x64 && arch == "x86_64") || universal {
            25
        } else {
            0
        };
    if installer {
        asset.score += 15;
    }
    if name.contains("portable") {
        asset.score += 10;
    }
    if (preference == "portable" && portable) || (preference == "installer" && installer) {
        asset.score += 100;
    }
    if pattern.is_some() {
        asset.score += 50;
    }
    asset.reason = if platform {
        format!(
            "适配 {os} {arch}{}",
            if arm || x64 || universal {
                ""
            } else {
                "（架构未明确标注）"
            }
        )
    } else {
        "平台未明确标注，请核对后选择".into()
    };
}
pub fn classify_for_project(
    asset: &mut ReleaseAsset,
    preference: &str,
    pattern: Option<&str>,
    owner: &str,
    repo: &str,
) {
    classify(asset, preference, pattern);
    let name = asset.name.to_ascii_lowercase();
    if name == "updater.exe" {
        asset.compatible = false;
        asset.score = 0;
        asset.reason = "辅助更新器，不是主程序".into();
    }
    if owner.eq_ignore_ascii_case("RoutineMiku")
        && repo.eq_ignore_ascii_case("apkstart")
        && token(&name, &["runtime"])
    {
        asset.compatible = false;
        asset.score = 0;
        asset.reason = "运行环境组件包，请选择完整安装包或便携包".into();
    }
    if owner.eq_ignore_ascii_case("Lake1059")
        && repo.eq_ignore_ascii_case("FFmpegFreeUI")
        && name == "ffmpegfreeui.x64.exe"
        && asset.compatible
    {
        asset.kind = "portable".into();
        asset.score += 30;
        if preference == "portable" {
            asset.score += 100;
        } else if preference == "installer" {
            asset.score -= 100;
        }
        asset.reason = "官方 Windows x64 单文件便携主程序，不是安装器".into();
    }
    if name.ends_with(".msix") {
        asset.reason = "暂不支持 MSIX 安装，请选择 Windows ZIP 便携包".into();
    }
}
pub fn choose(assets: &[ReleaseAsset]) -> Option<u64> {
    let mut candidates: Vec<&ReleaseAsset> = assets.iter().filter(|a| a.compatible).collect();
    candidates.sort_by_key(|a| std::cmp::Reverse(a.score));
    let first = candidates.first()?;
    if first.score < 35
        || candidates
            .get(1)
            .is_some_and(|a| first.score - a.score < 15)
    {
        return None;
    }
    Some(first.id)
}
pub fn update_status(p: &mut Project) {
    let Some(release) = &p.release else {
        p.status = "noRelease".into();
        return;
    };
    p.status = match p.installed_version.as_deref() {
        Some(installed) => match compare_versions(&release.tag, installed) {
            Some(Ordering::Greater) => "updateAvailable",
            Some(_) => "upToDate",
            None => "versionUnknown",
        },
        None => "updateAvailable",
    }
    .into();
    if !release.assets.iter().any(|a| a.compatible) && p.status == "updateAvailable" {
        p.status = "noCompatibleAsset".into();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_are_numeric() {
        assert_eq!(
            compare_versions("v1.10.0", "1.9.0"),
            Some(Ordering::Greater)
        );
        assert_eq!(compare_versions("v2.1", "2.1.0"), Some(Ordering::Equal));
        assert_eq!(
            compare_versions("v2.1.0-beta.1", "2.1.0"),
            Some(Ordering::Less)
        );
        assert_eq!(compare_versions("nightly", "main"), None);
        assert_eq!(
            compare_versions("1.2.3+build.2", "1.2.3+build.1"),
            Some(Ordering::Equal)
        );
    }
    #[test]
    fn patterns_match_without_code_execution() {
        assert!(wildcard("*WIN?x64.zip", "tool-win-x64.zip"));
        assert!(!wildcard("*.exe", "tool.zip"));
    }
    #[test]
    fn sample_main_programs_are_not_confused_with_components() {
        for (owner, repo, names) in [
            (
                "Lake1059",
                "FFmpegFreeUI",
                vec![
                    "FFmpegFreeUI.x64.exe",
                    "Updater.exe",
                    "FFmpegFreeUI.arm64.exe",
                ],
            ),
            (
                "RoutineMiku",
                "apkstart",
                vec![
                    "ApkStart-win64-portable.zip",
                    "ApkStart-runtime-v0.3.2.zip",
                    "ApkStart-linux-x64.zip",
                ],
            ),
            (
                "Predidit",
                "Kazumi",
                vec![
                    "Kazumi_windows.zip",
                    "Kazumi_windows_arm64.zip",
                    "Kazumi_linux_amd64.zip",
                ],
            ),
        ] {
            for preference in ["auto", "portable"] {
                let assets: Vec<_> = names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        let mut asset = ReleaseAsset {
                            id: index as u64 + 1,
                            name: (*name).into(),
                            size: 100,
                            url: format!(
                                "https://github.com/{owner}/{repo}/releases/download/v1/{name}"
                            ),
                            content_type: "application/octet-stream".into(),
                            digest: None,
                            kind: String::new(),
                            compatible: false,
                            score: 0,
                            reason: String::new(),
                        };
                        classify_windows_x64(&mut asset, preference, None);
                        if *name == "Updater.exe" || token(&name.to_ascii_lowercase(), &["runtime"])
                        {
                            asset.compatible = false;
                        }
                        if *name == "FFmpegFreeUI.x64.exe" {
                            asset.kind = "portable".into();
                            asset.score += 30;
                        }
                        asset
                    })
                    .collect();
                assert_eq!(choose(&assets), Some(1), "{repo}: {preference}");
                assert_eq!(assets[0].kind, "portable");
                assert!(
                    assets[1..].iter().all(|a| !a.compatible),
                    "{repo}: {preference}"
                );
            }
        }
    }
    #[test]
    fn ambiguity_is_not_auto_selected() {
        let a = ReleaseAsset {
            id: 1,
            name: "Kazumi_windows_amd64.zip".into(),
            size: 10,
            url: "https://github.com/Predidit/Kazumi/releases/download/v1/a.zip".into(),
            content_type: "".into(),
            digest: None,
            kind: "".into(),
            compatible: false,
            score: 0,
            reason: "".into(),
        };
        let mut one = a.clone();
        classify_for_platform(&mut one, "portable", None, "windows", "x86_64");
        assert!(one.compatible);
        let mut two = one.clone();
        two.id = 2;
        assert_eq!(choose(&[one, two]), None);
        let mut arm = a;
        arm.name = "Kazumi_windows_arm64.zip".into();
        classify_for_platform(&mut arm, "auto", None, "windows", "x86_64");
        assert!(!arm.compatible);
    }
}
