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

mod csdn;
mod custom;
mod douyin;
mod wechat_mp;
mod xiaohongshu;
pub use csdn::CsdnAdapter;
pub use custom::{CustomPlatform, CustomPlatformAdapter};
pub use douyin::DouyinAdapter;
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

/// 发布内容
#[derive(Debug, Clone)]
pub struct PublishContent {
    pub title: Option<String>,
    pub text: String,
    pub media_paths: Vec<String>,
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

    /// 登录流程：使用帐密登录，返回登录后的 cookie JSON
    async fn login(&self, page: &chromiumoxide::Page, username: &str, password: &str) -> Result<String>;

    /// 发布内容
    async fn publish(&self, page: &chromiumoxide::Page, content: &PublishContent) -> Result<()>;

    /// 会话检测：返回 true 表示会话有效
    async fn check_session(&self, page: &chromiumoxide::Page) -> Result<bool>;
}

/// 依次尝试多个 CSS 选择器，找到第一个可用元素并填入文本
///
/// 各平台前端结构会随版本变化，通过候选选择器提高兼容性；
/// 返回 true 表示成功填入，全部失败返回 false。
pub(crate) async fn fill_first(
    page: &chromiumoxide::Page,
    selectors: &[&str],
    value: &str,
) -> bool {
    for sel in selectors {
        if let Ok(el) = page.find_element(*sel).await {
            if el.click().await.is_ok() && el.type_str(value).await.is_ok() {
                return true;
            }
        }
    }
    false
}

/// 依次尝试多个 CSS 选择器，点击第一个找到的元素
pub(crate) async fn click_first(page: &chromiumoxide::Page, selectors: &[&str]) -> bool {
    for sel in selectors {
        if let Ok(el) = page.find_element(*sel).await {
            if el.click().await.is_ok() {
                return true;
            }
        }
    }
    false
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