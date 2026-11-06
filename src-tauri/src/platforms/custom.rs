// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 自定义平台适配器
//!
//! 用户可在设置中注册自定义平台（创作者首页 URL、发布页 URL、
//! 表单选择器），通用适配器按配置驱动：
//! - 登录：导航到创作者首页，用户通过截图辅助手动登录
//! - 发布：导航到发布页，按选择器填充标题/内容并提交
//! - 会话检测：不导航，检查同源已登录 UI 或显式配置的非空 URL 特征

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
    /// 显式登录态 URL 路由特征（仅同源非空匹配；未配置时需可见已登录 UI）
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

    // 有发布页与内容选择器即视为可发文本，媒体能力取决于是否配置了选择器。
    //
    // `publish` 必须绑定到「是否配置了提交按钮选择器」：没有提交选择器时
    // `publish()` 只能填充内容、永远无法真正提交（见下方 publish 的显式报错）。
    // 若这里仍返回 true，UI 会宣称平台可自动发布，任务还会被标成「发布成功」。
    fn capabilities(&self) -> super::Capabilities {
        super::Capabilities {
            publish: self.config.submit_selector.is_some(),
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
        if let Some(title) = content.title.as_deref().filter(|t| !t.is_empty()) {
            match &self.config.title_selector {
                Some(sel) => {
                    let el = page
                        .find_element(sel.as_str())
                        .await
                        .context("找不到标题输入框")?;
                    el.click().await.context("点击标题输入框失败")?;
                    el.type_str(title).await.context("填写标题失败")?;
                }
                // 用户填了标题但平台没配置标题选择器：不能静默丢弃，否则用户
                // 会以为标题发出去了，实际只发了正文。
                None => {
                    tracing::warn!(
                        "自定义平台「{}」未配置标题选择器，标题「{}」将被忽略",
                        self.config.name,
                        title
                    );
                }
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

        // 4. 提交：没有提交选择器时必须显式报错，绝不能填完内容就返回 Ok，
        //    否则调度器会把「只填了表单、从未提交」当成发布成功。
        let Some(sel) = self.config.submit_selector.as_deref() else {
            anyhow::bail!(
                "自定义平台「{}」未配置提交按钮选择器，无法自动提交；请补全提交选择器，或手动打开会话发布",
                self.config.name
            );
        };
        let el = page
            .find_element(sel)
            .await
            .context("找不到提交按钮")?;
        el.click().await.context("点击提交按钮失败")?;

        Ok(())
    }

    async fn check_session(&self, page: &Page) -> Result<bool> {
        super::auth::authenticated_probe(
            page,
            self.creator_url(),
            self.config.logged_in_pattern.as_deref(),
        ).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(submit_selector: Option<&str>) -> CustomPlatformAdapter {
        CustomPlatformAdapter::new(CustomPlatform {
            id: "my-platform".into(),
            name: "我的平台".into(),
            creator_url: "https://example.com/home".into(),
            publish_url: "https://example.com/publish".into(),
            title_selector: Some("#title".into()),
            content_selector: Some(".editor".into()),
            submit_selector: submit_selector.map(String::from),
            logged_in_pattern: None,
        })
    }

    #[test]
    fn publish_capability_requires_submit_selector() {
        // 没有提交按钮选择器时 publish() 无法真正提交（只会 bail!），
        // 因此能力矩阵必须如实声明 publish: false，否则会「假成功」。
        let without_submit = adapter(None);
        assert!(
            !without_submit.capabilities().publish,
            "未配置提交选择器时不得声明可自动发布"
        );

        // 配了提交选择器才允许声明可发布。
        let with_submit = adapter(Some("button[type=submit]"));
        assert!(with_submit.capabilities().publish);
        assert!(with_submit.capabilities().text);
    }

    #[test]
    fn content_selector_still_drives_text_capability() {
        // text 能力仍绑定内容选择器；与 publish 是两个独立维度。
        let mut config = CustomPlatform {
            id: "my-platform".into(),
            name: "我的平台".into(),
            creator_url: "https://example.com/home".into(),
            publish_url: "https://example.com/publish".into(),
            title_selector: None,
            content_selector: None,
            submit_selector: Some("button[type=submit]".into()),
            logged_in_pattern: None,
        };
        let caps = CustomPlatformAdapter::new(config.clone()).capabilities();
        assert!(caps.publish, "有提交选择器即可自动发布");
        assert!(!caps.text, "没有内容选择器则不能发文本");

        config.content_selector = Some(".editor".into());
        let caps = CustomPlatformAdapter::new(config).capabilities();
        assert!(caps.text);
    }
}
