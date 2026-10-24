// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 抖音平台适配器
//!
//! Spike 已验证：Obscura 可完整渲染抖音创作者平台
//! （登录入口可见、JS 全量执行）。
//!
//! 登录流程（QR 扫码）：
//! 1. 导航到创作者首页，未登录时呈现扫码登录页
//! 2. 用户在截图视图中用抖音 App 扫码
//! 3. 登录成功后用户点击「保存 Cookie」持久化会话
//!
//! 发布流程：待交互验证后填充选择器；
//! 过渡期可通过「自定义平台」注册抖音并配置选择器实现自动发布。

use super::{PublishContent, PlatformAdapter};
use anyhow::{bail, Result};
use async_trait::async_trait;
use chromiumoxide::Page;

pub struct DouyinAdapter;

impl DouyinAdapter {
    pub fn new() -> Self {
        DouyinAdapter
    }
}

#[async_trait]
impl PlatformAdapter for DouyinAdapter {
    fn platform_id(&self) -> &str {
        "douyin"
    }

    fn display_name(&self) -> &str {
        "抖音"
    }

    fn creator_url(&self) -> &str {
        "https://creator.douyin.com/creator-micro/home"
    }

    async fn login(&self, page: &Page, _username: &str, _password: &str) -> Result<String> {
        // 抖音创作者平台为扫码登录：导航到首页，
        // 未登录时呈现二维码，用户在截图视图中扫码完成登录。
        page.goto(self.creator_url())
            .await
            .map_err(|e| anyhow::anyhow!("导航到抖音创作者平台失败: {}", e))?;

        // 等待页面渲染（SPA 需要时间加载登录组件）
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // 导出当前 cookie（已登录则为有效会话，未登录则后续由用户扫码后保存）
        let cookies = page
            .get_cookies()
            .await
            .map_err(|e| anyhow::anyhow!("获取 cookie 失败: {}", e))?;
        let json = serde_json::to_string(&cookies)
            .map_err(|e| anyhow::anyhow!("cookie 序列化失败: {}", e))?;

        // 检查是否已登录（URL 不含 login 且 cookie 非空）
        let url = page.url().await?.unwrap_or_default();
        if url.contains("login") {
            bail!("尚未登录：请在截图视图中扫码登录，成功后点击「保存 Cookie」")
        }

        Ok(json)
    }

    async fn publish(&self, _page: &Page, _content: &PublishContent) -> Result<()> {
        // 发布流程需要精确的 DOM 选择器，待交互验证后填充。
        // 过渡方案：在「设置 → 自定义平台」中注册抖音，
        // 配置发布页 URL 与表单选择器即可自动发布。
        bail!("抖音自动发布待交互验证后实现；可先在自定义平台中注册抖音并配置选择器")
    }

    async fn check_session(&self, page: &Page) -> Result<bool> {
        // 导航到创作者首页，若被重定向到登录页则掉号
        page.goto(self.creator_url())
            .await
            .map_err(|e| anyhow::anyhow!("导航失败: {}", e))?;
        let url = page.url().await?.unwrap_or_default();
        Ok(!url.contains("login"))
    }
}
