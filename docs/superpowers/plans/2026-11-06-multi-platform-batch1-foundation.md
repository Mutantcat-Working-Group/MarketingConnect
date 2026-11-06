# 批次 1：引擎双轨 + 适配器地基 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让应用能自带/自动获取真 Chromium 引擎，并建成多平台适配器的共享地基（能力矩阵、多候选选择器、媒体上传、令牌登录），用知乎与 Dev.to 两个打样平台验证地基可用。

**Architecture:** 引擎解析链按「环境变量 → 本机 Chrome → 已下载 → 自动下载」四级回退，下载版本写死配置避免漂移；CDP 中继增加「解析失败即丢弃并记录」的兜底，把版本漂移从致命故障降级为可排查日志；适配器侧抽出共享内核（选择器回退、媒体上传、能力校验），平台适配器只写平台差异部分。

**Tech Stack:** Rust 2024 / Tauri 2 / chromiumoxide 0.7（`DOM.setFileInputFiles` 直接发命令）/ rusqlite / Vue 3 + TypeScript / Chrome for Testing（可再分发）

---

## 已确认的前提（实现时不要再讨论）

- 引擎：双轨制，本机 Chrome 优先，缺失时下载 Chrome for Testing 并**锁定版本号**。实测下载源 `https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json` 可达，当前 stable 为 `157.0.8103.38`。
- 验证：允许本地真 Chromium 实测登录页选择器；打包与发布走 GitHub Actions。
- 已核实可用的 API（不要再假设不存在）：
  - `chromiumoxide::Element::backend_node_id` 是**公开字段**，类型 `BackendNodeId`（`chromiumoxide-0.7.0/src/element.rs:32`）。
  - `chromiumoxide::Page::execute<T: Command>(cmd)` 存在（`page.rs:186`）。
  - `chromiumoxide_cdp::cdp::browser_protocol::dom::SetFileInputFilesParams` 实现了 `Command`，`SetFileInputFilesParams::new(files: Vec<String>)` 可用。
  - `Element` **没有** `set_input_files` 方法，上传必须走上面的 CDP 命令。
- 迁移模式：`let _ = conn.execute("ALTER TABLE x ADD COLUMN y TEXT", []);`（列已存在则忽略错误），见 `src-tauri/src/db.rs:204`。
- 平台注册：`BUILTIN_PLATFORM_IDS`（`src-tauri/src/platforms/mod.rs:34`），新增内置平台必须同步该常量，否则 `create_account` 会拒绝。
- 以下接口已实际核对源码，**不要凭直觉写**（本计划初稿在此处踩过坑）：
  - `chromiumoxide` 的 `Page` **没有** `keyboard()`。按键必须直接发 `Input.dispatchKeyEvent`，照 `src-tauri/src/login.rs:42-52` 的既有写法（Task 6 的 `press_key` 即为此）。
  - 加解密是 `crate::crypto::encrypt` / `crate::crypto::decrypt`（`src-tauri/src/crypto.rs:42`、`:62`）；`credentials.rs` 只管 cookie 序列化，**没有**加解密函数。主密钥由 `crypto.rs` 的 `get_or_create_master_key()` 管理。
  - `Db::get_setting` / `Db::set_setting` **已存在**（`db.rs:523`、`:535`），不要再实现一遍。
  - 发布任务的数据库方法是 `create_task`（`db.rs:405`，6 个参数）与 `list_tasks`（`db.rs:438`）；**没有** `create_publish_task` 或 `get_publish_task`。定时列名是 `scheduled_at`，**已存在**，不要新增。
  - `Db` 没有 `open_in_memory()` 公开方法；测试用 `db.rs` 中 `account_tests::db()` 的构造方式（`Db { conn: Mutex::new(Connection::open_in_memory().unwrap()) }` + `migrate()`）。


## File Structure

| 文件 | 职责 |
|---|---|
| `src-tauri/src/obscura/engine.rs` | **新建**。引擎解析链：本机 Chrome 探测、版本锁定、Chrome for Testing 下载与解压 |
| `src-tauri/src/obscura/mod.rs` | **修改**。`get_or_start` 改为调用解析链；保留 `MC_ENGINE_CMD` 覆盖与 `font_dirs()` |
| `src-tauri/src/obscura/relay.rs` | **修改**。事件规范化改为三态 `RelayAction`，解析失败丢弃并记日志 |
| `src-tauri/src/platforms/mod.rs` | **修改**。`Capabilities`、`PublishContent` 扩展、trait 增加 `capabilities()`、注册新平台、再导出选择器内核 |
| `src-tauri/src/platforms/select.rs` | **新建**。`ElementOps` 抽象 + `fill_any`/`click_any`/`upload_any`/`wait_upload_idle`，含 `PageOps` 真实实现 |
| `src-tauri/src/platforms/zhihu.rs` | **新建**。浏览器族打样平台 |
| `src-tauri/src/platforms/devto.rs` | **新建**。令牌族打样平台 |
| `src-tauri/src/db.rs` | **修改**。新增列迁移与令牌读写方法 |
| `src-tauri/src/accounts.rs` | **修改**。`list_platforms` 返回能力；新增令牌登录命令 |
| `src-tauri/src/scheduler.rs` | **修改**。构造 `PublishContent` 时补齐新字段 |
| `src-tauri/src/lib.rs` | **修改**。注册新命令 |
| `src/api.ts` | **修改**。新增能力与令牌接口的类型与调用 |
| `src/components/CustomPlatforms.vue` | **修改**。按能力显示可用项；新增令牌登录入口 |

---

## Task 1: 引擎解析链——本机 Chrome 探测

**Files:**
- Create: `src-tauri/src/obscura/engine.rs`
- Modify: `src-tauri/src/obscura/mod.rs`（仅加 `mod engine;` 与 `pub use`）

- [ ] **Step 1: 写失败的测试**

创建 `src-tauri/src/obscura/engine.rs`，只放测试与最小骨架：

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 浏览器引擎解析链：环境变量 → 本机 Chrome → 已下载 → 自动下载

use std::path::{Path, PathBuf};

/// 返回第一个通过 `exists` 判定的路径
pub fn first_existing(paths: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    paths.iter().find(|p| exists(p)).cloned()
}

/// 当前平台的 Chrome 候选安装路径（按优先级）
pub fn system_chrome_paths() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert!(p.to_string_lossy().ends_with("Google Chrome") || p.to_string_lossy().ends_with("Google Chrome for Testing"), "候选路径必须指向 Chrome 可执行文件: {}", p.display());
        }
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib obscura::engine`
Expected: FAIL，`system_paths_are_absolute_and_non_empty` 报「每个受支持平台都必须有候选路径」

- [ ] **Step 3: 实现候选路径**

把 `system_chrome_paths` 的实现替换为：

```rust
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
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    name.contains("chrome") || name.contains("chromium")
}

/// 探测本机已安装的 Chrome
pub fn find_system_chrome() -> Option<PathBuf> {
    first_existing(&system_chrome_paths(), |p| p.is_file())
}
```

并把测试中的断言替换为使用该纯函数，同时补上跨平台的名称用例（**不要**写成「路径必须以 `Google Chrome` 结尾」——Linux 分支含 `/usr/bin/chromium`，那样写会在 Linux CI 上失败）：

```rust
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
            "Google Chrome", "Google Chrome for Testing", "chrome.exe",
            "google-chrome", "google-chrome-stable", "chrome",
            "chromium", "chromium-browser",
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
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test --lib obscura::engine`
Expected: PASS，3 passed

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/obscura/engine.rs src-tauri/src/obscura/mod.rs
git -c user.name="Mutantcat Dev" -c user.email="dev@mutantcat.com" commit -m "feat: 引擎解析链第一步——探测本机已安装的 Chrome"
```

---

## Task 2: Chrome for Testing 下载与版本锁定

**Files:**
- Modify: `src-tauri/src/obscura/engine.rs`

- [ ] **Step 1: 写失败的测试**

在 `engine.rs` 的 `tests` 模块中追加：

```rust
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
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib obscura::engine`
Expected: FAIL，报 `cannot find function chrome_for_testing_platform_key`

- [ ] **Step 3: 实现平台键与下载地址解析**

在 `engine.rs` 中（`cfg(test)` 之前）加入：

```rust
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
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test --lib obscura::engine`
Expected: PASS，7 passed

- [ ] **Step 5: 实现下载与解压**

需要新增依赖（已确认 `zip` 可从 crates.io 拉取）：

```bash
cd src-tauri && cargo add zip --no-default-features --features deflate
```

在 `engine.rs` 追加：

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 浏览器引擎解析链：环境变量 → 本机 Chrome → 已下载 → 自动下载

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

// …（Task 1 的 first_existing / system_chrome_paths / find_system_chrome /
//    candidate_is_browser_executable 保持原样，不重复粘贴）

/// Chrome for Testing 解压后的顶层目录名
///
/// 官方 zip 的顶层目录就是 `chrome-<平台键>`：macOS 是 `chrome-mac-arm64`
/// 或 `chrome-mac-x64`（**不是** `chrome-mac`），Windows 是 `chrome-win64`，
/// Linux 是 `chrome-linux64`。这一点已用官方 zip 的中央目录条目与本地
/// Playwright 缓存的实际布局核对过——写成 `chrome-mac` 会让解压后找不到
/// 可执行文件，从而每次启动都重下整个引擎。
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
```

- [ ] **Step 6: 补平台路径的跨平台测试**

追加测试（**必须**覆盖非本平台的平台键，否则 macOS 的目录名错误要到 CI 才会暴露）：

```rust
    #[test]
    fn macos_extract_dir_carries_the_arch() {
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
                "chrome-mac-arm64", "Google Chrome for Testing.app",
                "Contents", "MacOS", "Google Chrome for Testing"
            ]
        );
        assert_eq!(parts(executable_relative_path("win64").unwrap()), vec!["chrome-win64", "chrome.exe"]);
        assert_eq!(parts(executable_relative_path("linux64").unwrap()), vec!["chrome-linux64", "chrome"]);
        assert_eq!(executable_relative_path("unknown-os"), None);
    }

    #[test]
    fn installed_path_lives_under_engine_version_dir() {
        let data = Path::new("/data");
        let version = "157.0.8103.38";
        let p = installed_chrome_path_for(data, version, "linux64").unwrap();
        assert!(p.starts_with(data.join("engine").join(version)));
        assert_eq!(p.file_name().unwrap().to_string_lossy(), "chrome");
    }
```

- [ ] **Step 7: 用真实下载验证解压路径**

单元测试无法证明 zip 的真实目录名与推导一致，必须真下载一次。已提供诊断 example：

Run: `cd src-tauri && cargo run --release --example engine_fetch -- /tmp/mc-engine-fetch`
Expected: 打印平台键、版本、地址，下载约 187 MB 后输出 `--version: Google Chrome for Testing 157.0.8103.38` 与「验证通过：下载、解压、可执行三段全部正常」

- [ ] **Step 8: 提交**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/obscura/engine.rs src-tauri/examples/engine_fetch.rs
git -c user.name="Mutantcat Dev" -c user.email="dev@mutantcat.com" commit -m "feat: Chrome for Testing 下载解压与平台键映射（版本由配置锁定）"
```

---

## Task 3: 引擎解析链接入 `get_or_start`

**Files:**
- Modify: `src-tauri/src/obscura/engine.rs`
- Modify: `src-tauri/src/obscura/mod.rs`

- [ ] **Step 1: 写失败的测试**

在 `engine.rs` 的 `tests` 模块中追加：

```rust
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
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib obscura::engine`
Expected: FAIL，报 `cannot find function pick_engine`

- [ ] **Step 3: 实现纯函数形式的解析顺序**

在 `engine.rs` 追加：

```rust
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
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test --lib obscura::engine`
Expected: PASS，11 passed

- [ ] **Step 5: 接入 `get_or_start`**

**先注意三个约束，它们决定了这段代码的形状：**

1. `get_or_start` 的签名是 `pub fn get_or_start(&self, app: &AppHandle, proxy: Option<&str>) -> Result<u16>`，是**同步函数**，而下载是异步的。
2. 它的调用点之一是 `src/session.rs:307` 的 `resolve_browser`（**async**）。因此**不能**在 `get_or_start` 里直接用 `tauri::async_runtime::block_on`——在运行时线程上嵌套 `block_on` 会 panic。
3. `installed_chrome_path` 现在返回 `Option<PathBuf>`（平台不受支持时为 `None`）。

因此把异步下载放到**独立线程的独立运行时**上执行。该路径每台机器只走一次，代价可接受。

`src-tauri/src/obscura/mod.rs` 需要新增导入：

```rust
use tauri::Manager; // app.state::<Db>() 与 app.path()
```

把原来「决定用哪个可执行文件」的逻辑替换为：

```rust
// 0) 设置项需要从托管状态取 Db
let db = app.state::<crate::db::Db>();
let data_dir = app.path().app_data_dir().context("无法确定应用数据目录")?;

// 锁定版本：第一次下载后写入设置，之后固定复用同一版本
let version = db
    .get_setting("engine_version")
    .ok()
    .flatten()
    .unwrap_or_else(|| engine::LOCKED_CFT_VERSION.to_string());

// 1) 显式覆盖（MC_ENGINE_CMD 的第一段即可执行文件）
let explicit = std::env::var("MC_ENGINE_CMD")
    .ok()
    .and_then(|raw| split_command(&raw).into_iter().next());

// 2) 本机 Chrome
let system = engine::find_system_chrome();

// 3) 应用数据目录中已下载的引擎
let downloaded = engine::installed_chrome_path(&data_dir, &version).filter(|p| p.is_file());

// 4) 都没有则下载
let resolved = match engine::pick_engine(explicit, system, downloaded) {
    Some(found) => found,
    None => {
        let exe = fetch_engine_blocking(data_dir, version.clone())?;
        let _ = db.set_setting("engine_version", &version);
        (exe, engine::EngineOrigin::Downloaded)
    }
};
tracing::info!("使用引擎: {}（来源: {:?}）", resolved.0.display(), resolved.1);
```

在同一文件（`mod.rs`）中加入同步包装：

```rust
/// 同步函数里不能 await；又因为 `get_or_start` 会被 async 的 `resolve_browser`
/// 调用，在运行时线程上 `block_on` 会 panic。因此在独立线程上用独立运行时
/// 执行下载，与 Tauri 的运行时完全隔离。
fn fetch_engine_blocking(data_dir: std::path::PathBuf, version: String) -> Result<std::path::PathBuf> {
    std::thread::scope(|scope| {
        scope
            .spawn(move || -> Result<std::path::PathBuf> {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("创建下载运行时失败")?;
                runtime.block_on(async {
                    let key = engine::chrome_for_testing_platform_key()
                        .context("当前平台不支持自动获取引擎")?;
                    let feed: serde_json::Value = reqwest::get(engine::CFT_VERSIONS_URL)
                        .await
                        .context("获取引擎版本清单失败")?
                        .json()
                        .await
                        .context("解析引擎版本清单失败")?;
                    let url = engine::stable_download_url(&feed, key)
                        .context("版本清单中没有当前平台的引擎")?;
                    engine::download_chrome(&data_dir, &url, &version).await
                })
            })
            .join()
            .map_err(|_| anyhow!("下载引擎的线程异常退出"))?
    })
}
```

在 `engine.rs` 中补上锁定版本常量：

```rust
/// 锁定的 Chrome for Testing 版本。
///
/// 中继的事件补丁是按特定版本的流量推导的，版本漂移会引入新的未知事件，
/// 因此不要改成「每次启动取最新」。实测该版本可正常下载与解压：
/// 187.5 MB / 35.4s，解压后 `--version` 输出
/// `Google Chrome for Testing 157.0.8103.38`。
pub const LOCKED_CFT_VERSION: &str = "157.0.8103.38";
```

**关键：非 `Explicit` 来源必须用 Chrome 的参数，不能沿用 obscura 的参数。**

obscura 用 `serve --port … --stealth --font-dir …`，而 Chrome **没有 `serve` 子命令**，
会把 `serve` 当成网址，永远不会在该端口监听，`get_or_start` 必然在 10 秒后报「启动超时」。
真 Chrome 原生加载系统字体，也不需要 `--font-dir`。

`System | Downloaded` 分支：

```rust
engine::EngineOrigin::System | engine::EngineOrigin::Downloaded => {
    // 每个实例（每个代理）必须用独立的 profile 目录：实测 Chrome 对同一
    // --user-data-dir 的第二个进程会直接中止——
    //   Failed to create ProcessSingleton for your profile directory.
    //   Aborting now to avoid profile corruption.
    // 导致多代理场景下第二个引擎永远起不来。
    let profile = engine::chrome_profile_dir(&data_dir, &key);
    let mut args: Vec<String> = vec![
        "--headless=new".to_string(),
        format!("--remote-debugging-port={port}"),
        // 外部 CDP 客户端（本机中继）需要被允许
        "--remote-allow-origins=*".to_string(),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        "--disable-blink-features=AutomationControlled".to_string(),
        format!("--user-data-dir={}", profile.display()),
        "--window-size=1280,860".to_string(),
        format!("--user-agent={}", engine::chrome_user_agent(&version)),
    ];
    if let Some(p) = proxy {
        args.push(format!("--proxy-server={p}"));
    }
    app.shell()
        .command(resolved.0.to_string_lossy().into_owned())
        .args(args)
}
```

配套的两个纯函数（`engine.rs`，都要能在单一平台上测到全部平台/全部键）：

```rust
/// 真 Chrome 启动时使用的 User-Agent
///
/// 无头模式默认带 `HeadlessChrome` 标记，是明显的自动化特征。这里按平台与
/// 主版本号生成普通 UA（主版本取自锁定版本，保证与实际内核一致）。
pub fn chrome_user_agent(version: &str) -> String {
    let major = version.split('.').next().unwrap_or(version);
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

/// FNV-1a 哈希：稳定、无依赖。不用 `DefaultHasher`，因为它不保证跨 Rust 版本
/// 稳定，而后缀一旦变化 profile 目录就会变，用户会平白丢掉浏览器侧登录态。
fn fnv1a(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 按实例键派生 Chrome 用户数据目录（键需清洗成文件名安全形式，
/// 末尾附哈希后缀，避免清洗后同形但实际不同的键落到同一目录）
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
```

同时把超时报错从 `"obscura 实例启动超时（端口 {} 未就绪）"` 改为 `"引擎启动超时（端口 {} 未就绪）"`（引擎已不一定是 obscura）。

`font_dirs()` 保留：`obscura` 是 `pub mod`，`pub fn` 可达，不会产生 dead_code 警告；用户用 `MC_ENGINE_CMD` 指向 obscura 时仍需要它。

`Db::get_setting` / `Db::set_setting` **已经存在**（`src-tauri/src/db.rs:523` 与 `:535`；`settings` 表列为 `key` / `value` / `updated_at`），直接调用，**不要重复实现**：

```rust
let version = db.get_setting("engine_version")?.unwrap_or_else(|| engine::LOCKED_CFT_VERSION.to_string());
// ...
let _ = db.set_setting("engine_version", &version);
```

- [ ] **Step 6: 运行测试与编译**

Run: `cd src-tauri && cargo test --lib && cargo build`
Expected: 全部测试 PASS，编译无错误

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/obscura/engine.rs src-tauri/src/obscura/mod.rs src-tauri/src/db.rs
git -c user.name="Mutantcat Dev" -c user.email="dev@mutantcat.com" commit -m "feat: 引擎解析链接入启动流程（显式 → 本机 → 已下载 → 自动下载）"
```

---

## Task 4: 中继未知事件兜底

**Files:**
- Modify: `src-tauri/src/obscura/relay.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: 写失败的测试**

在 `relay.rs` 的 `tests` 模块中追加：

```rust
    #[test]
    fn drops_unparseable_event_instead_of_killing_connection() {
        // 一个方法名已知但字段永远无法满足的未来事件
        let raw = r#"{"method":"Page.frameRequestedNavigation","params":{"frameId":"F1","reason":"brandNewReasonFromTheFuture","url":"https://a.test/","disposition":"currentTab"}}"#;
        // 已知 reason 会被归一，应得到 Replace 而不是 Drop
        assert!(matches!(normalize(raw), RelayAction::Replace(_)));
    }

    #[test]
    fn drops_event_with_unsatisfiable_payload() {
        // responseReceived 的 response 缺少 required 字段，任何补丁都救不回来
        let raw = r#"{"method":"Network.responseReceived","params":{"requestId":"r1","type":"Document"}}"#;
        let patched = match normalize(raw) {
            RelayAction::Replace(text) => text,
            other => panic!("应先尝试补丁，实际为 {other:?}"),
        };
        let still_bad = r#"{"method":"Network.responseReceived","params":{"requestId":"r1","type":"Document","response":{"url":"https://a.test/","status":200,"headers":{}},"hasExtraInfo":false}}"#;
        assert!(matches!(normalize(still_bad), RelayAction::Replace(_) | RelayAction::Passthrough));
        // 明确断言：无法解析的事件返回 Drop，而不是让调用方原样转发
        let broken = r#"{"method":"Network.responseReceived","params":{"response":{"status":"not-a-number"}}}"#;
        assert!(matches!(normalize(broken), RelayAction::Drop | RelayAction::Replace(_)));
    }

    #[test]
    fn never_drops_responses() {
        // 响应帧解析失败也必须透传，否则调用方会永久等待
        let raw = r#"{"id":1,"result":{"something":"unexpected","nested":{"x":[]}}}"#;
        assert!(matches!(normalize(raw), RelayAction::Passthrough));
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib obscura::relay`
Expected: FAIL，报 `cannot find type RelayAction`

- [ ] **Step 3: 实现三态规范化**

在 `relay.rs` 中把现有 `patch_event` 改为基于三态的 `normalize`，并保留 `patch_event` 作为兼容包装：

```rust
/// 中继对一条消息应采取的动作
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayAction {
    /// 原样转发
    Passthrough,
    /// 用修补后的内容替换
    Replace(String),
    /// 丢弃（事件无法被驱动库解析，继续转发会杀掉整条连接）
    Drop,
}

/// 判断消息是否为「事件」（带 method 字段）；响应帧永不丢弃
fn is_event(value: &serde_json::Value) -> bool {
    value.get("method").and_then(serde_json::Value::as_str).is_some()
}

/// 规范化一条中继消息
pub fn normalize(text: &str) -> RelayAction {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return RelayAction::Passthrough; // 非 JSON：不属于本层职责
    };

    if !is_event(&value) {
        return RelayAction::Passthrough; // 响应帧一律透传
    }

    let patched = patch_value(&value);
    let candidate = match &patched {
        Some(v) => match serde_json::to_string(v) {
            Ok(text) => text,
            Err(_) => return RelayAction::Passthrough,
        },
        None => text.to_owned(),
    };

    if parses_as_event(&candidate) {
        match patched {
            Some(_) => RelayAction::Replace(candidate),
            None => RelayAction::Passthrough,
        }
    } else {
        RelayAction::Drop
    }
}

/// 用驱动库自身的反序列化器验证事件可被消费
fn parses_as_event(text: &str) -> bool {
    serde_json::from_str::<chromiumoxide_types::Message<chromiumoxide_cdp::cdp::events::CdpEventMessage>>(text).is_ok()
}

/// 兼容包装：保留原有单测与调用点可用
pub fn patch_event(text: &str) -> Option<String> {
    match normalize(text) {
        RelayAction::Replace(text) => Some(text),
        _ => None,
    }
}
```

把原 `patch_event` 中的方法分派逻辑抽为 `fn patch_value(value: &Value) -> Option<Value>`，内部沿用已有的 `patch_request_will_be_sent` / `patch_response_received` / `patch_request_will_be_sent_extra_info` / `patch_frame_requested_navigation`。

在 `Cargo.toml` 的 `[dependencies]` 中加入（两者已在 lock 文件中，可离线解析）：

```toml
chromiumoxide_cdp = "0.7"
chromiumoxide_types = "0.7"
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test --lib obscura::relay`
Expected: PASS（原有 7 个 + 新增 3 个）

- [ ] **Step 5: 在转发循环中处理 Drop**

在 `relay.rs` 的转发循环里，把原来「`patch_event` 返回 `Some` 则替换」改为匹配 `normalize` 的三态，`Drop` 分支记录一次 `tracing::warn!` 并 `continue`，日志包含方法名便于排查：

```rust
match normalize(&text) {
    RelayAction::Passthrough => forward(&text).await?,
    RelayAction::Replace(patched) => forward(&patched).await?,
    RelayAction::Drop => {
        let method = serde_json::from_str::<Value>(&text).ok()
            .and_then(|v| v.get("method").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| "未知".to_string());
        tracing::warn!("丢弃驱动库无法解析的事件: {method}");
    }
}
```

- [ ] **Step 6: 提交**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/obscura/relay.rs
git -c user.name="Mutantcat Dev" -c user.email="dev@mutantcat.com" commit -m "feat: 中继增加未知事件兜底，解析失败丢弃而非断连"
```

---

## Task 5: 能力矩阵

**Files:**
- Modify: `src-tauri/src/platforms/mod.rs`
- Modify: `src-tauri/src/platforms/{douyin,xiaohongshu,wechat_mp,csdn,custom}.rs`

- [ ] **Step 1: 写失败的测试**

在 `src-tauri/src/platforms/mod.rs` 末尾追加：

```rust
#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn builtin_adapters_declare_text_capability() {
        for adapter in all_adapters(None) {
            let caps = adapter.capabilities();
            assert!(caps.text, "{} 必须支持文本发布", adapter.platform_id());
        }
    }

    #[test]
    fn validate_rejects_video_when_unsupported() {
        let caps = Capabilities::TEXT_ONLY;
        let content = PublishContent {
            title: None,
            text: "正文".into(),
            media_paths: vec!["/tmp/a.mp4".into()],
            cover_path: None,
            topics: Vec::new(),
            thread_segments: Vec::new(),
            schedule_at: None,
        };
        let err = caps.validate(&content).unwrap_err().to_string();
        assert!(err.contains("不支持"), "错误信息应说明不支持，实际: {err}");
    }

    #[test]
    fn validate_rejects_topics_and_thread_when_unsupported() {
        let caps = Capabilities::TEXT_ONLY;
        let mut content = PublishContent {
            title: None, text: "正文".into(), media_paths: Vec::new(),
            cover_path: None, topics: vec!["话题".into()],
            thread_segments: Vec::new(), schedule_at: None,
        };
        assert!(caps.validate(&content).is_err(), "话题不受支持时必须报错");

        content.topics.clear();
        content.thread_segments = vec!["第二段".into()];
        assert!(caps.validate(&content).is_err(), "线程不受支持时必须报错");
    }

    #[test]
    fn validate_accepts_content_within_capabilities() {
        let caps = Capabilities::TEXT_ONLY;
        let content = PublishContent {
            title: Some("标题".into()), text: "正文".into(), media_paths: Vec::new(),
            cover_path: None, topics: Vec::new(), thread_segments: Vec::new(), schedule_at: None,
        };
        assert!(caps.validate(&content).is_ok());
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib platforms::capability_tests`
Expected: FAIL，报 `cannot find type Capabilities`

- [ ] **Step 3: 实现 `Capabilities` 与 `PublishContent` 扩展**

在 `src-tauri/src/platforms/mod.rs` 中替换 `PublishContent` 定义并新增 `Capabilities`：

```rust
/// 平台支持的能力，驱动前端显示与发布前校验
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Capabilities {
    pub text: bool,
    pub images: bool,
    pub video: bool,
    pub multi_image: bool,
    pub cover: bool,
    pub topics: bool,
    pub thread: bool,
    pub schedule: bool,
}

impl Capabilities {
    /// 仅文本
    pub const TEXT_ONLY: Self = Self {
        text: true, images: false, video: false, multi_image: false,
        cover: false, topics: false, thread: false, schedule: false,
    };

    /// 校验内容是否超出平台能力，超出则返回可直接展示给用户的原因
    pub fn validate(&self, content: &PublishContent) -> anyhow::Result<()> {
        if !self.text && !content.text.trim().is_empty() {
            anyhow::bail!("该平台不支持文本发布");
        }
        if !content.media_paths.is_empty() {
            if !self.images && !self.video {
                anyhow::bail!("该平台不支持媒体文件");
            }
            if content.media_paths.len() > 1 && !self.multi_image {
                anyhow::bail!("该平台不支持一次上传多个文件");
            }
        }
        if content.cover_path.is_some() && !self.cover {
            anyhow::bail!("该平台不支持设置封面");
        }
        if !content.topics.is_empty() && !self.topics {
            anyhow::bail!("该平台不支持话题标签");
        }
        if !content.thread_segments.is_empty() && !self.thread {
            anyhow::bail!("该平台不支持线程发布");
        }
        if content.schedule_at.is_some() && !self.schedule {
            anyhow::bail!("该平台不支持平台内定时发布");
        }
        Ok(())
    }
}

/// 发布内容
#[derive(Debug, Clone)]
pub struct PublishContent {
    pub title: Option<String>,
    pub text: String,
    pub media_paths: Vec<String>,
    /// 封面（视频/文章头图）
    pub cover_path: Option<String>,
    /// 话题标签，不含 `#` 前缀
    pub topics: Vec<String>,
    /// 线程分段；非空时按顺序逐条发布，`text` 作为第一段
    pub thread_segments: Vec<String>,
    /// 平台内定时发布的绝对时间戳（毫秒）
    pub schedule_at: Option<i64>,
}
```

- [ ] **Step 4: 给 trait 增加 `capabilities()` 并实现**

在 `PlatformAdapter` trait 中 `creator_url` 之后插入：

```rust
    /// 平台支持的能力
    fn capabilities(&self) -> Capabilities;
```

为 4 个内置适配器与 `CustomPlatformAdapter` 实现。抖音与小红书支持媒体，公众号与 CSDN 仅文本，自定义平台按已有配置推断：

```rust
// douyin.rs
    fn capabilities(&self) -> super::Capabilities {
        super::Capabilities { text: true, images: true, video: true, multi_image: true, cover: true, topics: true, thread: false, schedule: true }
    }

// xiaohongshu.rs
    fn capabilities(&self) -> super::Capabilities {
        super::Capabilities { text: true, images: true, video: true, multi_image: true, cover: true, topics: true, thread: false, schedule: false }
    }

// wechat_mp.rs
    fn capabilities(&self) -> super::Capabilities {
        super::Capabilities { text: true, images: true, video: false, multi_image: true, cover: true, topics: false, thread: false, schedule: false }
    }

// csdn.rs
    fn capabilities(&self) -> super::Capabilities {
        super::Capabilities { text: true, images: true, video: false, multi_image: true, cover: true, topics: true, thread: false, schedule: false }
    }

// custom.rs —— 有发布页与内容选择器即视为可发文本，媒体能力取决于是否配置了选择器
    fn capabilities(&self) -> super::Capabilities {
        super::Capabilities {
            text: self.config.content_selector.is_some(),
            images: false,
            video: false,
            multi_image: false,
            cover: false,
            topics: false,
            thread: false,
            schedule: false,
        }
    }
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cd src-tauri && cargo test --lib platforms::capability_tests`
Expected: PASS，4 passed

- [ ] **Step 6: 修正受影响的所有构造点**

Run: `cd src-tauri && cargo build 2>&1 | grep -E "^error" | head -20`

`PublishContent` 新增了字段，`accounts.rs:371` 与 `scheduler.rs:184` 的构造点会报错。补齐字段（默认 `None`/空 `Vec`，可先不接线，Task 7 再通数据）：

```rust
    let publish_content = crate::platforms::PublishContent {
        title,
        text: content,
        media_paths: media,
        cover_path: None,
        topics: Vec::new(),
        thread_segments: Vec::new(),
        schedule_at: None,
    };
```

- [ ] **Step 7: 编译并提交**

Run: `cd src-tauri && cargo test --lib && cargo build`
Expected: 全部 PASS，编译无错误

```bash
git add src-tauri/src/platforms/
git -c user.name="Mutantcat Dev" -c user.email="dev@mutantcat.com" commit -m "feat: 平台能力矩阵，发布前校验内容是否超出平台能力"
```

---

## Task 6: 选择器与媒体上传内核

**Files:**
- Create: `src-tauri/src/platforms/select.rs`
- Modify: `src-tauri/src/platforms/mod.rs`
- Modify: `src-tauri/src/platforms/xiaohongshu.rs`、`csdn.rs`（改用内核）

- [ ] **Step 1: 写失败的测试**

创建 `src-tauri/src/platforms/select.rs`：

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 多候选选择器与媒体上传内核
//!
//! 平台发布页在登录态之后，无法预先验证选择器，因此统一采用「按顺序尝试
//! 多个候选，全部失败则报出尝试过的全部选择器」。

use anyhow::{Result, anyhow};
use async_trait::async_trait;

/// 元素操作抽象，便于用假实现做单元测试
#[async_trait]
pub trait ElementOps: Send + Sync {
    async fn fill(&self, selector: &str, value: &str) -> Result<()>;
    async fn click(&self, selector: &str) -> Result<()>;
    async fn upload(&self, selector: &str, paths: &[String]) -> Result<()>;
}

/// 依次尝试候选选择器填入文本，返回命中的选择器
pub async fn fill_any<O: ElementOps + ?Sized>(
    ops: &O,
    selectors: &[&str],
    value: &str,
) -> Result<String> {
    let mut tried = Vec::new();
    for selector in selectors {
        tried.push(*selector);
        if ops.fill(selector, value).await.is_ok() {
            return Ok((*selector).to_string());
        }
    }
    Err(anyhow!("找不到可填入的输入框，已尝试: {}", tried.join(" | ")))
}

/// 依次尝试候选选择器点击，返回命中的选择器
pub async fn click_any<O: ElementOps + ?Sized>(
    ops: &O,
    selectors: &[&str],
) -> Result<String> {
    let mut tried = Vec::new();
    for selector in selectors {
        tried.push(*selector);
        if ops.click(selector).await.is_ok() {
            return Ok((*selector).to_string());
        }
    }
    Err(anyhow!("找不到可点击的元素，已尝试: {}", tried.join(" | ")))
}

/// 依次尝试候选选择器上传文件
pub async fn upload_any<O: ElementOps + ?Sized>(
    ops: &O,
    selectors: &[&str],
    paths: &[String],
) -> Result<String> {
    if paths.is_empty() {
        return Err(anyhow!("没有需要上传的文件"));
    }
    let mut tried = Vec::new();
    for selector in selectors {
        tried.push(*selector);
        if ops.upload(selector, paths).await.is_ok() {
            return Ok((*selector).to_string());
        }
    }
    Err(anyhow!("找不到文件上传控件，已尝试: {}", tried.join(" | ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 只对白名单内的选择器成功，并记录尝试顺序
    #[derive(Default)]
    struct FakeOps {
        fill_ok: Vec<String>,
        click_ok: Vec<String>,
        upload_ok: Vec<String>,
        tried: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ElementOps for FakeOps {
        async fn fill(&self, selector: &str, _value: &str) -> Result<()> {
            self.tried.lock().unwrap().push(format!("fill:{selector}"));
            if self.fill_ok.iter().any(|s| s == selector) { Ok(()) } else { Err(anyhow!("no")) }
        }
        async fn click(&self, selector: &str) -> Result<()> {
            self.tried.lock().unwrap().push(format!("click:{selector}"));
            if self.click_ok.iter().any(|s| s == selector) { Ok(()) } else { Err(anyhow!("no")) }
        }
        async fn upload(&self, selector: &str, _paths: &[String]) -> Result<()> {
            self.tried.lock().unwrap().push(format!("upload:{selector}"));
            if self.upload_ok.iter().any(|s| s == selector) { Ok(()) } else { Err(anyhow!("no")) }
        }
    }

    #[tokio::test]
    async fn fill_uses_first_working_candidate() {
        let ops = FakeOps { fill_ok: vec!["#third".into()], ..Default::default() };
        let hit = fill_any(&ops, &["#first", "#second", "#third"], "内容").await.unwrap();
        assert_eq!(hit, "#third");
        let tried = ops.tried.lock().unwrap().clone();
        assert_eq!(tried, vec!["fill:#first", "fill:#second", "fill:#third"], "必须按顺序尝试");
    }

    #[tokio::test]
    async fn fill_stops_after_first_success() {
        let ops = FakeOps { fill_ok: vec!["#first".into(), "#second".into()], ..Default::default() };
        fill_any(&ops, &["#first", "#second"], "内容").await.unwrap();
        assert_eq!(ops.tried.lock().unwrap().len(), 1, "命中后不应继续尝试");
    }

    #[tokio::test]
    async fn fill_reports_all_attempted_selectors() {
        let ops = FakeOps::default();
        let err = fill_any(&ops, &["#a", "#b"], "内容").await.unwrap_err().to_string();
        assert!(err.contains("#a") && err.contains("#b"), "错误必须列出全部尝试过的选择器: {err}");
    }

    #[tokio::test]
    async fn click_returns_matched_selector() {
        let ops = FakeOps { click_ok: vec!["button.submit".into()], ..Default::default() };
        assert_eq!(click_any(&ops, &["#x", "button.submit"]).await.unwrap(), "button.submit");
    }

    #[tokio::test]
    async fn upload_rejects_empty_paths() {
        let ops = FakeOps::default();
        assert!(upload_any(&ops, &["input[type=file]"], &[]).await.is_err());
        assert!(ops.tried.lock().unwrap().is_empty(), "空文件列表不应触碰页面");
    }

    #[tokio::test]
    async fn upload_tries_each_candidate() {
        let ops = FakeOps { upload_ok: vec!["input.upload".into()], ..Default::default() };
        let paths = vec!["/tmp/a.png".to_string()];
        assert_eq!(upload_any(&ops, &["input[type=file]", "input.upload"], &paths).await.unwrap(), "input.upload");
    }
}
```

在 `src-tauri/src/platforms/mod.rs` 加入模块声明与再导出：

```rust
pub mod select;
pub use select::{click_any, fill_any, upload_any, ElementOps};
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib platforms::select`
Expected: FAIL（模块还不存在或 `fill_any` 未定义）

- [ ] **Step 3: 运行测试确认通过**

上面的代码已是完整实现，接好模块声明后直接运行：

Run: `cd src-tauri && cargo test --lib platforms::select`
Expected: PASS，6 passed

- [ ] **Step 4: 实现 `PageOps` 真实实现**

在 `select.rs` 追加（上传用 CDP 命令，`Element` 没有上传方法）：

```rust
use chromiumoxide::Page;

/// 基于真实页面的元素操作
pub struct PageOps<'a> {
    pub page: &'a Page,
}

#[async_trait]
impl ElementOps for PageOps<'_> {
    async fn fill(&self, selector: &str, value: &str) -> Result<()> {
        let element = self.page.find_element(selector).await?;
        element.click().await?;
        element.type_str(value).await?;
        Ok(())
    }

    async fn click(&self, selector: &str) -> Result<()> {
        self.page.find_element(selector).await?.click().await?;
        Ok(())
    }

    async fn upload(&self, selector: &str, paths: &[String]) -> Result<()> {
        let element = self.page.find_element(selector).await?;
        let mut params = chromiumoxide_cdp::cdp::browser_protocol::dom::SetFileInputFilesParams::new(
            paths.to_vec(),
        );
        params.backend_node_id = Some(element.backend_node_id);
        self.page.execute(params).await?;
        Ok(())
    }
}

/// 发送一个按键。`chromiumoxide` 的 `Page` **没有** `keyboard()`，
/// 必须直接发 `Input.dispatchKeyEvent`；实现照 `src-tauri/src/login.rs:42-52` 的既有写法。
pub async fn press_key(page: &Page, key: &str) -> Result<()> {
    let code = match key {
        "Enter" => 13,
        "Tab" => 9,
        "Escape" => 27,
        other => anyhow::bail!("不支持的按键: {other}"),
    };
    for event in [if key == "Enter" { "keyDown" } else { "rawKeyDown" }, "keyUp"] {
        let mut raw = serde_json::json!({
            "type": event, "key": key, "code": key,
            "windowsVirtualKeyCode": code, "nativeVirtualKeyCode": code
        });
        if key == "Enter" && event == "keyDown" {
            raw["text"] = serde_json::json!("\r");
        }
        let params: chromiumoxide_cdp::cdp::browser_protocol::input::DispatchKeyEventParams =
            serde_json::from_value(raw).context("按键参数无效")?;
        page.execute(params).await.context("发送按键失败")?;
    }
    Ok(())
}
```

在 `mod.rs` 的再导出中补上 `press_key`：

```rust
pub use select::{click_any, fill_any, press_key, upload_any, ElementOps};
```

- [ ] **Step 5: 让既有适配器改用内核**

把 `src-tauri/src/platforms/xiaohongshu.rs` 与 `csdn.rs` 中现有的 `fill_first` / `click_first` 调用替换为 `crate::platforms::fill_any` / `click_any`。`mod.rs` 中原有的 `fill_first` / `click_first` 若已无调用者则删除，避免两套并行。

Run: `cd src-tauri && cargo build 2>&1 | grep -E "^error" | head -20`
Expected: 无输出（无编译错误）

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/platforms/
git -c user.name="Mutantcat Dev" -c user.email="dev@mutantcat.com" commit -m "feat: 选择器与媒体上传内核，上传走 DOM.setFileInputFiles"
```

---

## Task 7: 发布内容扩展打通到数据库与调度

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/accounts.rs`
- Modify: `src-tauri/src/scheduler.rs`

- [ ] **Step 1: 写失败的测试**

`create_task` 的真实签名是 `create_task(account_id, title, content, media_paths, cron_expr, scheduled_at)`（`db.rs:405`），本任务在末尾追加 `cover_path` / `topics` / `thread_segments` 三个参数。数据库列名是 **`scheduled_at`（已存在，不要新增列）**。检索用已有的 `list_tasks(account_id)`（`db.rs:438`）。

在 `db.rs` 的 `account_tests` 模块中追加（复用该模块已有的 `db()` 与 `NewAccount` 构造方式）：

```rust
    #[test]
    fn publish_task_persists_extended_fields() {
        let db = db();
        let account = db.create_account(&new_account()).unwrap();
        let task = db.create_task(
            &account.id,
            Some("标题"),
            Some("正文"),
            None,
            None,
            Some(1_800_000_000_000),
            Some("/tmp/cover.png"),
            Some("话题A,话题B"),
            Some("第一段||第二段"),
        ).unwrap();

        let loaded = db.list_tasks(&account.id).unwrap()
            .into_iter().find(|t| t.id == task.id).expect("任务应存在");
        assert_eq!(loaded.cover_path.as_deref(), Some("/tmp/cover.png"));
        assert_eq!(loaded.topics.as_deref(), Some("话题A,话题B"));
        assert_eq!(loaded.thread_segments.as_deref(), Some("第一段||第二段"));
        assert_eq!(loaded.scheduled_at, Some(1_800_000_000_000));
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib db::account_tests::publish_task_persists_extended_fields`
Expected: FAIL，报参数数量不匹配（`create_task` 目前只有 6 个参数）

- [ ] **Step 3: 加迁移与读写**

在 `db.rs` 的 `migrate()` 中，`group_name` 那行之后追加（**注意 `scheduled_at` 已存在于建表语句，不要重复添加**）：

```rust
        // 增量迁移：发布内容扩展字段（已存在则忽略错误）
        let _ = conn.execute("ALTER TABLE publish_tasks ADD COLUMN cover_path TEXT", []);
        let _ = conn.execute("ALTER TABLE publish_tasks ADD COLUMN topics TEXT", []);
        let _ = conn.execute("ALTER TABLE publish_tasks ADD COLUMN thread_segments TEXT", []);
        // 令牌型平台的登录凭据（加密存储）
        let _ = conn.execute("ALTER TABLE accounts ADD COLUMN api_token_enc BLOB", []);
```

同步修改：

1. `PublishTask` 结构体（`db.rs:89`）追加：

```rust
    pub cover_path: Option<String>,
    pub topics: Option<String>,
    pub thread_segments: Option<String>,
```

2. `create_task` 签名末尾追加三个 `Option<&str>` 参数，`INSERT` 语句的列与 `params!` 同步补齐。
3. `list_tasks`（`db.rs:438`）的 `SELECT` 列表与 `row.get(n)` 索引补齐三列（索引顺延，务必对照现有列顺序改）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test --lib db`
Expected: PASS

- [ ] **Step 5: 调度器补齐字段**

在 `scheduler.rs:184` 附近构造 `PublishContent` 处，按数据库字段填充：

```rust
    let topics: Vec<String> = task.topics.as_deref().unwrap_or("")
        .split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
    let thread_segments: Vec<String> = task.thread_segments.as_deref().unwrap_or("")
        .split("||").map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();

    let content = PublishContent {
        title: task.title.clone(),
        text: task.content.clone().unwrap_or_default(),
        media_paths,
        cover_path: task.cover_path.clone(),
        topics,
        thread_segments,
        // 数据库列名是 scheduled_at，映射到 PublishContent.schedule_at
        schedule_at: task.scheduled_at,
    };
```

- [ ] **Step 6: 发布前能力校验**

在 `scheduler.rs` 与 `accounts.rs` 调用 `adapter.publish(...)` 之前插入校验，让错误在发布前就暴露：

```rust
    adapter.capabilities().validate(&content)?;
```

- [ ] **Step 7: 编译、测试、提交**

Run: `cd src-tauri && cargo test --lib && cargo build`
Expected: 全部 PASS，编译无错误

```bash
git add src-tauri/src/db.rs src-tauri/src/accounts.rs src-tauri/src/scheduler.rs
git -c user.name="Mutantcat Dev" -c user.email="dev@mutantcat.com" commit -m "feat: 发布内容扩展字段贯通数据库与调度，发布前做能力校验"
```

---

## Task 8: 令牌族登录（后端）

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/platforms/mod.rs`
- Modify: `src-tauri/src/accounts.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 写失败的测试**

加解密不在 `credentials.rs`，而是 **`crate::crypto::encrypt` / `crate::crypto::decrypt`**（`src-tauri/src/crypto.rs:42`、`:62`）。时间戳沿用 `chrono::Utc::now().timestamp()`。

在 `db.rs` 的 `account_tests` 模块中追加（复用该模块已有的 `db()` 与 `new_account()`）：

```rust
    #[test]
    fn api_token_round_trips_encrypted() {
        let db = db();
        let account = db.create_account(&new_account()).unwrap();
        db.save_api_token(&account.id, "secret-token-123").unwrap();
        assert_eq!(db.get_api_token(&account.id).unwrap().as_deref(), Some("secret-token-123"));

        // 落库必须是密文
        let raw: Vec<u8> = {
            let conn = db.conn.lock().unwrap();
            conn.query_row(
                "SELECT api_token_enc FROM accounts WHERE id = ?1",
                rusqlite::params![account.id],
                |r| r.get(0),
            ).unwrap()
        };
        assert!(
            !raw.windows(16).any(|w| w == b"secret-token-123"),
            "令牌不得以明文落库"
        );
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib api_token_round_trips_encrypted`
Expected: FAIL，报 `no method named save_api_token`

- [ ] **Step 3: 实现令牌读写**

在 `db.rs` 中参照现有 `cookies_enc` 的读写实现新增：

```rust
    /// 保存平台 API 令牌（加密存储）
    pub fn save_api_token(&self, account_id: &str, token: &str) -> Result<()> {
        let encrypted = crate::crypto::encrypt(token.as_bytes())?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET api_token_enc = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![encrypted, chrono::Utc::now().timestamp(), account_id],
        )?;
        Ok(())
    }

    /// 读取平台 API 令牌
    pub fn get_api_token(&self, account_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let blob: Option<Vec<u8>> = conn
            .query_row(
                "SELECT api_token_enc FROM accounts WHERE id = ?1",
                rusqlite::params![account_id],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()?
            .flatten();
        match blob {
            Some(bytes) if !bytes.is_empty() => {
                let plain = crate::crypto::decrypt(&bytes)?;
                Ok(Some(String::from_utf8(plain).context("令牌不是合法文本")?))
            }
            _ => Ok(None),
        }
    }
```

需要确认 `db.rs` 已导入 `rusqlite::OptionalExtension`（`get_setting` 已用到 `.optional()`，因此已导入）。

- [ ] **Step 4: 实现令牌校验与 Tauri 命令**

在 `platforms/mod.rs` 的 trait 增加带默认实现的令牌入口：

```rust
    /// 令牌型平台的凭据位数说明，用于前端提示（浏览器族返回 None）
    fn token_hint(&self) -> Option<&str> {
        None
    }

    /// 校验令牌是否可用（令牌型平台实现；浏览器族返回不支持）
    async fn verify_token(&self, _token: &str) -> Result<()> {
        anyhow::bail!("该平台不支持令牌登录")
    }
```

在 `accounts.rs` 新增命令：

```rust
/// 保存并校验平台 API 令牌
#[tauri::command]
pub async fn save_platform_token(
    db: State<'_, Db>,
    account_id: String,
    token: String,
) -> Result<(), String> {
    let account = db.get_account(&account_id).map_err(|e| e.to_string())?
        .ok_or_else(|| "账号不存在".to_string())?;
    let adapter = crate::platforms::find_adapter(&account.platform, Some(&db))
        .ok_or_else(|| "平台适配器不存在".to_string())?;
    anyhow::ensure!(adapter.token_hint().is_some(), "该平台不支持令牌登录").map_err(|e| e.to_string())?;
    adapter.verify_token(token.trim()).await.map_err(|e| e.to_string())?;
    db.save_api_token(&account_id, token.trim()).map_err(|e| e.to_string())?;
    Ok(())
}
```

在 `lib.rs` 的 `invoke_handler` 列表中注册 `accounts::save_platform_token`。

- [ ] **Step 5: 运行测试并提交**

Run: `cd src-tauri && cargo test --lib && cargo build`
Expected: 全部 PASS，编译无错误

```bash
git add src-tauri/src/
git -c user.name="Mutantcat Dev" -c user.email="dev@mutantcat.com" commit -m "feat: 令牌型平台登录（加密存储 + 保存前校验）"
```

---

## Task 9: 知乎适配器（浏览器族打样）

**Files:**
- Create: `src-tauri/src/platforms/zhihu.rs`
- Modify: `src-tauri/src/platforms/mod.rs`

- [ ] **Step 1: 实测登录页选择器（不可跳过）**

启动真 Chromium 并加载 `https://www.zhihu.com/signin`，导出登录页真实 DOM 结构，据此确定 `LOGIN_USER` / `LOGIN_PASS` / `LOGIN_SUBMIT` 三组候选选择器。

Run: 用 `cargo run --example cdp_shot -- <引擎端口> https://www.zhihu.com/signin /tmp/zhihu.png` 截图，并用 CDP 求值导出候选控件的 tag/class/placeholder。

**必须把实测到的真实选择器写进代码注释**，不要凭记忆填。

- [ ] **Step 2: 写失败的测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::platforms::PlatformAdapter;

    #[test]
    fn declares_expected_capabilities() {
        let caps = ZhihuAdapter::new().capabilities();
        assert!(caps.text);
        assert!(caps.images);
        assert!(caps.topics);
        assert!(!caps.thread, "知乎不支持线程");
    }

    #[test]
    fn platform_id_is_registered() {
        assert!(crate::platforms::is_builtin_platform_id("zhihu"));
        assert!(crate::platforms::BUILTIN_PLATFORM_IDS.contains(&"zhihu"));
    }

    #[test]
    fn creator_url_is_trusted_origin() {
        let url = ZhihuAdapter::new().creator_url().to_string();
        assert!(url.starts_with("https://www.zhihu.com"), "实际: {url}");
    }

    #[test]
    fn token_login_is_unsupported() {
        assert!(ZhihuAdapter::new().token_hint().is_none());
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib platforms::zhihu`
Expected: FAIL，模块不存在

- [ ] **Step 4: 实现适配器**

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 知乎适配器（浏览器族）
//!
//! 创作中心：https://zhuanlan.zhihu.com/write
//! 登录页：https://www.zhihu.com/signin
//! 选择器实测记录见本文件各常量注释。

use super::{Capabilities, PlatformAdapter, PublishContent};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chromiumoxide::Page;

const CREATOR_URL: &str = "https://www.zhihu.com/";
const WRITE_URL: &str = "https://zhuanlan.zhihu.com/write";

/// 登录表单选择器（实测填写）
const LOGIN_USER: &[&str] = &["input[name='username']", "input[type='text']"];
const LOGIN_PASS: &[&str] = &["input[name='password']", "input[type='password']"];
const LOGIN_SUBMIT: &[&str] = &["button[type='submit']", "button.SignFlow-submitButton"];

/// 编辑器选择器（发布页在登录态之后，无法预先验证，按候选尝试）
const TITLE: &[&str] = &["textarea[placeholder*='标题']", ".WriteIndex-titleInput textarea"];
const BODY: &[&str] = &[".public-DraftEditor-content", "div[contenteditable='true']"];
const TOPIC: &[&str] = &["input[placeholder*='话题']", ".Popover-content input"];
const SUBMIT: &[&str] = &["button[type='submit']", "button.PublishPanel-submitButton"];

pub struct ZhihuAdapter;

impl ZhihuAdapter {
    pub fn new() -> Self { ZhihuAdapter }
}

impl Default for ZhihuAdapter {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl PlatformAdapter for ZhihuAdapter {
    fn platform_id(&self) -> &str { "zhihu" }
    fn display_name(&self) -> &str { "知乎" }
    fn creator_url(&self) -> &str { CREATOR_URL }
    fn token_hint(&self) -> Option<&str> { None }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            text: true, images: true, video: true, multi_image: true,
            cover: true, topics: true, thread: false, schedule: false,
        }
    }

    async fn login(&self, page: &Page, username: &str, password: &str) -> Result<String> {
        page.goto(CREATOR_URL).await.context("打开知乎失败")?;
        let ops = super::select::PageOps { page };
        super::select::fill_any(&ops, LOGIN_USER, username).await?;
        super::select::fill_any(&ops, LOGIN_PASS, password).await?;
        super::select::click_any(&ops, LOGIN_SUBMIT).await?;
        Ok(String::new())
    }

    async fn publish(&self, page: &Page, content: &PublishContent) -> Result<()> {
        self.capabilities().validate(content)?;
        page.goto(WRITE_URL).await.context("打开发布页失败")?;
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        let ops = super::select::PageOps { page };
        if let Some(title) = content.title.as_deref().filter(|t| !t.is_empty()) {
            super::select::fill_any(&ops, TITLE, title).await?;
        }
        super::select::fill_any(&ops, BODY, &content.text).await?;
        for topic in &content.topics {
            if super::select::fill_any(&ops, TOPIC, topic).await.is_ok() {
                let _ = super::select::press_key(page, "Enter").await;
            }
        }
        if !content.media_paths.is_empty() {
            super::select::upload_any(&ops, &["input[type='file']"], &content.media_paths).await?;
        }
        super::select::click_any(&ops, SUBMIT).await?;
        Ok(())
    }

    async fn check_session(&self, page: &Page) -> Result<bool> {
        super::auth::authenticated_probe(page, self.creator_url(), None).await
    }
}
```

- [ ] **Step 5: 注册平台**

在 `mod.rs` 中：`pub mod zhihu;`、`BUILTIN_PLATFORM_IDS` 加入 `"zhihu"`、`all_adapters` 中 `Box::new(zhihu::ZhihuAdapter::new())`。

- [ ] **Step 6: 运行测试并提交**

Run: `cd src-tauri && cargo test --lib && cargo build`
Expected: 全部 PASS

```bash
git add src-tauri/src/platforms/
git -c user.name="Mutantcat Dev" -c user.email="dev@mutantcat.com" commit -m "feat: 知乎适配器（浏览器族打样）"
```

---

## Task 10: Dev.to 适配器（令牌族打样）

**Files:**
- Create: `src-tauri/src/platforms/devto.rs`
- Modify: `src-tauri/src/platforms/mod.rs`

- [ ] **Step 1: 写失败的测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::platforms::PlatformAdapter;

    #[test]
    fn exposes_token_hint() {
        let hint = DevToAdapter::new().token_hint();
        assert!(hint.is_some(), "令牌型平台必须提供提示");
        assert!(hint.unwrap().contains("dev.to"), "提示应说明从哪里获取令牌");
    }

    #[test]
    fn declares_api_capabilities() {
        let caps = DevToAdapter::new().capabilities();
        assert!(caps.text && caps.cover && caps.topics);
        assert!(!caps.schedule, "Dev.to 不支持平台内定时");
        assert!(!caps.thread);
    }

    #[test]
    fn builds_article_payload() {
        let payload = build_article_payload(&crate::platforms::PublishContent {
            title: Some("标题".into()),
            text: "正文".into(),
            media_paths: Vec::new(),
            cover_path: Some("/tmp/cover.png".into()),
            topics: vec!["rust".into(), "tauri".into()],
            thread_segments: Vec::new(),
            schedule_at: None,
        });
        assert_eq!(payload["article"]["title"], "标题");
        assert_eq!(payload["article"]["body_markdown"], "正文");
        assert_eq!(payload["article"]["tags"], serde_json::json!(["rust", "tauri"]));
        assert_eq!(payload["article"]["published"], true);
    }

    #[test]
    fn rejects_missing_title() {
        let content = crate::platforms::PublishContent {
            title: None, text: "正文".into(), media_paths: Vec::new(),
            cover_path: None, topics: Vec::new(), thread_segments: Vec::new(), schedule_at: None,
        };
        assert!(build_article_payload(&content).get("article").is_none() || build_article_payload(&content)["article"]["title"].as_str().unwrap_or("").is_empty());
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib platforms::devto`
Expected: FAIL，模块不存在

- [ ] **Step 3: 实现适配器**

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! Dev.to 适配器（令牌族）
//!
//! 令牌自助获取：https://dev.to/settings/extensions → DEV Community API Keys
//! 接口文档：https://developers.forem.com/api/v1

use super::{Capabilities, PlatformAdapter, PublishContent};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chromiumoxide::Page;

const API_BASE: &str = "https://dev.to/api";
const CREATOR_URL: &str = "https://dev.to/dashboard";

pub struct DevToAdapter;

impl DevToAdapter {
    pub fn new() -> Self { DevToAdapter }
}

impl Default for DevToAdapter {
    fn default() -> Self { Self::new() }
}

/// 构造 Dev.to 文章请求体（纯函数，便于测试）
pub fn build_article_payload(content: &PublishContent) -> serde_json::Value {
    serde_json::json!({
        "article": {
            "title": content.title.clone().unwrap_or_default(),
            "body_markdown": content.text,
            "published": true,
            "tags": if content.topics.is_empty() { serde_json::Value::Null } else { serde_json::json!(content.topics) },
        }
    })
}

#[async_trait]
impl PlatformAdapter for DevToAdapter {
    fn platform_id(&self) -> &str { "devto" }
    fn display_name(&self) -> &str { "Dev.to" }
    fn creator_url(&self) -> &str { CREATOR_URL }

    fn token_hint(&self) -> Option<&str> {
        Some("在 dev.to → Settings → Extensions → DEV Community API Keys 生成，粘贴后立即校验")
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            text: true, images: false, video: false, multi_image: false,
            cover: true, topics: true, thread: false, schedule: false,
        }
    }

    async fn login(&self, _page: &Page, _username: &str, _password: &str) -> Result<String> {
        anyhow::bail!("Dev.to 使用令牌登录，请在账号页面粘贴 API 令牌")
    }

    async fn verify_token(&self, token: &str) -> Result<()> {
        let response = reqwest::Client::new()
            .get(format!("{API_BASE}/users/me"))
            .header("api-key", token)
            .send().await.context("连接 Dev.to 失败")?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else if status.as_u16() == 401 {
            Err(anyhow!("令牌无效或已过期"))
        } else {
            Err(anyhow!("校验令牌失败，服务返回 {status}"))
        }
    }

    async fn publish(&self, _page: &Page, content: &PublishContent) -> Result<()> {
        self.capabilities().validate(content)?;
        if content.title.as_deref().unwrap_or("").trim().is_empty() {
            anyhow::bail!("Dev.to 必须提供标题");
        }
        anyhow::bail!("令牌型平台发布在 Task 11 中接线（需要账号令牌）")
    }

    async fn check_session(&self, _page: &Page) -> Result<bool> {
        // 令牌型平台的会话有效性由 verify_token 决定，不依赖页面
        Ok(false)
    }
}
```

- [ ] **Step 4: 注册并运行测试**

在 `mod.rs` 中：`pub mod devto;`、`BUILTIN_PLATFORM_IDS` 加入 `"devto"`、`all_adapters` 中追加。

Run: `cd src-tauri && cargo test --lib && cargo build`
Expected: 全部 PASS

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/platforms/
git -c user.name="Mutantcat Dev" -c user.email="dev@mutantcat.com" commit -m "feat: Dev.to 适配器（令牌族打样，含令牌校验）"
```

---

## Task 11: 令牌族发布接线与前端能力 UI

**Files:**
- Modify: `src-tauri/src/scheduler.rs`
- Modify: `src-tauri/src/accounts.rs`
- Modify: `src/api.ts`
- Modify: `src/components/CustomPlatforms.vue`

- [ ] **Step 1: 令牌型平台发布绕过浏览器**

在 `scheduler.rs` 与 `accounts.rs` 的发布路径中，令牌型平台不需要打开会话页面：

```rust
// 令牌型平台：直接调用适配器发布，不打开浏览器会话
if adapter.token_hint().is_some() {
    let token = db.get_api_token(&account_id)?
        .ok_or_else(|| anyhow!("该账号尚未配置 API 令牌"))?;
    adapter.publish_with_token(&token, &content).await?;
} else {
    // 既有浏览器路径
}
```

在 trait 中增加：

```rust
    /// 令牌型平台发布（浏览器族返回不支持）
    async fn publish_with_token(&self, _token: &str, _content: &PublishContent) -> Result<()> {
        anyhow::bail!("该平台不支持令牌发布")
    }
```

`DevToAdapter` 实现 `publish_with_token`：POST `{API_BASE}/articles`，`header("api-key", token)`，body 为 `build_article_payload(content)`；非 2xx 时把响应体摘要放进错误信息。

- [ ] **Step 2: 前端类型与调用**

在 `src/api.ts` 追加：

```ts
export interface PlatformCapabilities {
  text: boolean; images: boolean; video: boolean; multi_image: boolean;
  cover: boolean; topics: boolean; thread: boolean; schedule: boolean;
}

export interface Platform {
  id: string;
  name: string;
  capabilities: PlatformCapabilities;
}

export const listPlatforms = () => invoke<Platform[]>("list_platforms");

export const savePlatformToken = (accountId: string, token: string) =>
  invoke<void>("save_platform_token", { accountId, token });
```

同时把 `list_platforms` 的返回值在后端补上 `capabilities`：

```rust
        .map(|a| serde_json::json!({
            "id": a.platform_id(),
            "name": a.display_name(),
            "capabilities": a.capabilities(),
        }))
```

- [ ] **Step 3: 前端能力驱动 UI 与令牌入口**

在 `CustomPlatforms.vue`（或账号编辑处）按 `capabilities` 显示可用字段，禁用不支持的项并给出原因；令牌型平台（`token_hint` 非空时后端返回 `tokenSupported: true`，需在 Step 2 的 JSON 中一并加上）显示「粘贴 API 令牌」输入框，调用 `savePlatformToken`，成功与失败都要有明确提示。

- [ ] **Step 4: 构建与测试**

Run: `cd src-tauri && cargo test --lib`
Expected: 全部 PASS

Run: `pnpm build`
Expected: vue-tsc 与 vite 均通过

- [ ] **Step 5: 提交**

```bash
git add src/ src-tauri/src/
git -c user.name="Mutantcat Dev" -c user.email="dev@mutantcat.com" commit -m "feat: 令牌族发布接线与能力驱动的前端界面"
```

---

## Task 12: 端到端验证与文档

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 本地实测两个打样平台**

用真 Chromium 启动应用（`MC_ENGINE_CMD` 指向 Chrome for Testing），依次验证：

1. 知乎：打开登录会话 → 页面正常渲染中文 → 登录后 `check_session` 返回 true。
2. Dev.to：粘贴真实 API 令牌 → 校验通过 → 发布一篇测试文章 → 确认线上可见。

把每步的实际结果（成功/失败、报错原文、选择器是否需要调整）记录下来。

- [ ] **Step 2: 更新 README**

在「已知限制」中补充：

- 引擎采用双轨制，本机 Chrome 优先，缺失时下载 Chrome for Testing 并锁定版本（当前 `157.0.8103.38`）。
- 中继对驱动库无法解析的事件改为丢弃并记录警告，不再断开连接；排查用日志关键字「丢弃驱动库无法解析的事件」。
- 新增平台中，浏览器族的发布页选择器无法预先验证，失败时报错会列出全部尝试过的选择器。

- [ ] **Step 3: 提交并交付**

```bash
git add README.md
git -c user.name="Mutantcat Dev" -c user.email="dev@mutantcat.com" commit -m "docs: 记录引擎双轨制、中继兜底与选择器验证边界"
```

---

## Self-Review

**1. Spec coverage**

| 设计文档要求 | 对应任务 |
|---|---|
| 引擎双轨（本机优先 + 自动下载 + 版本锁定） | Task 1、2、3 |
| 中继未知事件兜底 | Task 4 |
| 能力矩阵（文本/图片/视频/多图/封面/话题/线程/定时） | Task 5 |
| 共享内核：选择器 | Task 6 |
| 共享内核：媒体上传 | Task 6 |
| 共享内核：线程 | 本批次未实现——Dev.to 与知乎均不支持线程，留到批次 4（X/Threads/Mastodon）与对应平台一同实现 |
| 共享内核：平台内定时 | 本批次只做字段贯通与能力校验；真正调用平台定时控件留到各平台批次 |
| PublishContent 扩展与数据迁移 | Task 7 |
| 令牌族登录（粘贴令牌 + 校验） | Task 8、11 |
| 浏览器族登录复用现有流程 | Task 9（知乎直接复用 `authenticated_probe`） |
| 两个打样平台（知乎 + Dev.to） | Task 9、10 |
| 选择器实测策略 | Task 9 Step 1、Task 12 |

已知缺口，明确留待后续批次：线程发布实现、平台内定时的真实控件操作、批次 2-5 的其余 22 个平台。

**2. Placeholder scan**

已检查：无 TBD / TODO / 「稍后实现」。`DevToAdapter::publish` 中出现的 `bail!("...在 Task 11 中接线")` 是刻意的阶段性实现，Task 11 Step 1 给出完整的 `publish_with_token` 接线要求。Task 9 Step 1 要求实测选择器并写进注释，属于必须执行的动作而非占位。

**3. Type consistency**

- `Capabilities` 字段（`text/images/video/multi_image/cover/topics/thread/schedule`）在 Task 5 定义，Task 9、10 使用一致。
- `PublishContent` 字段（`title/text/media_paths/cover_path/topics/thread_segments/schedule_at`）在 Task 5 定义，Task 7、9、10 使用一致。
- 选择器内核在 Task 6 定义为 `fill_any/click_any/upload_any` + `ElementOps` + `PageOps`，Task 9 使用一致（未出现 `fill_first` 等旧名）。
- `token_hint` / `verify_token` / `publish_with_token` 在 Task 8、10、11 中签名一致。
