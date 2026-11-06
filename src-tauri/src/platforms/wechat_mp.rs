// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 微信公众号平台适配器
//!
//! 公众平台：https://mp.weixin.qq.com/
//!
//! 登录流程：
//! 1. 导航到公众平台首页，呈现二维码登录
//! 2. 用户用微信 App 扫码并在手机上确认
//! 3. 登录成功后 URL 变为
//!    `https://mp.weixin.qq.com/cgi-bin/home?t=home/index&token=xxx&lang=zh_CN`
//!    （其中 token 为本次登录会话的凭证，随 cookie 一起持久化）
//!
//! 发布说明：公众号图文编辑页的 URL 依赖登录 token，且正文编辑器为
//! iframe（UEditor）结构，静态选择器无法可靠填充，故自动发文暂不实现。
//! 可打开的会话中用截图视图手动编辑发布，
//! 或在「设置 → 自定义平台」中按实际情况配置选择器。

use super::{PlatformAdapter, PublishContent};
use anyhow::{bail, Result};
use async_trait::async_trait;
use chromiumoxide::Page;

/// 公众平台首页（未登录为扫码登录页，登录后跳转 cgi-bin/home）
const CREATOR_URL: &str = "https://mp.weixin.qq.com/";

pub struct WechatMpAdapter;

impl WechatMpAdapter {
    pub fn new() -> Self {
        WechatMpAdapter
    }
}

/// 判断当前 URL 是否处于已登录状态
///
/// 未登录时停留在 `https://mp.weixin.qq.com/`（无 token）；
/// 登录后会带上 `token=` 并进入 `cgi-bin/` 路径。
fn is_logged_in(url: &str) -> bool {
    url.contains("token=") || url.contains("cgi-bin/")
}

#[async_trait]
impl PlatformAdapter for WechatMpAdapter {
    fn platform_id(&self) -> &str {
        "wechat_mp"
    }

    fn display_name(&self) -> &str {
        "公众号"
    }

    fn creator_url(&self) -> &str {
        CREATOR_URL
    }

    fn capabilities(&self) -> super::Capabilities {
        // publish() 目前只会 bail!（正文为 iframe 编辑器，暂不支持自动发文），
        // 故 publish 及其余能力位全部为 false，避免用户误以为可自动发布。
        super::Capabilities {
            publish: false,
            text: false, images: false, video: false, multi_image: false,
            cover: false, topics: false, thread: false, schedule: false,
        }
    }

    async fn login(&self, page: &Page, _username: &str, _password: &str) -> Result<String> {
        page.goto(CREATOR_URL)
            .await
            .map_err(|e| anyhow::anyhow!("导航到微信公众平台失败: {}", e))?;

        // 扫码登录需要较长时间，等待二维码渲染
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        let cookies = page
            .get_cookies()
            .await
            .map_err(|e| anyhow::anyhow!("获取 cookie 失败: {}", e))?;
        let json =
            serde_json::to_string(&cookies).map_err(|e| anyhow::anyhow!("cookie 序列化失败: {}", e))?;

        let url = page.url().await?.unwrap_or_default();
        if !is_logged_in(&url) {
            bail!("尚未登录：请在截图视图中用微信扫码并在手机上确认，成功后点击「保存 Cookie」")
        }

        Ok(json)
    }

    async fn publish(&self, _page: &Page, _content: &PublishContent) -> Result<()> {
        // 公众号图文编辑页 URL 需要登录 token，且正文为 iframe 富文本编辑器，
        // 无法用静态选择器可靠填充；宁可明确拒绝，也不做可能产生半成品草稿的操作。
        bail!(
            "公众号自动发文暂不支持（正文为 iframe 编辑器且发布页依赖登录 token）：\
             请打开会话后在截图视图中手动编辑发布，或在「自定义平台」中配置选择器"
        )
    }

    async fn check_session(&self, page: &Page) -> Result<bool> {
        super::auth::authenticated_probe(page, self.creator_url(), None).await
    }
}