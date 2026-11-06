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

/// 探测本机已安装的 Chrome
pub fn find_system_chrome() -> Option<PathBuf> {
    first_existing(&system_chrome_paths(), |p| p.is_file())
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
