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
//! - 内置适配器（douyin 等，静态注册）
//! - 自定义平台（用户在设置中注册，存于 DB，由 CustomPlatformAdapter 驱动）

use crate::db::Db;
use anyhow::Result;
use async_trait::async_trait;

mod custom;
mod douyin;
pub use custom::{CustomPlatform, CustomPlatformAdapter};
pub use douyin::DouyinAdapter;

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

/// 获取所有已注册的平台适配器
///
/// db 为 Some 时追加用户注册的自定义平台。
pub fn all_adapters(db: Option<&Db>) -> Vec<Box<dyn PlatformAdapter>> {
    let mut adapters: Vec<Box<dyn PlatformAdapter>> =
        vec![Box::new(douyin::DouyinAdapter::new())];

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
