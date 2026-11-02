// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 小红书平台适配器
//!
//! 创作服务平台：https://creator.xiaohongshu.com/
//!
//! 登录流程：
//! 1. 导航到创作服务平台首页，未登录会自动跳转 /login（扫码 / 手机号验证码登录）
//! 2. 用户在截图视图中完成登录
//! 3. 登录成功后点击「保存 Cookie」持久化会话
//!
//! 发布流程：导航到笔记发布页，按候选选择器填充标题/正文后点击发布。
//! 页面结构变化导致选择器全部失效时会给出明确提示，
//! 此时可在「设置 → 自定义平台」中精确配置选择器。

use super::{PlatformAdapter, PublishContent};
use anyhow::{bail, Result};
use async_trait::async_trait;
use chromiumoxide::Page;

/// 创作服务平台首页
const CREATOR_URL: &str = "https://creator.xiaohongshu.com/";
/// 笔记发布页
const PUBLISH_URL: &str = "https://creator.xiaohongshu.com/publish/publish";

pub struct XiaohongshuAdapter;

impl XiaohongshuAdapter {
    pub fn new() -> Self {
        XiaohongshuAdapter
    }
}

#[async_trait]
impl PlatformAdapter for XiaohongshuAdapter {
    fn platform_id(&self) -> &str {
        "xiaohongshu"
    }

    fn display_name(&self) -> &str {
        "小红书"
    }

    fn creator_url(&self) -> &str {
        CREATOR_URL
    }

    async fn login(&self, page: &Page, _username: &str, _password: &str) -> Result<String> {
        // 小红书创作服务平台为扫码 / 手机号验证码登录：
        // 导航到首页，未登录时自动跳转登录页，用户在截图视图中操作。
        page.goto(CREATOR_URL)
            .await
            .map_err(|e| anyhow::anyhow!("导航到小红书创作服务平台失败: {}", e))?;

        // 等待 SPA 渲染登录组件
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        let cookies = page
            .get_cookies()
            .await
            .map_err(|e| anyhow::anyhow!("获取 cookie 失败: {}", e))?;
        let json =
            serde_json::to_string(&cookies).map_err(|e| anyhow::anyhow!("cookie 序列化失败: {}", e))?;

        let url = page.url().await?.unwrap_or_default();
        if url.contains("login") {
            bail!("尚未登录：请在截图视图中扫码或输入手机号登录，成功后点击「保存 Cookie」")
        }

        Ok(json)
    }

    async fn publish(&self, page: &Page, content: &PublishContent) -> Result<()> {
        page.goto(PUBLISH_URL)
            .await
            .map_err(|e| anyhow::anyhow!("导航到小红书发布页失败: {}", e))?;

        // 发布页为前端框架渲染，等待编辑器挂载
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        let url = page.url().await?.unwrap_or_default();
        if url.contains("login") {
            bail!("会话已失效：请先打开会话完成登录后再发布")
        }

        // 1. 标题（候选选择器逐个尝试）
        if let Some(title) = &content.title {
            if !title.is_empty() {
                let ok = super::fill_first(
                    page,
                    &[
                        "input[placeholder*=\"标题\"]",
                        "input[placeholder*=\"填写标题\"]",
                        ".title-input input",
                        "input.d-text",
                    ],
                    title,
                )
                .await;
                if !ok {
                    bail!("找不到小红书标题输入框（页面结构可能已变更），请在「自定义平台」中配置标题选择器")
                }
            }
        }

        // 2. 正文（富文本编辑器）
        let ok = super::fill_first(
            page,
            &[
                "div[contenteditable=\"true\"]",
                ".ql-editor",
                "textarea[placeholder*=\"正文\"]",
                "div[placeholder*=\"正文\"]",
            ],
            &content.text,
        )
        .await;
        if !ok {
            bail!("找不到小红书正文编辑器（页面结构可能已变更），请在「自定义平台」中配置正文选择器")
        }

        // 3. 发布按钮（只点击平台特定类名，避免误触其它按钮）
        let ok = super::click_first(
            page,
            &[
                "button.publishBtn",
                "button.publish-btn",
                "button.submit-btn",
                ".publish-btn",
            ],
        )
        .await;
        if !ok {
            bail!("内容已填入编辑器，但找不到发布按钮：请在截图视图中确认后手动点击发布")
        }

        Ok(())
    }

    async fn check_session(&self, page: &Page) -> Result<bool> {
        page.goto(CREATOR_URL)
            .await
            .map_err(|e| anyhow::anyhow!("导航失败: {}", e))?;
        let url = page.url().await?.unwrap_or_default();
        Ok(!url.contains("login"))
    }
}