//! 抖音平台适配器（骨架）
//!
//! ⚠️ 当前为骨架实现：选择器与流程需在 spike 验证后填充。
//! 抖音创作者平台为超重 SPA，Obscura 渲染兼容性是首要验证项。

use super::{PublishContent, PlatformAdapter};
use anyhow::{bail, Result};
use async_trait::async_trait;

pub struct DouyinAdapter;

impl DouyinAdapter {
    pub fn new() -> Self {
        DouyinAdapter
    }
}

#[async_trait]
impl PlatformAdapter for DouyinAdapter {
    fn platform_id(&self) -> &str {
        "douyin"
    }

    fn display_name(&self) -> &str {
        "抖音"
    }

    fn creator_url(&self) -> &str {
        "https://creator.douyin.com/creator-micro/home"
    }

    async fn login(
        &self,
        _page: &chromiumoxide::Page,
        _username: &str,
        _password: &str,
    ) -> Result<String> {
        // TODO(spike): 验证 obscura 渲染后填充登录流程
        // 1. 导航到 creator.douyin.com
        // 2. 切换到帐密登录 tab
        // 3. 填写用户名/密码并提交
        // 4. 等待登录成功，导出 cookie
        bail!("抖音登录流程待 spike 验证后实现")
    }

    async fn publish(&self, _page: &chromiumoxide::Page, _content: &PublishContent) -> Result<()> {
        // TODO(spike): 验证 obscura 渲染后填充发布流程
        // 1. 导航到发布页
        // 2. 上传媒体文件
        // 3. 填写标题/正文
        // 4. 点击发布
        bail!("抖音发布流程待 spike 验证后实现")
    }

    async fn check_session(&self, page: &chromiumoxide::Page) -> Result<bool> {
        // 基础检测：导航到创作者首页，若被重定向到登录页则掉号
        page.goto(self.creator_url()).await?;
        let url = page.url().await?.unwrap_or_default();
        Ok(!url.contains("login"))
    }
}
