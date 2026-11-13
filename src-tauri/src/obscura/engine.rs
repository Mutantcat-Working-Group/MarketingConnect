// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 浏览器引擎解析链：环境变量 → 本机 Chrome → 已下载 → 自动下载

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// 返回第一个通过 `exists` 判定的路径
pub fn first_existing(paths: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    paths.iter().find(|p| exists(p)).cloned()
}

/// 当前平台的 Chrome 候选安装路径（按优先级）
pub fn system_chrome_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ));
        paths.push(PathBuf::from(
            "/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
        ));
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            paths.push(home.join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        for key in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(key) {
                paths.push(
                    PathBuf::from(base)
                        .join("Google/Chrome/Application/chrome.exe"),
                );
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        paths.push(PathBuf::from("/usr/bin/google-chrome"));
        paths.push(PathBuf::from("/usr/bin/google-chrome-stable"));
        paths.push(PathBuf::from("/opt/google/chrome/chrome"));
        paths.push(PathBuf::from("/usr/bin/chromium"));
        paths.push(PathBuf::from("/usr/bin/chromium-browser"));
        paths.push(PathBuf::from("/snap/bin/chromium"));
    }

    paths
}

/// 候选文件名是否像浏览器可执行文件
///
/// 抽成纯函数是为了能在单一平台上测试所有平台的候选名：Linux 与 Windows
/// 的 `#[cfg]` 分支在本机不参与编译，若把断言写在平台测试里，只有等对应
/// 平台的 CI 跑起来才会发现写错。
pub fn candidate_is_browser_executable(path: &Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    name.contains("chrome") || name.contains("chromium")
}

/// 探测本机已安装的 Chrome
pub fn find_system_chrome() -> Option<PathBuf> {
    first_existing(&system_chrome_paths(), |p| p.is_file())
}

/// 从「`<二进制> --version`」的输出中取出主版本号
///
/// 输出形如 `Google Chrome 157.0.8103.38` / `Google Chrome for Testing
/// 157.0.8103.38`。取不到时返回 None，由调用方回退。
pub fn chrome_major_from_version_output(output: &str) -> Option<&str> {
    output
        .split_whitespace()
        .find(|token| token.split('.').count() >= 3 && token.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(|token| token.split('.').next().unwrap_or(token))
}

/// 探测一个浏览器可执行文件的真实主版本号
///
/// 直接跑 `<二进制> --version` 取输出，而不是信任配置里的锁定版本：
/// 引擎解析链可能选中本机 Chrome（版本随用户更新），UA 必须与之一致。
/// 探测失败返回 None，调用方应省略 `--user-agent`，让 Chrome 用它自己的
/// 真实 UA——**不要用锁定版本顶替**，那会造出假 UA。
pub fn detect_major_version(executable: &Path) -> Option<String> {
    let output = std::process::Command::new(executable)
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    chrome_major_from_version_output(&text).map(str::to_string)
}

/// 真 Chrome 启动时使用的 User-Agent
///
/// 无头模式默认会带上 `HeadlessChrome` 标记，是明显的自动化特征。
/// 这里按平台与主版本号生成与之匹配的普通 UA。
///
/// **major 必须来自实际启动的那个二进制**，不能图省事用 `LOCKED_CFT_VERSION`：
/// 引擎解析链是「本机 Chrome 优先」，本机装的是 156 而锁定的是 157 时，
/// 用锁定版本会造出「UA 声明 157、内核却是 156」的假 UA——对风控平台
/// 而言这比带 HeadlessChrome 标记更可疑。实测确认过这个坑：
/// UA 报 157.0.0.0，而 `Chrome --version` 是 156.0.8078.80。
pub fn chrome_user_agent(major: &str) -> String {
    let platform = if cfg!(target_os = "macos") {
        "Macintosh; Intel Mac OS X 10_15_7"
    } else if cfg!(target_os = "windows") {
        "Windows NT 10.0; Win64; x64"
    } else {
        "X11; Linux x86_64"
    };
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36"
    )
}

/// 引擎来源，用于日志与错误提示
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineOrigin {
    /// MC_ENGINE_CMD 显式指定
    Explicit,
    /// 本机已安装的 Chrome
    System,
    /// 应用数据目录中已下载的 Chrome for Testing
    Downloaded,
}

/// 纯函数：按优先级挑选引擎
pub fn pick_engine(
    explicit: Option<PathBuf>,
    system: Option<PathBuf>,
    downloaded: Option<PathBuf>,
) -> Option<(PathBuf, EngineOrigin)> {
    explicit
        .map(|p| (p, EngineOrigin::Explicit))
        .or_else(|| system.map(|p| (p, EngineOrigin::System)))
        .or_else(|| downloaded.map(|p| (p, EngineOrigin::Downloaded)))
}

/// Chrome for Testing 官方接口里当前平台的键
pub fn chrome_for_testing_platform_key() -> Option<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { Some("mac-arm64") }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { Some("mac-x64") }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    { Some("win64") }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { Some("linux64") }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    { Some("linux64") }
    #[cfg(not(any(
        all(target_os = "macos", any(target_arch = "aarch64", target_arch = "x86_64")),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64"))
    )))]
    { None }
}

/// 官方版本清单地址
pub const CFT_VERSIONS_URL: &str =
    "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";

/// 锁定的 Chrome for Testing 版本。
///
/// 中继的事件补丁是按特定版本的流量推导的，版本漂移会引入新的未知事件，
/// 因此不要改成「每次启动取最新」。实测该版本可正常下载与解压：
/// 187.5 MB / 35.4s，解压后 `--version` 输出
/// `Google Chrome for Testing 157.0.8103.38`。
pub const LOCKED_CFT_VERSION: &str = "157.0.8103.38";

/// 从官方清单中取出指定平台的 Stable 下载地址
pub fn stable_download_url(feed: &serde_json::Value, platform_key: &str) -> Option<String> {
    feed.get("channels")?
        .get("Stable")?
        .get("downloads")?
        .get("chrome")?
        .as_array()?
        .iter()
        .find(|item| item.get("platform").and_then(serde_json::Value::as_str) == Some(platform_key))
        .and_then(|item| item.get("url"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

/// 从官方清单中取出 Stable 版本号
pub fn stable_version(feed: &serde_json::Value) -> Option<String> {
    feed.get("channels")?
        .get("Stable")?
        .get("version")?
        .as_str()
        .map(str::to_owned)
}

/// Chrome for Testing 解压后的顶层目录名
///
/// 官方 zip 的顶层目录就是 `chrome-<平台键>`：macOS 是 `chrome-mac-arm64`
/// 或 `chrome-mac-x64`（**不是** `chrome-mac`），Windows 是 `chrome-win64`，
/// Linux 是 `chrome-linux64`。这一点已用官方 zip 的中央目录条目与本地
/// Playwright 缓存的实际布局核对过。
pub fn extracted_dir_name(platform_key: &str) -> String {
    format!("chrome-{platform_key}")
}

/// 解压目录内可执行文件的相对路径
///
/// 抽成纯函数是为了能在单一平台上覆盖全部平台键——各平台的 `#[cfg]`
/// 分支在其它平台不参与编译，写错只有等到对应平台的 CI 才会暴露。
pub fn executable_relative_path(platform_key: &str) -> Option<PathBuf> {
    let dir = extracted_dir_name(platform_key);
    if platform_key.starts_with("mac-") {
        Some(
            PathBuf::from(dir)
                .join("Google Chrome for Testing.app")
                .join("Contents")
                .join("MacOS")
                .join("Google Chrome for Testing"),
        )
    } else if platform_key.starts_with("win") {
        Some(PathBuf::from(dir).join("chrome.exe"))
    } else if platform_key.starts_with("linux") {
        Some(PathBuf::from(dir).join("chrome"))
    } else {
        None
    }
}

/// 指定平台键下，引擎在应用数据目录中的安装位置
pub fn installed_chrome_path_for(
    data_dir: &Path,
    version: &str,
    platform_key: &str,
) -> Option<PathBuf> {
    Some(
        data_dir
            .join("engine")
            .join(version)
            .join(executable_relative_path(platform_key)?),
    )
}

/// 当前平台的引擎安装位置（平台不受支持时返回 `None`）
pub fn installed_chrome_path(data_dir: &Path, version: &str) -> Option<PathBuf> {
    installed_chrome_path_for(data_dir, version, chrome_for_testing_platform_key()?)
}

/// 下载并解压 Chrome for Testing；返回可执行文件路径。
/// `version` 必须来自配置（锁定版本），不在此函数内取最新。
pub async fn download_chrome(data_dir: &Path, url: &str, version: &str) -> Result<PathBuf> {
    let key = chrome_for_testing_platform_key().context("当前平台不支持自动获取引擎")?;
    let exe =
        installed_chrome_path_for(data_dir, version, key).context("无法确定引擎安装路径")?;
    if exe.is_file() {
        return Ok(exe);
    }

    let bytes = reqwest::get(url)
        .await
        .context("下载引擎失败")?
        .error_for_status()
        .context("下载引擎失败")?
        .bytes()
        .await
        .context("读取引擎数据失败")?;

    let target_dir = data_dir.join("engine").join(version);
    std::fs::create_dir_all(&target_dir).context("创建引擎目录失败")?;

    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("引擎压缩包无法解析")?;
    archive.extract(&target_dir).context("解压引擎失败")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&exe).context("引擎文件缺失")?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&exe, perms).context("设置引擎执行权限失败")?;
    }

    anyhow::ensure!(exe.is_file(), "解压后未找到引擎可执行文件");
    Ok(exe)
}

/// FNV-1a 哈希：稳定、无依赖，用于给 profile 目录名加确定性后缀
///
/// 不用 `DefaultHasher`，因为它不保证跨 Rust 版本稳定；后缀一旦变化，
/// profile 目录就会变，用户会平白丢掉浏览器侧的登录态。
fn fnv1a(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 按实例键派生 Chrome 用户数据目录
///
/// Chrome 对同一 `--user-data-dir` 的第二个进程会直接中止
/// （ProcessSingleton 冲突），而 `get_or_start` 是按代理分实例的，
/// 因此每个实例必须有独立目录。键里可能含 `:`、`/`、`@` 等字符，
/// 需要清洗成文件名安全的形式；末尾附哈希后缀，避免清洗后同形但
/// 实际不同的键（如 `http://a-b` 与 `http://a_b`）落到同一目录。
pub fn chrome_profile_dir(data_dir: &Path, instance_key: &str) -> PathBuf {
    let mut safe: String = instance_key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    safe.truncate(32);
    if safe.is_empty() {
        safe.push_str("direct");
    }
    data_dir
        .join("chrome-profile")
        .join(format!("{safe}-{:08x}", fnv1a(instance_key) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_dir_is_stable_for_same_key() {
        let d = Path::new("/data");
        assert_eq!(
            chrome_profile_dir(d, "socks5://x:1"),
            chrome_profile_dir(d, "socks5://x:1"),
            "同一个键必须每次得到同一目录，否则用户会丢登录态"
        );
    }

    #[test]
    fn profile_dir_differs_per_instance_key() {
        let d = Path::new("/data");
        assert_ne!(
            chrome_profile_dir(d, "http://a:8080"),
            chrome_profile_dir(d, "http://b:8080"),
            "不同代理必须用不同 profile，否则第二个实例会中止"
        );
        assert_ne!(chrome_profile_dir(d, "direct"), chrome_profile_dir(d, "http://a:8080"));
    }

    #[test]
    fn profile_dir_survives_sanitization_collisions() {
        let d = Path::new("/data");
        // 清洗后同形、但实际不同的键不能落到同一目录
        assert_ne!(chrome_profile_dir(d, "http://a-b"), chrome_profile_dir(d, "http://a_b"));
    }

    #[test]
    fn profile_dir_name_is_filesystem_safe() {
        let p = chrome_profile_dir(Path::new("/data"), "http://user:pw@127.0.0.1:8080/pa th");
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(!name.contains('/') && !name.contains(':') && !name.contains('@') && !name.contains(' '), "目录名不安全: {name}");
        assert!(p.starts_with(Path::new("/data").join("chrome-profile")));
    }

    #[test]
    fn picks_first_existing_candidate() {
        let paths = vec![PathBuf::from("/a"), PathBuf::from("/b"), PathBuf::from("/c")];
        let hit = first_existing(&paths, |p| p == Path::new("/b") || p == Path::new("/c"));
        assert_eq!(hit, Some(PathBuf::from("/b")), "应返回顺序最靠前的命中项");
    }

    #[test]
    fn returns_none_when_nothing_exists() {
        let paths = vec![PathBuf::from("/a")];
        assert_eq!(first_existing(&paths, |_| false), None);
    }

    #[test]
    fn system_paths_are_absolute_and_non_empty() {
        let paths = system_chrome_paths();
        assert!(!paths.is_empty(), "每个受支持平台都必须有候选路径");
        for p in &paths {
            assert!(p.is_absolute(), "候选路径必须是绝对路径: {}", p.display());
            assert!(
                candidate_is_browser_executable(p),
                "候选路径必须指向 Chrome/Chromium 可执行文件: {}",
                p.display()
            );
        }
    }

    #[test]
    fn accepts_browser_names_from_every_platform() {
        // 显式覆盖三平台的真实文件名：非本平台的分支不会参与编译，
        // 只有在这里断言才能在本机拦住 Linux/Windows 上的错误预期
        for name in [
            "Google Chrome",
            "Google Chrome for Testing",
            "chrome.exe",
            "google-chrome",
            "google-chrome-stable",
            "chrome",
            "chromium",
            "chromium-browser",
        ] {
            assert!(
                candidate_is_browser_executable(Path::new(name)),
                "{name} 应被判定为浏览器可执行文件"
            );
        }
    }

    #[test]
    fn rejects_non_browser_names() {
        // 判定是宽松的子串检查，只用于拦住明显不是浏览器的候选名
        for name in ["bash", "sh", "node", "firefox"] {
            assert!(
                !candidate_is_browser_executable(Path::new(name)),
                "{name} 不应被判定为浏览器可执行文件"
            );
        }
    }

    #[test]
    fn maps_platform_to_cft_key() {
        let key = chrome_for_testing_platform_key().expect("受支持平台必须有 CfT 键");
        assert!(
            ["mac-arm64", "mac-x64", "win64", "linux64"].contains(&key),
            "CfT 平台键必须是官方取值，实际为 {key}"
        );
    }

    #[test]
    fn picks_download_url_for_current_platform() {
        // 取自官方接口的真实响应片段
        let json = serde_json::json!({
            "channels": { "Stable": { "version": "157.0.8103.38", "downloads": { "chrome": [
                { "platform": "mac-arm64", "url": "https://storage.googleapis.com/chrome-for-testing-public/157.0.8103.38/mac-arm64/chrome-mac-arm64.zip" },
                { "platform": "linux64",  "url": "https://storage.googleapis.com/chrome-for-testing-public/157.0.8103.38/linux64/chrome-linux64.zip" }
            ]}}}
        });
        let url = stable_download_url(&json, "mac-arm64").expect("应解析出下载地址");
        assert!(url.starts_with("https://storage.googleapis.com/chrome-for-testing-public/"));
        assert!(url.ends_with("chrome-mac-arm64.zip"));
    }

    #[test]
    fn rejects_missing_platform_in_feed() {
        let json = serde_json::json!({ "channels": { "Stable": { "version": "1", "downloads": { "chrome": [] }}}});
        assert_eq!(stable_download_url(&json, "mac-arm64"), None);
    }

    #[test]
    fn rejects_malformed_feed() {
        assert_eq!(stable_download_url(&serde_json::json!({}), "mac-arm64"), None);
    }

    #[test]
    fn macos_extract_dir_carries_the_arch() {
        // 官方 zip 顶层目录是 chrome-mac-arm64 / chrome-mac-x64，不是 chrome-mac。
        // 写成 chrome-mac 会让解压后找不到可执行文件，从而每次启动都重下整个引擎。
        assert_eq!(extracted_dir_name("mac-arm64"), "chrome-mac-arm64");
        assert_eq!(extracted_dir_name("mac-x64"), "chrome-mac-x64");
    }

    #[test]
    fn resolves_executable_for_every_platform_key() {
        // 用 iter() 而不是字符串比较，避免 Windows CI 上分隔符不同导致失败
        let parts = |p: PathBuf| -> Vec<String> {
            p.iter().map(|c| c.to_string_lossy().to_string()).collect()
        };

        assert_eq!(
            parts(executable_relative_path("mac-arm64").unwrap()),
            vec![
                "chrome-mac-arm64",
                "Google Chrome for Testing.app",
                "Contents",
                "MacOS",
                "Google Chrome for Testing"
            ]
        );
        assert_eq!(
            parts(executable_relative_path("win64").unwrap()),
            vec!["chrome-win64", "chrome.exe"]
        );
        assert_eq!(
            parts(executable_relative_path("linux64").unwrap()),
            vec!["chrome-linux64", "chrome"]
        );
        assert_eq!(executable_relative_path("unknown-os"), None);
    }

    #[test]
    fn installed_path_lives_under_engine_version_dir() {
        let data = Path::new("/data");
        let version = "157.0.8103.38";
        let p = installed_chrome_path_for(data, version, "linux64").unwrap();
        assert!(
            p.starts_with(data.join("engine").join(version)),
            "安装路径必须在 engine/<版本> 之下，实际: {}",
            p.display()
        );
        assert_eq!(p.file_name().unwrap().to_string_lossy(), "chrome");
    }

    #[test]
    fn resolution_order_prefers_explicit_then_system() {
        let explicit = Some(PathBuf::from("/explicit/chrome"));
        let system = Some(PathBuf::from("/system/chrome"));
        let downloaded = Some(PathBuf::from("/downloaded/chrome"));
        let picked = pick_engine(explicit.clone(), system.clone(), downloaded.clone());
        assert_eq!(picked, Some(("/explicit/chrome".into(), EngineOrigin::Explicit)));
    }

    #[test]
    fn resolution_falls_back_to_system_without_explicit() {
        let picked = pick_engine(None, Some(PathBuf::from("/system/chrome")), Some(PathBuf::from("/downloaded/chrome")));
        assert_eq!(picked, Some(("/system/chrome".into(), EngineOrigin::System)));
    }

    #[test]
    fn resolution_falls_back_to_downloaded_last() {
        let picked = pick_engine(None, None, Some(PathBuf::from("/downloaded/chrome")));
        assert_eq!(picked, Some(("/downloaded/chrome".into(), EngineOrigin::Downloaded)));
    }

    #[test]
    fn resolution_returns_none_when_all_missing() {
        assert_eq!(pick_engine(None, None, None), None);
    }

    #[test]
    fn user_agent_has_no_headless_marker() {
        let ua = chrome_user_agent("157");
        assert!(!ua.to_lowercase().contains("headless"), "UA 不得含无头标记: {ua}");
        assert!(ua.contains("Chrome/157.0.0.0"), "UA 主版本必须与引擎一致: {ua}");
        assert!(ua.starts_with("Mozilla/5.0 ("), "UA 格式必须标准: {ua}");
    }

    #[test]
    fn user_agent_tolerates_malformed_version() {
        assert!(chrome_user_agent("weird").contains("Chrome/weird.0.0.0"));
    }

    #[test]
    fn major_is_parsed_from_real_chrome_version_output() {
        // 本机 Chrome 与 Chrome for Testing 的输出格式不同，都要能取到主版本
        assert_eq!(chrome_major_from_version_output("Google Chrome 156.0.8078.80"), Some("156"));
        assert_eq!(chrome_major_from_version_output("Google Chrome for Testing 157.0.8103.38"), Some("157"));
        assert_eq!(chrome_major_from_version_output("Google Chrome 157.0.8103.38\n"), Some("157"));
        // 取不到时不得 panic，由调用方回退
        assert_eq!(chrome_major_from_version_output(""), None);
        assert_eq!(chrome_major_from_version_output("no version here"), None);
    }

    #[test]
    fn ua_never_claims_a_version_the_binary_does_not_have() {
        // 回归：以前用 LOCKED_CFT_VERSION 拼 UA，本机 Chrome 是 156 时
        // 会产出「Chrome/157.0.0.0」的假 UA。现在 UA 只接受实际主版本。
        let actual = chrome_major_from_version_output("Google Chrome 156.0.8078.80").unwrap();
        let ua = chrome_user_agent(actual);
        assert!(ua.contains("Chrome/156.0.0.0"), "UA 主版本必须等于真实内核: {ua}");
        assert!(!ua.contains("157"), "不得夹带未使用的锁定版本: {ua}");
    }
}
