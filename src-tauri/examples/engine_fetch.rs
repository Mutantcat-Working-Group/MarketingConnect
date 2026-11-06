// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 端到端验证引擎解析链的「下载 + 解压」部分。
//!
//! 这是唯一能真正验证解压路径是否正确的手段：各平台的 zip 顶层目录名
//! （如 macOS 是 `chrome-mac-arm64` 而非 `chrome-mac`）只有在真实解压后
//! 才能确认，单元测试无法覆盖。
//!
//! 用法: cargo run --example engine_fetch -- [数据目录]
//! 默认数据目录: /tmp/mc-engine-fetch

use anyhow::{Context, Result};
use marketing_connect_lib::obscura::engine;

#[tokio::main]
async fn main() -> Result<()> {
    let data_dir = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "/tmp/mc-engine-fetch".to_string()),
    );

    let key = engine::chrome_for_testing_platform_key().context("当前平台不受支持")?;
    println!("平台键: {key}");

    let feed: serde_json::Value = reqwest::get(engine::CFT_VERSIONS_URL)
        .await
        .context("获取版本清单失败")?
        .json()
        .await
        .context("解析版本清单失败")?;

    let version = engine::stable_version(&feed).context("清单中缺少版本号")?;
    let url = engine::stable_download_url(&feed, key).context("清单中缺少当前平台的下载地址")?;
    println!("版本: {version}");
    println!("地址: {url}");

    let expected = engine::installed_chrome_path(&data_dir, &version)
        .context("无法推导安装路径")?;
    let cached = expected.is_file();
    println!("期望位置: {}", expected.display());
    println!("是否已缓存: {cached}");

    let started = std::time::Instant::now();
    let exe = engine::download_chrome(&data_dir, &url, &version).await?;
    println!("耗时: {:.1}s", started.elapsed().as_secs_f64());
    println!("实际位置: {}", exe.display());

    anyhow::ensure!(exe == expected, "实际解压位置与推导位置不一致");

    // 真跑一次，证明拿到的是可执行的浏览器而不是空壳
    let output = std::process::Command::new(&exe)
        .arg("--version")
        .output()
        .context("执行引擎失败")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("--version: {}", stdout.trim());
    anyhow::ensure!(!stdout.trim().is_empty(), "引擎没有输出版本号");

    println!("\n验证通过：下载、解压、可执行三段全部正常");
    Ok(())
}
