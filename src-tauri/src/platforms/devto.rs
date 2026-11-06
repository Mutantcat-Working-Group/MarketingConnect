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

/// 规范化 dev.to 标签
///
/// dev.to 的 `tags` 上限为 **4 个** 且**只允许字母数字**，超限或含非法字符会被
/// API 直接拒绝。这里统一做：去掉 `#` 前缀与非字母数字字符、去重（忽略大小写）、
/// 截断到 4 个。中文等非 ASCII 字符会被过滤掉从而丢弃该标签。
fn normalize_tags(topics: &[String]) -> Vec<String> {
    const MAX_TAGS: usize = 4;
    let mut tags: Vec<String> = Vec::new();
    for topic in topics {
        // 过滤非字母数字字符时 `#` 一并被去掉，无需单独的 trim_start_matches。
        let trimmed = topic.trim();
        let cleaned: String = trimmed
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        if cleaned.is_empty() {
            // 让用户能感知标签被丢弃：dev.to 只接受字母数字标签，中文等会被
            // API 拒绝，这里必须过滤，但不能静默丢弃。
            if !trimmed.is_empty() {
                tracing::warn!("dev.to 不支持标签「{}」：只允许字母数字，已丢弃", topic);
            }
            continue;
        }
        if cleaned != trimmed {
            tracing::warn!("dev.to 标签「{}」含非法字符，已规范化为「{}」", topic, cleaned);
        }
        // 忽略大小写去重，避免 "Rust" 与 "rust" 触发 API 的重复标签拒绝。
        if tags.iter().any(|t| t.eq_ignore_ascii_case(&cleaned)) {
            tracing::warn!("dev.to 标签「{}」与已保留标签重复，已跳过", topic);
            continue;
        }
        if tags.len() == MAX_TAGS {
            // 最多 4 个是 dev.to API 的硬性限制，超出部分只能丢弃。
            tracing::warn!(
                "dev.to 最多支持 {} 个标签，标签「{}」及之后内容已被截断",
                MAX_TAGS,
                topic
            );
            break;
        }
        tags.push(cleaned);
    }
    tags
}

/// 截断平台返回的错误响应体，避免把整页 HTML 塞进用户可见的错误信息。
fn summarize_body(body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return "(无响应内容)".to_string();
    }
    const MAX_CHARS: usize = 500;
    if body.chars().count() <= MAX_CHARS {
        body.to_string()
    } else {
        format!("{}…", body.chars().take(MAX_CHARS).collect::<String>())
    }
}

/// 构造 Dev.to 文章请求体（纯函数，便于测试）
pub fn build_article_payload(content: &PublishContent) -> serde_json::Value {
    let tags = normalize_tags(&content.topics);
    serde_json::json!({
        "article": {
            "title": content.title.clone().unwrap_or_default(),
            "body_markdown": content.text,
            "published": true,
            // dev.to 不接受超过 4 个或含非字母数字字符的标签，已在 normalize_tags 中规范化。
            "tags": if tags.is_empty() { serde_json::Value::Null } else { serde_json::json!(tags) },
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
            // 封面：dev.to 的 `main_image` 需要**已托管的图片 URL**（须先调用
            // POST /api/images 上传取得 URL），本地文件路径不能直接用。
            // 本批次不实现，留待后续批次，故 cover 保持 false。
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
        anyhow::bail!("Dev.to 请配置 API 令牌后发布（在账号设置中粘贴令牌），不支持浏览器发布")
    }

    /// 通过 API 令牌发布文章
    ///
    /// POST `https://dev.to/api/articles`，请求头 `api-key: <token>`，
    /// 请求体 `{ "article": <build_article_payload> }`。非 2xx 时把响应体
    /// 摘要带进错误信息（dev.to 会在 body 里说明具体原因）。
    async fn publish_with_token(&self, token: &str, content: &PublishContent) -> Result<()> {
        self.capabilities().validate(content)?;
        if content.title.as_deref().unwrap_or("").trim().is_empty() {
            anyhow::bail!("Dev.to 必须提供标题");
        }

        let payload = build_article_payload(content);
        let response = reqwest::Client::new()
            .post(format!("{API_BASE}/articles"))
            .header("api-key", token)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("连接 Dev.to 失败")?;

        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "发布到 dev.to 失败（HTTP {}）：{}",
            status.as_u16(),
            summarize_body(&body)
        )
    }

    async fn check_session(&self, _page: &Page) -> Result<bool> {
        // 令牌型平台没有浏览器会话，页面探测无意义：会话有效性一律由
        // `verify_token` 判定，调用方（accounts / session_monitor / login）
        // 必须先用 `token_hint()` 分流到令牌校验。若仍有调用方走到这里，
        // 直接报错而不是返回 `Ok(false)`——返回 false 会被当成「掉号」，
        // 触发系统通知与短信误报。
        anyhow::bail!("Dev.to 为令牌型平台，不支持页面会话检测，请改用 verify_token 校验 API 令牌")
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
        // 封面本批次未实现（main_image 需已托管的图片 URL），必须保持 false。
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

    fn content_with(topics: Vec<String>) -> PublishContent {
        PublishContent {
            title: None, text: "正文".into(), media_paths: Vec::new(),
            cover_path: None, topics, thread_segments: Vec::new(), schedule_at: None,
        }
    }

    #[test]
    fn normalizes_tags_to_devto_constraints() {
        // dev.to 只接受最多 4 个、且仅含字母数字的标签；超限或含非法字符会被
        // API 直接拒绝，故 build_article_payload 必须先规范化。
        let tags = normalize_tags(&[
            "#Rust".into(),        // 去掉 `#`
            "rust".into(),         // 与前一个忽略大小写重复
            "hello world".into(),  // 空格被过滤
            "中文".into(),          // 非字母数字，整体丢弃
            "a-b_c".into(),        // 连字符/下划线被过滤
            "tauri".into(),
            "fifth".into(),        // 第 5 个被截断
            "".into(),
        ]);
        assert_eq!(tags, vec!["Rust", "helloworld", "abc", "tauri"]);
    }

    #[test]
    fn normalizes_more_than_four_tags_to_four() {
        let topics: Vec<String> = (0..10).map(|i| format!("tag{i}")).collect();
        assert_eq!(normalize_tags(&topics).len(), 4);
    }

    #[test]
    fn empty_tags_serialize_as_null() {
        let payload = build_article_payload(&content_with(vec!["中文".into()]));
        assert_eq!(payload["article"]["tags"], serde_json::Value::Null);
    }

    #[test]
    fn summarize_body_truncates_multibyte_safely() {
        let long = "错".repeat(600);
        let summary = summarize_body(&long);
        assert_eq!(summary.chars().count(), 501, "截断后应为 500 字符加省略号");
        assert!(summary.ends_with('…'));
        assert_eq!(summarize_body("   "), "(无响应内容)");
        assert_eq!(summarize_body("  boom  "), "boom");
    }

    // 只走到请求前的本地校验分支，不会发起任何网络请求（用的是假令牌）。
    #[tokio::test]
    async fn publish_with_token_validates_before_any_network_call() {
        let adapter = DevToAdapter::new();

        // 缺标题：在发请求前就报错
        let err = adapter
            .publish_with_token("fake-token", &content_with(Vec::new()))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("标题"), "实际: {err}");

        // 声明了封面但 dev.to 本批次未实现（cover: false）：同样在请求前拒绝
        let mut with_cover = content_with(Vec::new());
        with_cover.title = Some("标题".into());
        with_cover.cover_path = Some("/tmp/cover.png".into());
        let err = adapter
            .publish_with_token("fake-token", &with_cover)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("封面"), "实际: {err}");
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
