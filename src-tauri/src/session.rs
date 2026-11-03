// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
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

use crate::credentials::{Credentials, cookie_param, http_origin};
use crate::db::{Account, Db};
use crate::obscura::{CdpBrowser, CdpState, ObscuraManager};
use crate::platforms;
use anyhow::{Context, Result};
use chromiumoxide::cdp::browser_protocol::page::{
    AddScriptToEvaluateOnNewDocumentParams, RemoveScriptToEvaluateOnNewDocumentParams,
};
use chromiumoxide::Page;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use tracing::{info, warn};

/// 账号会话：独立的 BrowserContext + Page
pub struct AccountSession {
    pub context_id: String,
    pub page: Page,
    /// 所属 Obscura 实例端口（用于释放时定位）
    pub port: u16,
    /// Only this configured platform origin may export/restore JS storage.
    trusted_origin: Option<String>,
}

/// 交互式登录的保护时长
///
/// 该标记会暂停后台巡检与发布。用户中途离开（切走标签页、放着不管）时，
/// 标记必须能自行过期，否则账号会被永久锁死、发布持续失败。
const LOGIN_ACTIVE_TTL: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// 会话注册表（account_id → session）
pub struct SessionRegistry {
    sessions: Mutex<HashMap<String, AccountSession>>,
    /// account_id → 最近一次交互时间
    interactive_logins: Mutex<HashMap<String, std::time::Instant>>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        SessionRegistry {
            sessions: Mutex::new(HashMap::new()),
            interactive_logins: Mutex::new(HashMap::new()),
        }
    }

    /// Interactive login owns navigation while active; monitors must skip it.
    pub fn set_login_active(&self, account_id: &str, active: bool) {
        let mut logins = self.interactive_logins.lock().unwrap();
        if active {
            // 每次交互都刷新时间戳，长时间扫码/填表不会被误判为过期。
            logins.insert(account_id.to_string(), std::time::Instant::now());
        } else {
            logins.remove(account_id);
        }
    }

    pub fn is_login_active(&self, account_id: &str) -> bool {
        let mut logins = self.interactive_logins.lock().unwrap();
        match logins.get(account_id) {
            Some(started) if started.elapsed() < LOGIN_ACTIVE_TTL => true,
            Some(_) => {
                logins.remove(account_id);
                false
            }
            None => false,
        }
    }

    /// 为账号打开会话
    ///
    /// 1. 按账号代理获取/启动 Obscura 实例并建立 CDP 连接
    /// 2. 创建独立 BrowserContext + about:blank 页面，先恢复 cookie
    /// 3. 注册仅匹配平台来源的 storage 初始化脚本，再导航并移除脚本
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

        // Read/decrypt before creating a context, so DB failure cannot leak a context.
        let credentials = db.get_account_cookies(&account.id)
            .map_err(|_| anyhow::anyhow!("读取账号凭据失败"))?
            .and_then(|json| match Credentials::parse(&json) {
                Ok(saved) => Some(saved),
                Err(_) => {
                    warn!("账号保存的凭据不可读取，跳过恢复");
                    None
                }
            });
        let trusted_origin = http_origin(&creator_url);

        // Cookies must exist before the first request to the platform.
        let (context_id, page) = browser.create_context_page("about:blank").await?;
        if let Some(saved) = &credentials {
            let count = restore_cookies(&page, saved).await;
            info!("已恢复 {} 个 cookie", count);
        }

        // Install before navigation: a SPA may redirect to SSO before a later evaluate.
        // Never navigate to the saved URL; only the configured creator URL is trusted.
        let initialization = async {
            if let (Some(saved), Some(origin)) = (&credentials, trusted_origin.as_deref()) {
                if saved.storage_matches(origin)
                    && (!saved.local_storage.is_empty() || !saved.session_storage.is_empty())
                {
                    let script = storage_restore_script(saved, origin)?;
                    let identifier = page.evaluate_on_new_document(
                        AddScriptToEvaluateOnNewDocumentParams::new(script),
                    ).await.map_err(|_| anyhow::anyhow!("注册页面凭据初始化失败"))?;
                    return Ok(Some(identifier));
                }
            }
            Ok::<_, anyhow::Error>(None)
        }.await;
        let script_id = match initialization {
            Ok(identifier) => identifier,
            Err(error) => {
                let _ = browser.dispose_context(&context_id).await;
                return Err(error);
            }
        };
        let navigation = page.goto(creator_url.as_str()).await
            .map_err(|_| anyhow::anyhow!("平台页面导航失败"));
        // Always remove, including after navigation failure. If removal fails, dispose
        // the context instead of allowing stale credentials to overwrite later updates.
        let removal_failed = if let Some(identifier) = script_id {
            page.execute(RemoveScriptToEvaluateOnNewDocumentParams::new(identifier))
                .await.is_err()
        } else {
            false
        };
        if navigation.is_err() || removal_failed {
            let _ = browser.dispose_context(&context_id).await;
            return Err(if removal_failed {
                anyhow::anyhow!("移除页面凭据初始化失败")
            } else {
                anyhow::anyhow!("平台页面导航失败")
            });
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
                    trusted_origin,
                },
            );

        Ok(())
    }

    /// 关闭账号会话（释放 context）
    pub async fn close(&self, app: &AppHandle, account_id: &str) -> Result<()> {
        let session = self.sessions.lock().unwrap().remove(account_id);
        self.set_login_active(account_id, false);
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
        let (page, trusted_origin) = {
            let sessions = self.sessions.lock().unwrap();
            let session = sessions.get(account_id).context("账号会话不存在，无法导出凭据")?;
            (session.page.clone(), session.trusted_origin.clone())
        };

        // Browser-wide Storage.getCookies scoping is not verified for Obscura.
        // Network.getCookies via Page guarantees target isolation, at the cost of
        // omitting cookies that do not match the current page/frame URLs.
        let cookies = page.get_cookies().await
            .map_err(|_| anyhow::anyhow!("页面 cookie 导出失败"))?;
        let count = cookies.len();
        let values = serde_json::to_value(&cookies)
            .map_err(|_| anyhow::anyhow!("凭据序列化失败"))?;
        let mut saved = Credentials::new(values.as_array().cloned().unwrap_or_default());
        if let Some(origin) = trusted_origin {
            match export_storage(&page, &origin).await {
                Ok(Some(storage)) => {
                    saved.origin = Some(storage.origin);
                    saved.url = Some(storage.url);
                    saved.local_storage = storage.local_storage;
                    saved.session_storage = storage.session_storage;
                }
                Ok(None) => warn!("页面来源不匹配，跳过 storage 导出"),
                Err(_) => return Err(anyhow::anyhow!("页面 storage 导出失败，未覆盖已保存凭据")),
            }
        }
        // 空快照通常意味着页面被重定向到空白页或尚未真正登录；
        // 直接写入会用空数据覆盖可用凭据，导致下次打开直接掉登录。
        if count == 0
            && saved.local_storage.is_empty()
            && saved.session_storage.is_empty()
        {
            return Err(anyhow::anyhow!("未读取到任何登录凭证，已保留原有凭据"));
        }
        db.save_account_cookies(account_id, &saved.to_json()?)
            .map_err(|_| anyhow::anyhow!("保存账号凭据失败"))?;
        info!("已导出 {} 个 cookie", count);
        Ok(count)
    }

    /// 截取账号当前页面（PNG 字节）
    ///
    /// 用于无头模式下的登录辅助：用户查看截图完成扫码/填表登录。
    /// 只截取视口，确保截图坐标与页面交互坐标一致。
    pub async fn screenshot(&self, account_id: &str) -> Result<Vec<u8>> {
        let page = self
            .get_page(account_id)
            .context("账号会话不存在，无法截图")?;

        let params = chromiumoxide::page::ScreenshotParams::builder()
            .full_page(false)
            .build();

        let png = page
            .screenshot(params)
            .await
            .map_err(|_| anyhow::anyhow!("页面截图失败"))?;
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
    let cdp = app.state::<CdpState>();

    // 获取/启动账号代理对应的实例。
    // `get_or_start` 内部用阻塞 sleep 轮询端口，最多等 10 秒；
    // 直接在异步上下文里调用会卡住整个 Tokio 工作线程（连带 MCP、调度器）。
    let app_for_start = app.clone();
    let proxy = account.proxy_url.clone();
    let port = tokio::task::spawn_blocking(move || {
        let obscura = app_for_start.state::<ObscuraManager>();
        obscura.get_or_start(&app_for_start, proxy.as_deref())
    })
    .await
    .map_err(|e| anyhow::anyhow!("启动浏览器引擎任务失败: {}", e))??;

    // 获取/建立 CDP 连接
    let ws_url = crate::obscura::ws_url(port);
    let browser = match cdp.get_or_connect(port, &ws_url).await {
        Ok(browser) => browser,
        Err(first_error) => {
            // 端口可能仍可连接，但旧 WebSocket handler 已断开；丢弃缓存后只重连一次。
            warn!("CDP 连接失败，清理端口 {} 的缓存并重试: {}", port, first_error);
            cdp.remove(port).await;
            cdp.get_or_connect(port, &ws_url)
                .await
                .map_err(|retry_error| anyhow::anyhow!("CDP 重连失败（首次: {}; 重试: {}）", first_error, retry_error))?
        }
    };

    Ok((port, browser))
}

/// Restore independently so an obsolete cookie does not discard all other credentials.
async fn restore_cookies(page: &Page, saved: &Credentials) -> usize {
    let mut count = 0;
    let mut failed = 0;
    for cookie in &saved.cookies {
        match cookie_param(cookie) {
            Ok(param) => {
                if page.set_cookie(param).await.is_ok() {
                    count += 1;
                } else {
                    failed += 1;
                }
            }
            Err(_) => failed += 1,
        }
    }
    if failed > 0 {
        warn!("有 {} 个 cookie 未能恢复", failed);
    }
    count
}

#[derive(serde::Deserialize)]
struct PageStorage {
    origin: String,
    url: String,
    local_storage: BTreeMap<String, String>,
    session_storage: BTreeMap<String, String>,
}

async fn export_storage(page: &Page, trusted_origin: &str) -> Result<Option<PageStorage>> {
    let origin = serde_json::to_string(trusted_origin)
        .map_err(|_| anyhow::anyhow!("storage 来源序列化失败"))?;
    let script = format!(r#"(() => {{
        if (window.location.origin !== {origin}) return null;
        const snapshot = storage => {{
            const entries = [];
            for (let i = 0; i < storage.length; i++) {{
                const key = storage.key(i);
                if (key !== null) {{
                    const value = storage.getItem(key);
                    if (value !== null) entries.push([key, value]);
                }}
            }}
            return Object.fromEntries(entries);
        }};
        return {{ origin: window.location.origin, url: window.location.href,
            local_storage: snapshot(window.localStorage),
            session_storage: snapshot(window.sessionStorage) }};
    }})()"#);
    let storage: Option<PageStorage> = page.evaluate(script).await
        .map_err(|_| anyhow::anyhow!("storage 读取失败"))?
        .into_value().map_err(|_| anyhow::anyhow!("storage 数据格式无效"))?;
    // Defense in depth: no credential data is included in validation errors.
    Ok(storage.filter(|storage| storage.origin == trusted_origin
        && http_origin(&storage.url).as_deref() == Some(trusted_origin)))
}

fn storage_restore_script(saved: &Credentials, trusted_origin: &str) -> Result<String> {
    if !saved.storage_matches(trusted_origin) {
        return Err(anyhow::anyhow!("storage 来源不匹配"));
    }
    let data = serde_json::to_string(&serde_json::json!({
        "origin": trusted_origin,
        "local": saved.local_storage,
        "session": saved.session_storage,
    })).map_err(|_| anyhow::anyhow!("storage 序列化失败"))?;
    // Parse JSON rather than a JS object literal, preserving keys such as __proto__.
    let literal = serde_json::to_string(&data)
        .map_err(|_| anyhow::anyhow!("storage 序列化失败"))?;
    Ok(format!(r#"(() => {{
        // Only initialize the main platform document, never embedded login frames.
        if (window.top !== window) return;
        const saved = JSON.parse({literal});
        if (window.location.origin !== saved.origin) return;
        try {{
            for (const [key, value] of Object.entries(saved.local)) window.localStorage.setItem(key, value);
            for (const [key, value] of Object.entries(saved.session)) window.sessionStorage.setItem(key, value);
        }} catch (_) {{
            // Storage may be disabled. Do not throw or log credential-bearing script data.
        }}
    }})()"#))
}
