//! 账号会话生命周期管理
//!
//! 每个账号对应一个独立的 Obscura BrowserContext：
//! - open: 创建 context + page，从数据库恢复加密的 cookie
//! - export_cookies: 导出页面 cookie，加密存入数据库
//! - close: 释放 context
//!
//! 会话注册表保存在内存中（account_id → session），
//! cookie 的持久化在 SQLite（加密），跨重启恢复。

use crate::db::{Account, Db};
use crate::obscura::client::CdpBrowser;
use crate::platforms;
use anyhow::{Context, Result};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use chromiumoxide::Page;
use std::collections::HashMap;
use std::sync::Mutex;
use tracing::{info, warn};

/// 账号会话：独立的 BrowserContext + Page
pub struct AccountSession {
    pub context_id: String,
    pub page: Page,
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
    /// 1. 创建独立 BrowserContext + Page（导航到平台创作者首页）
    /// 2. 若数据库有保存的 cookie，解密后恢复到页面
    pub async fn open(&self, cdp: &CdpBrowser, db: &Db, account: &Account) -> Result<()> {
        // 已存在则直接返回
        if self.get_page(account.id.as_str()).is_some() {
            return Ok(());
        }

        // 平台创作者首页 URL
        let creator_url = platforms::find_adapter(&account.platform)
            .map(|a| a.creator_url().to_string())
            .unwrap_or_else(|| "about:blank".to_string());

        info!("为账号 {} 创建会话（平台: {}）", account.name, account.platform);

        // 创建 context + page
        let (context_id, page) = cdp.create_context_page(&creator_url).await?;

        // 恢复已保存的 cookie
        if let Some(cookies_json) = db.get_account_cookies(&account.id)? {
            match restore_cookies(&page, &cookies_json).await {
                Ok(n) => info!("账号 {} 恢复了 {} 个 cookie", account.name, n),
                Err(e) => warn!("账号 {} cookie 恢复失败: {}", account.name, e),
            }
            // 恢复后刷新页面使 cookie 生效
            let _ = page.goto(&creator_url).await;
        }

        self.sessions
            .lock()
            .unwrap()
            .insert(account.id.clone(), AccountSession { context_id, page });

        Ok(())
    }

    /// 关闭账号会话（释放 context）
    pub async fn close(&self, cdp: &CdpBrowser, account_id: &str) -> Result<()> {
        let session = self.sessions.lock().unwrap().remove(account_id);
        if let Some(session) = session {
            info!("关闭账号 {} 的会话", account_id);
            cdp.dispose_context(&session.context_id).await?;
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

    /// 当前打开的会话数量
    pub fn len(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }

    /// 是否没有打开的会话
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
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

        let mut param = CookieParam::new(c.name.clone(), c.value.clone()).url(url);
        if let Some(path) = &c.path {
            param = param.path(path.clone());
        }
        if let Some(secure) = c.secure {
            param = param.secure(secure);
        }
        if let Some(http_only) = c.http_only {
            param = param.http_only(http_only);
        }

        if let Err(e) = page.set_cookie(param).await {
            warn!("恢复 cookie {}.{} 失败: {}", host, c.name, e);
        } else {
            count += 1;
        }
    }
    Ok(count)
}
