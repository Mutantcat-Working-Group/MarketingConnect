// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 自定义平台适配器
//!
//! 用户可在设置中注册自定义平台（创作者首页 URL、发布页 URL、
//! 表单选择器），通用适配器按配置驱动：
//! - 登录：导航到创作者首页，用户通过截图辅助手动登录
//! - 发布：导航到发布页，按选择器填充标题/内容并提交
//! - 会话检测：导航到创作者首页，按 URL 特征判断登录态

use super::{PlatformAdapter, PublishContent};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chromiumoxide::Page;

/// 自定义平台配置
#[derive(Debug, Clone)]
pub struct CustomPlatform {
    /// 平台标识（用户自定义，如 "my-platform"）
    pub id: String,
    /// 平台显示名
    pub name: String,
    /// 创作者平台首页 URL
    pub creator_url: String,
    /// 发布页 URL
    pub publish_url: String,
    /// 标题输入框选择器（可选）
    pub title_selector: Option<String>,
    /// 内容输入框选择器（可选）
    pub content_selector: Option<String>,
    /// 提交按钮选择器（可选）
    pub submit_selector: Option<String>,
    /// 登录态 URL 特征（登录后 URL 包含该字符串；为空则用 "不含 login" 启发式）
    pub logged_in_pattern: Option<String>,
}

/// 自定义平台适配器
pub struct CustomPlatformAdapter {
    config: CustomPlatform,
}

impl CustomPlatformAdapter {
    pub fn new(config: CustomPlatform) -> Self {
        CustomPlatformAdapter { config }
    }
}

#[async_trait]
impl PlatformAdapter for CustomPlatformAdapter {
    fn platform_id(&self) -> &str {
        &self.config.id
    }

    fn display_name(&self) -> &str {
        &self.config.name
    }

    fn creator_url(&self) -> &str {
        &self.config.creator_url
    }

    async fn login(&self, page: &Page, _username: &str, _password: &str) -> Result<String> {
        // 通用平台不支持自动帐密登录：导航到创作者首页，
        // 用户通过截图辅助手动登录后保存 cookie
        page.goto(self.config.creator_url.as_str())
            .await
            .context("导航到创作者首页失败")?;
        Ok(String::new())
    }

    async fn publish(&self, page: &Page, content: &PublishContent) -> Result<()> {
        // 1. 导航到发布页
        page.goto(self.config.publish_url.as_str())
            .await
            .context("导航到发布页失败")?;

        // 等待页面渲染（简单延时，通用平台无法精确等待）
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // 2. 填充标题（配置了选择器且内容有标题时）
        if let (Some(sel), Some(title)) = (&self.config.title_selector, &content.title) {
            if !title.is_empty() {
                let el = page
                    .find_element(sel.as_str())
                    .await
                    .context("找不到标题输入框")?;
                el.click().await.context("点击标题输入框失败")?;
                el.type_str(title).await.context("填写标题失败")?;
            }
        }

        // 3. 填充内容
        if let Some(sel) = &self.config.content_selector {
            let el = page
                .find_element(sel.as_str())
                .await
                .context("找不到内容输入框")?;
            el.click().await.context("点击内容输入框失败")?;
            el.type_str(&content.text)
                .await
                .context("填写内容失败")?;
        }

        // 4. 提交（配置了选择器时）
        if let Some(sel) = &self.config.submit_selector {
            let el = page
                .find_element(sel.as_str())
                .await
                .context("找不到提交按钮")?;
            el.click().await.context("点击提交按钮失败")?;
        }

        Ok(())
    }

    async fn check_session(&self, page: &Page) -> Result<bool> {
        page.goto(self.config.creator_url.as_str())
            .await
            .context("导航到创作者首页失败")?;
        let url = page.url().await?.unwrap_or_default();

        // 配置了登录特征：URL 包含即有效
        if let Some(pattern) = &self.config.logged_in_pattern {
            Ok(url.contains(pattern.as_str()))
        } else {
            // 默认启发式：URL 不含 login 即视为有效
            Ok(!url.contains("login"))
        }
    }
}
