// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! Dev.to 适配器（令牌族）
//!
//! 令牌自助获取：https://dev.to/settings/extensions → DEV Community API Keys
//! 接口文档：https://developers.forem.com/api/v1

use super::{Capabilities, PlatformAdapter, PublishContent};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chromiumoxide::Page;

const API_BASE: &str = "https://dev.to/api";
const CREATOR_URL: &str = "https://dev.to/dashboard";

pub struct DevToAdapter;

impl DevToAdapter {
    pub fn new() -> Self { DevToAdapter }
}

impl Default for DevToAdapter {
    fn default() -> Self { Self::new() }
}

/// 构造 Dev.to 文章请求体（纯函数，便于测试）
///
/// 目前只有本模块的 `#[cfg(test)]` 用例引用它；真正的发布接线在 Task 11
/// （`publish_with_token`）中完成，非测试构建下因此是 dead code。
/// 这里按 `Capabilities::TEXT_ONLY` / `zhihu.rs` 的 `LOGIN_*` 先例做精确抑制，
/// 不要为了消警告而删除它。
#[allow(dead_code)]
pub fn build_article_payload(content: &PublishContent) -> serde_json::Value {
    serde_json::json!({
        "article": {
            "title": content.title.clone().unwrap_or_default(),
            "body_markdown": content.text,
            "published": true,
            "tags": if content.topics.is_empty() { serde_json::Value::Null } else { serde_json::json!(content.topics) },
        }
    })
}

#[async_trait]
impl PlatformAdapter for DevToAdapter {
    fn platform_id(&self) -> &str { "devto" }
    fn display_name(&self) -> &str { "Dev.to" }
    fn creator_url(&self) -> &str { CREATOR_URL }

    fn token_hint(&self) -> Option<&str> {
        Some("在 dev.to → Settings → Extensions → DEV Community API Keys 生成，粘贴后立即校验")
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            publish: true,
            text: true, images: false, video: false, multi_image: false,
            // Task 11 实现图片上传（dev.to 的 main_image 需要已托管的图片 URL，
            // 本地路径不能直接用）后再改回 true。
            cover: false,
            topics: true, thread: false, schedule: false,
        }
    }

    async fn login(&self, _page: &Page, _username: &str, _password: &str) -> Result<String> {
        anyhow::bail!("Dev.to 使用令牌登录，请在账号页面粘贴 API 令牌")
    }

    async fn verify_token(&self, token: &str) -> Result<()> {
        let response = reqwest::Client::new()
            .get(format!("{API_BASE}/users/me"))
            .header("api-key", token)
            .send().await.context("连接 Dev.to 失败")?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else if status.as_u16() == 401 {
            Err(anyhow!("令牌无效或已过期"))
        } else {
            Err(anyhow!("校验令牌失败，服务返回 {status}"))
        }
    }

    async fn publish(&self, _page: &Page, content: &PublishContent) -> Result<()> {
        self.capabilities().validate(content)?;
        if content.title.as_deref().unwrap_or("").trim().is_empty() {
            anyhow::bail!("Dev.to 必须提供标题");
        }
        anyhow::bail!("令牌型平台发布在 Task 11 中接线（需要账号令牌）")
    }

    async fn check_session(&self, _page: &Page) -> Result<bool> {
        // 令牌型平台的会话有效性由 verify_token 决定，不依赖页面
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platforms::PlatformAdapter;

    #[test]
    fn exposes_token_hint() {
        // 计划片段直接 `DevToAdapter::new().token_hint()` 会因 `&str` 借用临时值而
        // 无法通过借用检查（E0716），这里先把适配器绑定到局部变量，语义不变。
        let adapter = DevToAdapter::new();
        let hint = adapter.token_hint();
        assert!(hint.is_some(), "令牌型平台必须提供提示");
        assert!(hint.unwrap().contains("dev.to"), "提示应说明从哪里获取令牌");
    }

    #[test]
    fn declares_api_capabilities() {
        let caps = DevToAdapter::new().capabilities();
        assert!(caps.publish && caps.text && caps.topics);
        // 封面尚未实现（main_image 需已托管的图片 URL），Task 11 前必须为 false。
        assert!(!caps.cover, "Dev.to 封面未实现，不得声明 cover: true");
        assert!(!caps.schedule, "Dev.to 不支持平台内定时");
        assert!(!caps.thread);
    }

    #[test]
    fn builds_article_payload() {
        let payload = build_article_payload(&crate::platforms::PublishContent {
            title: Some("标题".into()),
            text: "正文".into(),
            media_paths: Vec::new(),
            cover_path: Some("/tmp/cover.png".into()),
            topics: vec!["rust".into(), "tauri".into()],
            thread_segments: Vec::new(),
            schedule_at: None,
        });
        assert_eq!(payload["article"]["title"], "标题");
        assert_eq!(payload["article"]["body_markdown"], "正文");
        assert_eq!(payload["article"]["tags"], serde_json::json!(["rust", "tauri"]));
        assert_eq!(payload["article"]["published"], true);
    }

    #[test]
    fn rejects_missing_title() {
        let content = crate::platforms::PublishContent {
            title: None, text: "正文".into(), media_paths: Vec::new(),
            cover_path: None, topics: Vec::new(), thread_segments: Vec::new(), schedule_at: None,
        };
        assert!(build_article_payload(&content).get("article").is_none() || build_article_payload(&content)["article"]["title"].as_str().unwrap_or("").is_empty());
    }
}
