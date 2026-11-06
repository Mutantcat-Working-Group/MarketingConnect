// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 平台适配器
//!
//! 每个平台实现 PlatformAdapter trait 的三个核心接口：
//! - login: 登录流程（支持帐密自动登录）
//! - publish: 发布内容
//! - check_session: 会话健康检测（掉号判断）
//!
//! 适配器来源：
//! - 内置适配器（抖音 / 小红书 / 公众号 / CSDN，静态注册）
//! - 自定义平台（用户在设置中注册，存于 DB，由 CustomPlatformAdapter 驱动）

use crate::db::Db;
use anyhow::Result;
use async_trait::async_trait;

pub(crate) mod auth;
mod csdn;
mod custom;
mod douyin;
mod wechat_mp;
mod xiaohongshu;
pub mod select;
pub use csdn::CsdnAdapter;
pub use custom::{CustomPlatform, CustomPlatformAdapter};
pub use douyin::DouyinAdapter;
// 内核中媒体上传（upload_any）与 press_key 要到 Task 9 的知乎适配器才接线，
// 此处先再导出；加 allow 以免为尚未接线的骨架引入新的编译警告（同 `Capabilities` 的处理）。
#[allow(unused_imports)]
pub use select::{ElementOps, click_any, fill_any, press_key, upload_any};
pub use wechat_mp::WechatMpAdapter;
pub use xiaohongshu::XiaohongshuAdapter;

/// 内置平台 ID 列表
///
/// 这些 ID 为内置适配器保留，自定义平台不可占用，
/// 否则会与内置适配器重名导致查找结果不确定。
pub const BUILTIN_PLATFORM_IDS: &[&str] = &["douyin", "xiaohongshu", "wechat_mp", "csdn"];

/// 判断是否为内置平台 ID
pub fn is_builtin_platform_id(id: &str) -> bool {
    BUILTIN_PLATFORM_IDS.contains(&id)
}

/// 平台支持的能力，驱动前端显示与发布前校验
// 能力矩阵在后续任务（前端能力 UI 与发布前校验接线）才被实际调用，
// 此处先落地类型定义；加 allow 以免为尚未接线的骨架引入新的编译警告。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Capabilities {
    pub text: bool,
    pub images: bool,
    pub video: bool,
    pub multi_image: bool,
    pub cover: bool,
    pub topics: bool,
    pub thread: bool,
    pub schedule: bool,
}

#[allow(dead_code)]
impl Capabilities {
    /// 仅文本
    pub const TEXT_ONLY: Self = Self {
        text: true, images: false, video: false, multi_image: false,
        cover: false, topics: false, thread: false, schedule: false,
    };

    /// 校验内容是否超出平台能力，超出则返回可直接展示给用户的原因
    pub fn validate(&self, content: &PublishContent) -> anyhow::Result<()> {
        if !self.text && !content.text.trim().is_empty() {
            anyhow::bail!("该平台不支持文本发布");
        }
        if !content.media_paths.is_empty() {
            if !self.images && !self.video {
                anyhow::bail!("该平台不支持媒体文件");
            }
            if content.media_paths.len() > 1 && !self.multi_image {
                anyhow::bail!("该平台不支持一次上传多个文件");
            }
        }
        if content.cover_path.is_some() && !self.cover {
            anyhow::bail!("该平台不支持设置封面");
        }
        if !content.topics.is_empty() && !self.topics {
            anyhow::bail!("该平台不支持话题标签");
        }
        if !content.thread_segments.is_empty() && !self.thread {
            anyhow::bail!("该平台不支持线程发布");
        }
        if content.schedule_at.is_some() && !self.schedule {
            anyhow::bail!("该平台不支持平台内定时发布");
        }
        Ok(())
    }
}

/// 发布内容
#[derive(Debug, Clone)]
pub struct PublishContent {
    pub title: Option<String>,
    pub text: String,
    pub media_paths: Vec<String>,
    /// 封面（视频/文章头图）
    pub cover_path: Option<String>,
    /// 话题标签，不含 `#` 前缀
    pub topics: Vec<String>,
    /// 线程分段；非空时按顺序逐条发布，`text` 作为第一段
    pub thread_segments: Vec<String>,
    /// 平台内定时发布的绝对时间戳（毫秒）
    pub schedule_at: Option<i64>,
}

/// 平台适配器 trait
#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    /// 平台标识（douyin / xiaohongshu / ...）
    fn platform_id(&self) -> &str;

    /// 平台显示名
    fn display_name(&self) -> &str;

    /// 创作者平台首页 URL
    fn creator_url(&self) -> &str;

    /// 平台支持的能力
    fn capabilities(&self) -> Capabilities;

    /// 登录流程：使用帐密登录，返回登录后的 cookie JSON
    async fn login(&self, page: &chromiumoxide::Page, username: &str, password: &str) -> Result<String>;

    /// 发布内容
    async fn publish(&self, page: &chromiumoxide::Page, content: &PublishContent) -> Result<()>;

    /// 会话检测：返回 true 表示会话有效
    async fn check_session(&self, page: &chromiumoxide::Page) -> Result<bool>;
}

/// 获取所有已注册的平台适配器
///
/// db 为 Some 时追加用户注册的自定义平台。
pub fn all_adapters(db: Option<&Db>) -> Vec<Box<dyn PlatformAdapter>> {
    let mut adapters: Vec<Box<dyn PlatformAdapter>> = vec![
        Box::new(douyin::DouyinAdapter::new()),
        Box::new(xiaohongshu::XiaohongshuAdapter::new()),
        Box::new(wechat_mp::WechatMpAdapter::new()),
        Box::new(csdn::CsdnAdapter::new()),
    ];

    // 追加自定义平台（DB 查询失败时静默跳过）
    if let Some(db) = db {
        if let Ok(platforms) = db.list_custom_platforms() {
            for p in platforms {
                adapters.push(Box::new(custom::CustomPlatformAdapter::new(p)));
            }
        }
    }

    adapters
}

/// 按平台 ID 查找适配器
pub fn find_adapter(platform_id: &str, db: Option<&Db>) -> Option<Box<dyn PlatformAdapter>> {
    all_adapters(db)
        .into_iter()
        .find(|a| a.platform_id() == platform_id)
}

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn builtin_adapters_declare_text_capability() {
        for adapter in all_adapters(None) {
            let caps = adapter.capabilities();
            assert!(caps.text, "{} 必须支持文本发布", adapter.platform_id());
        }
    }

    #[test]
    fn validate_rejects_video_when_unsupported() {
        let caps = Capabilities::TEXT_ONLY;
        let content = PublishContent {
            title: None,
            text: "正文".into(),
            media_paths: vec!["/tmp/a.mp4".into()],
            cover_path: None,
            topics: Vec::new(),
            thread_segments: Vec::new(),
            schedule_at: None,
        };
        let err = caps.validate(&content).unwrap_err().to_string();
        assert!(err.contains("不支持"), "错误信息应说明不支持，实际: {err}");
    }

    #[test]
    fn validate_rejects_topics_and_thread_when_unsupported() {
        let caps = Capabilities::TEXT_ONLY;
        let mut content = PublishContent {
            title: None, text: "正文".into(), media_paths: Vec::new(),
            cover_path: None, topics: vec!["话题".into()],
            thread_segments: Vec::new(), schedule_at: None,
        };
        assert!(caps.validate(&content).is_err(), "话题不受支持时必须报错");

        content.topics.clear();
        content.thread_segments = vec!["第二段".into()];
        assert!(caps.validate(&content).is_err(), "线程不受支持时必须报错");
    }

    #[test]
    fn validate_accepts_content_within_capabilities() {
        let caps = Capabilities::TEXT_ONLY;
        let content = PublishContent {
            title: Some("标题".into()), text: "正文".into(), media_paths: Vec::new(),
            cover_path: None, topics: Vec::new(), thread_segments: Vec::new(), schedule_at: None,
        };
        assert!(caps.validate(&content).is_ok());
    }
}