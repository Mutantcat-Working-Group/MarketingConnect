// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 端到端验证：走与 App 完全相同的 CDP 路径——中继 + CdpBrowser。
//!
//! 用法: cargo run --example cdp_selftest -- <引擎端口> <平台地址>

use marketing_connect_lib::obscura::{CdpBrowser, CdpRelay};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 打开日志，才能看到 chromiumoxide handler 的真实报错
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
        )
        .init();

    let mut args = std::env::args().skip(1);
    let engine_port: u16 = args
        .next()
        .unwrap_or_else(|| "9444".into())
        .parse()
        .expect("引擎端口");
    let creator_url = args
        .next()
        .unwrap_or_else(|| "https://creator.douyin.com/".to_string());

    // 1. 启动中继（App 里由 CdpRelay 状态管理，同样按引擎端口复用）
    let relay = CdpRelay::default();
    let relay_port = relay.port_for(engine_port).await?;
    println!("[1] 中继就绪: 引擎 {engine_port} → 中继 {relay_port}");

    // 2. 通过中继连接（与 session::resolve_browser 一致）
    let ws_url = format!("ws://127.0.0.1:{relay_port}/devtools/browser");
    let browser = CdpBrowser::connect(&ws_url)
        .await
        .map_err(|e| anyhow::anyhow!("连接中继失败: {e}"))?;
    println!("[2] 通过中继连接 CDP 成功");

    // 3. 创建隔离上下文与页面
    let (_context_id, page) = browser
        .create_context_page("about:blank")
        .await
        .map_err(|e| anyhow::anyhow!("创建 BrowserContext 失败: {e}"))?;
    println!("[3] BrowserContext + Page 创建成功");

    // 4. 导航到平台（这一步以前必定失败：Network 事件打挂 handler）
    page.goto(creator_url.as_str())
        .await
        .map_err(|e| anyhow::anyhow!("平台页面导航失败: {e}"))?;
    println!("[4] 导航成功: {creator_url}");

    // 5. 等待页面稳定，再确认连接仍然存活
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;

    // 使用 App 生产路径的求值助手（内含上下文失效重试）
    let title: String = marketing_connect_lib::obscura::evaluate(
        &page,
        "document.title + ' | ' + location.href",
    )
    .await
    .map_err(|e| anyhow::anyhow!("连接已断开或求值失败: {e}"))?;
    println!("[5] 页面实况: {title}");

    // 6. 截图（登录面板依赖）
    let bytes = page
        .screenshot(chromiumoxide::page::ScreenshotParams::builder().build())
        .await
        .map_err(|e| anyhow::anyhow!("截图失败: {e}"))?;
    println!("[6] 截图成功: {} 字节", bytes.len());

    // 7. 空闲后继续求值，确认连接可长期存活
    for _ in 0..3 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let _: String = marketing_connect_lib::obscura::evaluate(&page, "document.title")
            .await
            .map_err(|e| anyhow::anyhow!("连接在空闲后断开: {e}"))?;
    }
    println!("[7] 连接在持续网络事件下保持稳定 ✅");

    println!("\n结论：全部通过，打开登录会话的链路已修复。");
    Ok(())
}
