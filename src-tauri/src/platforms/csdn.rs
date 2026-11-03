// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! CSDN 平台适配器
//!
//! 创作中心：https://mp.csdn.net/
//! 登录页：https://passport.csdn.net/login（帐号密码 / 扫码登录）
//!
//! 登录流程：
//! 1. 导航到创作中心，未登录会被重定向到 passport.csdn.net 登录页
//! 2. 用户在截图视图中输入帐号密码或扫码登录
//! 3. 登录成功后点击「保存 Cookie」持久化会话
//!
//! 发布流程：导航到 Markdown 编辑器新建文章页，
//! 按候选选择器填充标题 / 正文后点击发布。

use super::{PlatformAdapter, PublishContent};
use anyhow::{bail, Result};
use async_trait::async_trait;
use chromiumoxide::Page;

/// 创作中心（未登录会重定向到 passport.csdn.net）
const CREATOR_URL: &str = "https://mp.csdn.net/";
/// Markdown 编辑器新建文章页
const PUBLISH_URL: &str = "https://editor.csdn.net/md/";

pub struct CsdnAdapter;

impl CsdnAdapter {
    pub fn new() -> Self {
        CsdnAdapter
    }
}

/// 判断当前 URL 是否处于未登录状态
///
/// 未登录访问创作中心会被重定向到 passport.csdn.net 登录页。
fn is_login_page(url: &str) -> bool {
    url.contains("passport.csdn.net") || url.contains("login")
}

#[async_trait]
impl PlatformAdapter for CsdnAdapter {
    fn platform_id(&self) -> &str {
        "csdn"
    }

    fn display_name(&self) -> &str {
        "CSDN"
    }

    fn creator_url(&self) -> &str {
        CREATOR_URL
    }

    async fn login(&self, page: &Page, _username: &str, _password: &str) -> Result<String> {
        page.goto(CREATOR_URL)
            .await
            .map_err(|e| anyhow::anyhow!("导航到 CSDN 创作中心失败: {}", e))?;

        // 等待重定向与页面渲染
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        let cookies = page
            .get_cookies()
            .await
            .map_err(|e| anyhow::anyhow!("获取 cookie 失败: {}", e))?;
        let json =
            serde_json::to_string(&cookies).map_err(|e| anyhow::anyhow!("cookie 序列化失败: {}", e))?;

        let url = page.url().await?.unwrap_or_default();
        if is_login_page(&url) {
            bail!("尚未登录：请在截图视图中输入帐号密码或扫码登录，成功后点击「保存 Cookie」")
        }

        Ok(json)
    }

    async fn publish(&self, page: &Page, content: &PublishContent) -> Result<()> {
        page.goto(PUBLISH_URL)
            .await
            .map_err(|e| anyhow::anyhow!("导航到 CSDN 编辑器失败: {}", e))?;

        // 编辑器加载 + 登录态校验重定向
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        let url = page.url().await?.unwrap_or_default();
        if is_login_page(&url) {
            bail!("会话已失效：请先打开会话完成登录后再发布")
        }

        // 1. 标题
        if let Some(title) = &content.title {
            if !title.is_empty() {
                let ok = super::fill_first(
                    page,
                    &[
                        "input[placeholder*=\"标题\"]",
                        ".article-bar__title input",
                        "input.article-bar__title",
                        "input[placeholder*=\"请输入文章标题\"]",
                    ],
                    title,
                )
                .await;
                if !ok {
                    bail!("找不到 CSDN 标题输入框（页面结构可能已变更），请在「自定义平台」中配置标题选择器")
                }
            }
        }

        // 2. 正文（Markdown 编辑器）
        let ok = super::fill_first(
            page,
            &[
                ".editor__inner",
                ".CodeMirror textarea",
                ".cm-content",
                "textarea[placeholder*=\"正文\"]",
                "div[contenteditable=\"true\"]",
            ],
            &content.text,
        )
        .await;
        if !ok {
            bail!("找不到 CSDN 正文编辑器（页面结构可能已变更），请在「自定义平台」中配置正文选择器")
        }

        // 3. 发布按钮（只点击平台特定类名，避免误触保存草稿以外的按钮）
        let ok = super::click_first(
            page,
            &[
                "button.btn-publish",
                ".btn-publish",
                "button.article-bar__publish",
                ".article-bar__publish",
            ],
        )
        .await;
        if !ok {
            bail!("内容已填入编辑器，但找不到发布按钮：请在截图视图中确认后手动点击发布")
        }

        Ok(())
    }

    async fn check_session(&self, page: &Page) -> Result<bool> {
        super::auth::authenticated_probe(page, self.creator_url(), None).await
    }
}