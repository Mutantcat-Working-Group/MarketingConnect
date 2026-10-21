//! 平台适配器
//!
//! 每个平台实现 PlatformAdapter trait 的三个核心接口：
//! - login: 登录流程（支持帐密自动登录）
//! - publish: 发布内容
//! - check_session: 会话健康检测（掉号判断）

use anyhow::Result;
use async_trait::async_trait;

mod douyin;
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
pub fn all_adapters() -> Vec<Box<dyn PlatformAdapter>> {
    vec![Box::new(super::platforms::douyin::DouyinAdapter::new())]
}

/// 按平台 ID 查找适配器
pub fn find_adapter(platform_id: &str) -> Option<Box<dyn PlatformAdapter>> {
    all_adapters()
        .into_iter()
        .find(|a| a.platform_id() == platform_id)
}
