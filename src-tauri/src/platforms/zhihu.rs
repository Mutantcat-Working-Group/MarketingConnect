// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 知乎适配器（浏览器族打样）
//!
//! 创作中心：https://www.zhihu.com/
//! 文章编辑器：https://zhuanlan.zhihu.com/write
//! 登录页：https://www.zhihu.com/signin
//!
//! **选择器均未经过真机验证**：发布页在登录态之后（本项目没有知乎账号），
//! 登录流程当前又是交互式的（见 `login` 的说明）。候选选择器集中定义在下方常量，
//! 一旦全部落空，`platforms::select` 内核会在错误里列出尝试过的每一个选择器，
//! 便于按真实 DOM 校正。请不要把这里的选择器当成已验证。

use super::{Capabilities, PlatformAdapter, PublishContent};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chromiumoxide::Page;

/// 创作中心首页（也是会话检测的可信 origin）
const CREATOR_URL: &str = "https://www.zhihu.com/";
/// 文章编辑器
const WRITE_URL: &str = "https://zhuanlan.zhihu.com/write";

// ---- 登录表单选择器 ----------------------------------------------------------
// LOGIN_URL 是登录页（注意：登录表单在 `/signin`，**不是**首页 `/`）。
// LOGIN_USER / LOGIN_PASS / LOGIN_SUBMIT 是**未实测**的候选选择器：**保留给未来的
// 自动化重登**。当前全仓没有任何地方调用 `PlatformAdapter::login`，真实登录由前端
// `login_action`（点击 / 输入 / 按键）配合 `finish_account_login`（持久化会话）
// 交互完成，见 `src-tauri/src/login.rs`。若将来接线自动化重登，请先按真实登录页
// DOM 校正这三组选择器。
//
// 由于 `login` 当前没有调用方，以下常量在非测试构建下会被判为 dead_code，
// 这里像 `Capabilities::TEXT_ONLY` 一样逐个精确抑制，不要为了消警告而删除它们。
#[allow(dead_code)]
const LOGIN_URL: &str = "https://www.zhihu.com/signin";
#[allow(dead_code)]
const LOGIN_USER: &[&str] = &["input[name='username']", "input[type='text']"];
#[allow(dead_code)]
const LOGIN_PASS: &[&str] = &["input[name='password']", "input[type='password']"];
#[allow(dead_code)]
const LOGIN_SUBMIT: &[&str] = &["button[type='submit']", "button.SignFlow-submitButton"];

// ---- 编辑器选择器 ------------------------------------------------------------
// 候选选择器（未实测）：发布页必须在登录态之后才能观察，本项目没有知乎账号。
// 全部失败时 `select` 内核会报出尝试过的每一个选择器。
const TITLE: &[&str] = &["textarea[placeholder*='标题']", ".WriteIndex-titleInput textarea"];
const BODY: &[&str] = &[".public-DraftEditor-content", "div[contenteditable='true']"];
const TOPIC: &[&str] = &["input[placeholder*='话题']", ".Popover-content input"];
const SUBMIT: &[&str] = &["button[type='submit']", "button.PublishPanel-submitButton"];
/// 媒体（图片 / 视频）上传控件，同样是未实测的候选选择器
const MEDIA: &[&str] = &["input[type='file']"];

pub struct ZhihuAdapter;

impl ZhihuAdapter {
    pub fn new() -> Self {
        ZhihuAdapter
    }
}

impl Default for ZhihuAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PlatformAdapter for ZhihuAdapter {
    fn platform_id(&self) -> &str {
        "zhihu"
    }

    fn display_name(&self) -> &str {
        "知乎"
    }

    fn creator_url(&self) -> &str {
        CREATOR_URL
    }

    fn token_hint(&self) -> Option<&str> {
        None
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            text: true,
            images: true,
            video: true,
            multi_image: true,
            cover: true,
            topics: true,
            thread: false,
            schedule: false,
        }
    }

    /// 帐密自动登录（**保留给未来的自动化重登，当前流程为交互式登录**）
    ///
    /// 全仓当前没有任何调用方：实际登录由前端 `login_action` + `finish_account_login`
    /// 在可见页面里完成（扫码或输密码），见 `src-tauri/src/login.rs` 与 `src/api.ts`。
    /// 因此本方法及其选择器未经真机验证，接线自动化重登前必须先按真实登录页校正
    /// `LOGIN_USER` / `LOGIN_PASS` / `LOGIN_SUBMIT`。
    async fn login(&self, page: &Page, username: &str, password: &str) -> Result<String> {
        // 登录表单在 /signin（不是首页）：直接打开登录页，避免依赖首页跳转。
        page.goto(LOGIN_URL).await.context("打开知乎登录页失败")?;
        // 等待 SPA 渲染登录组件，否则 find_element 会在空 DOM 上失败。
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        let ops = super::select::PageOps { page };
        super::select::fill_any(&ops, LOGIN_USER, username).await?;
        super::select::fill_any(&ops, LOGIN_PASS, password).await?;
        super::select::click_any(&ops, LOGIN_SUBMIT).await?;
        // 提交后等待跳转 / 渲染，再读取 cookie。
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // 按既有适配器（csdn / xiaohongshu / wechat_mp）的模式返回 **cookie JSON**，
        // 不能返回空串——调用方要用这段 JSON 持久化会话。
        let cookies = page.get_cookies().await.context("获取 cookie 失败")?;
        let json = serde_json::to_string(&cookies).context("cookie 序列化失败")?;

        let url = page.url().await?.unwrap_or_default();
        if url.contains("signin") || url.contains("login") {
            anyhow::bail!("尚未登录：请在截图视图中完成验证码/登录后点击「保存 Cookie」");
        }
        Ok(json)
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
            super::select::upload_any(&ops, MEDIA, &content.media_paths).await?;
        }
        super::select::click_any(&ops, SUBMIT).await?;
        Ok(())
    }

    async fn check_session(&self, page: &Page) -> Result<bool> {
        super::auth::authenticated_probe(page, self.creator_url(), None).await
    }
}

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
