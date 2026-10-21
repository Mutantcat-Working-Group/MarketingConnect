//! 账号会话生命周期管理
//!
//! 每个账号对应一个独立的 Obscura BrowserContext：
//! - open: 按账号代理获取/启动 Obscura 实例，创建 context + page，
//!   从数据库恢复加密的 cookie
//! - export_cookies: 导出页面 cookie，加密存入数据库
//! - close: 释放 context
//!
//! 会话注册表保存在内存中（account_id → session），
//! cookie 的持久化在 SQLite（加密），跨重启恢复。

use crate::db::{Account, Db};
use crate::obscura::{CdpBrowser, CdpState, ObscuraManager};
use crate::platforms;
use anyhow::{Context, Result};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use chromiumoxide::Page;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use tracing::{info, warn};

/// 账号会话：独立的 BrowserContext + Page
pub struct AccountSession {
    pub context_id: String,
    pub page: Page,
    /// 所属 Obscura 实例端口（用于释放时定位）
    pub port: u16,
}

/// 会话注册表（account_id → session）
pub struct SessionRegistry {
    sessions: Mutex<HashMap<String, AccountSession>>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        SessionRegistry {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// 为账号打开会话
    ///
    /// 1. 按账号代理获取/启动 Obscura 实例并建立 CDP 连接
    /// 2. 创建独立 BrowserContext + Page（导航到平台创作者首页）
    /// 3. 若数据库有保存的 cookie，解密后恢复到页面
    pub async fn open(&self, app: &AppHandle, db: &Db, account: &Account) -> Result<()> {
        // 已存在则直接返回
        if self.get_page(account.id.as_str()).is_some() {
            return Ok(());
        }

        // 1. 解析实例与 CDP 连接
        let (port, browser) = resolve_browser(app, account).await?;

        // 2. 平台创作者首页 URL
        let creator_url = platforms::find_adapter(&account.platform, Some(db))
            .map(|a| a.creator_url().to_string())
            .unwrap_or_else(|| "about:blank".to_string());

        info!(
            "为账号 {} 创建会话（平台: {}，实例端口: {}）",
            account.name, account.platform, port
        );

        // 3. 创建 context + page
        let (context_id, page) = browser.create_context_page(&creator_url).await?;

        // 4. 恢复已保存的 cookie
        if let Some(cookies_json) = db.get_account_cookies(&account.id)? {
            match restore_cookies(&page, &cookies_json).await {
                Ok(n) => info!("账号 {} 恢复了 {} 个 cookie", account.name, n),
                Err(e) => warn!("账号 {} cookie 恢复失败: {}", account.name, e),
            }
            // 恢复后刷新页面使 cookie 生效
            let _ = page.goto(creator_url.as_str()).await;
        }

        self.sessions
            .lock()
            .unwrap()
            .insert(
                account.id.clone(),
                AccountSession {
                    context_id,
                    page,
                    port,
                },
            );

        Ok(())
    }

    /// 关闭账号会话（释放 context）
    pub async fn close(&self, app: &AppHandle, account_id: &str) -> Result<()> {
        let session = self.sessions.lock().unwrap().remove(account_id);
        if let Some(session) = session {
            info!("关闭账号 {} 的会话", account_id);
            // 通过端口找到对应的 CDP 连接释放 context
            let cdp = app.state::<CdpState>();
            if let Some(browser) = cdp.get(session.port).await {
                browser.dispose_context(&session.context_id).await?;
            }
        }
        Ok(())
    }

    /// 获取账号的 Page 克隆（不存在返回 None）
    pub fn get_page(&self, account_id: &str) -> Option<Page> {
        self.sessions
            .lock()
            .unwrap()
            .get(account_id)
            .map(|s| s.page.clone())
    }

    /// 导出账号当前页面的 cookie，加密存入数据库
    pub async fn export_cookies(&self, db: &Db, account_id: &str) -> Result<usize> {
        let page = self
            .get_page(account_id)
            .context("账号会话不存在，无法导出 cookie")?;

        let cookies = page.get_cookies().await?;
        let json = serde_json::to_string(&cookies).context("cookie 序列化失败")?;
        db.save_account_cookies(account_id, &json)?;
        info!("账号 {} 导出了 {} 个 cookie", account_id, cookies.len());
        Ok(cookies.len())
    }

    /// 截取账号当前页面（PNG 字节）
    ///
    /// 用于无头模式下的登录辅助：用户查看截图完成扫码/填表登录。
    /// 默认截取整页（full_page），适配登录页的滚动内容。
    pub async fn screenshot(&self, account_id: &str) -> Result<Vec<u8>> {
        let page = self
            .get_page(account_id)
            .context("账号会话不存在，无法截图")?;

        let params = chromiumoxide::page::ScreenshotParams::builder()
            .full_page(true)
            .build();

        let png = page
            .screenshot(params)
            .await
            .context("页面截图失败")?;
        Ok(png)
    }

    /// 当前打开的会话数量
    pub fn len(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }

    /// 是否没有打开的会话
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 当前打开会话的账号 ID 列表
    pub fn open_account_ids(&self) -> Vec<String> {
        self.sessions.lock().unwrap().keys().cloned().collect()
    }
}

/// 按账号代理解析 Obscura 实例与 CDP 连接
///
/// 返回 (实例端口, CDP 浏览器连接)。
pub async fn resolve_browser(
    app: &AppHandle,
    account: &Account,
) -> Result<(u16, Arc<CdpBrowser>)> {
    let obscura = app.state::<ObscuraManager>();
    let cdp = app.state::<CdpState>();

    // 获取/启动账号代理对应的实例
    let port = obscura.get_or_start(app, account.proxy_url.as_deref())?;

    // 获取/建立 CDP 连接
    let browser = cdp.get_or_connect(port, &crate::obscura::ws_url(port)).await?;

    Ok((port, browser))
}

/// 把 JSON 格式的 cookie 数组恢复到页面
///
/// cookie JSON 结构来自 CDP 的 `Network.getCookies` 返回值。
/// 恢复时按每个 cookie 的 domain 构造 URL 后逐条设置。
async fn restore_cookies(page: &Page, cookies_json: &str) -> Result<usize> {
    #[derive(serde::Deserialize)]
    struct SavedCookie {
        name: String,
        value: String,
        domain: String,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        secure: Option<bool>,
        #[serde(default)]
        http_only: Option<bool>,
    }

    let cookies: Vec<SavedCookie> =
        serde_json::from_str(cookies_json).context("cookie JSON 解析失败")?;

    let mut count = 0;
    for c in &cookies {
        // 按 domain 构造 URL（去掉前导点）
        let host = c.domain.trim_start_matches('.');
        if host.is_empty() {
            continue;
        }
        let url = if c.secure.unwrap_or(false) {
            format!("https://{}", host)
        } else {
            format!("http://{}", host)
        };

        let mut builder = CookieParam::builder()
            .name(c.name.clone())
            .value(c.value.clone())
            .url(url);
        if let Some(path) = &c.path {
            builder = builder.path(path.clone());
        }
        if let Some(secure) = c.secure {
            builder = builder.secure(secure);
        }
        if let Some(http_only) = c.http_only {
            builder = builder.http_only(http_only);
        }
        let param = builder
            .build()
            .map_err(|e| anyhow::anyhow!("CookieParam 构建失败: {}", e))?;

        if let Err(e) = page.set_cookie(param).await {
            warn!("恢复 cookie {}.{} 失败: {}", host, c.name, e);
        } else {
            count += 1;
        }
    }
    Ok(count)
}
