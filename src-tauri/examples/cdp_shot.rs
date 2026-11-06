// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 渲染验证：走中继打开平台页面并存下截图，用于检查字体/排版是否正常。
//!
//! 用法: cargo run --example cdp_shot -- <引擎端口> <页面地址> <输出 png>

use marketing_connect_lib::obscura::{CdpBrowser, CdpRelay};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let engine_port: u16 = args.next().unwrap_or_else(|| "9462".into()).parse()?;
    let url = args
        .next()
        .unwrap_or_else(|| "https://creator.douyin.com/".to_string());
    let output = args
        .next()
        .unwrap_or_else(|| "/tmp/mc-engine/shot.png".to_string());

    let relay = CdpRelay::default();
    let relay_port = relay.port_for(engine_port).await?;
    let ws_url = format!("ws://127.0.0.1:{relay_port}/devtools/browser");
    let browser = CdpBrowser::connect(&ws_url).await?;
    let (_context_id, page) = browser.create_context_page("about:blank").await?;

    page.goto(url.as_str()).await?;
    tokio::time::sleep(std::time::Duration::from_secs(8)).await;

    let title: String =
        marketing_connect_lib::obscura::evaluate(&page, "document.title").await?;
    println!("标题: {title}");

    // 可选：第 4 个参数为要在页面上求值的 JS 表达式
    if let Some(expression) = args.next() {
        let value: String =
            marketing_connect_lib::obscura::evaluate(&page, expression).await?;
        println!("求值结果: {value}");
    }

    let png = page
        .screenshot(chromiumoxide::page::ScreenshotParams::builder().full_page(false).build())
        .await?;
    std::fs::write(&output, &png)?;
    println!("已保存 {} ({} 字节)", output, png.len());
    Ok(())
}
