//! CDP 客户端封装（基于 chromiumoxide 0.7）
//!
//! 连接到 Obscura 的 CDP 服务，为每个账号创建独立的
//! BrowserContext（cookie/storage 隔离 + 指纹随机化）。

use anyhow::{Context, Result};
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::target::{
    CreateBrowserContextParams, CreateTargetParams,
};
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use tracing::info;

/// CDP 浏览器连接（全局单例）
pub struct CdpBrowser {
    browser: Browser,
    _handler_task: tokio::task::JoinHandle<()>,
}

impl CdpBrowser {
    /// 连接到指定 WebSocket 地址的 CDP 服务
    ///
    /// ws:// 开头的地址会直接作为 websocket 端点使用；
    /// http:// 开头的地址会先通过 /json/version 自动发现 websocket 地址。
    pub async fn connect(ws_url: &str) -> Result<Self> {
        let (browser, mut handler) = Browser::connect(ws_url)
            .await
            .context("连接 CDP 服务失败")?;

        // handler 必须在后台任务中持续运行以处理 CDP 消息
        let handler_task = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(e) = event {
                    tracing::warn!("CDP handler 错误: {}", e);
                    break;
                }
            }
        });

        info!("CDP 连接成功: {}", ws_url);
        Ok(CdpBrowser {
            browser,
            _handler_task: handler_task,
        })
    }

    /// 创建独立的浏览器上下文并打开页面
    ///
    /// 返回 (context_id, page)。每个账号调用一次，
    /// context 之间 cookie/storage 完全隔离。
    pub async fn create_context_page(&self, url: &str) -> Result<(String, chromiumoxide::Page)> {
        let ctx_id = self
            .browser
            .create_browser_context(CreateBrowserContextParams::default())
            .await
            .context("创建 BrowserContext 失败")?;

        let params = CreateTargetParams::builder()
            .url(url)
            .browser_context_id(ctx_id.clone())
            .build()
            .map_err(|e| anyhow::anyhow!("CreateTargetParams 构建失败: {}", e))?;

        let page = self
            .browser
            .new_page(params)
            .await
            .context("创建页面失败")?;

        Ok((ctx_id.inner().clone(), page))
    }

    /// 释放浏览器上下文
    pub async fn dispose_context(&self, context_id: &str) -> Result<()> {
        self.browser
            .dispose_browser_context(context_id)
            .await
            .context("释放 BrowserContext 失败")?;
        Ok(())
    }
}

/// 全局 CDP 状态（Tauri managed state）
pub struct CdpState {
    inner: TokioMutex<Option<Arc<CdpBrowser>>>,
}

impl CdpState {
    pub fn new() -> Self {
        CdpState {
            inner: TokioMutex::new(None),
        }
    }

    /// 获取已连接的浏览器，未连接则返回 None
    pub async fn get(&self) -> Option<Arc<CdpBrowser>> {
        self.inner.lock().await.clone()
    }

    /// 设置已连接的浏览器
    pub async fn set(&self, browser: Arc<CdpBrowser>) {
        *self.inner.lock().await = Some(browser);
    }

    /// 清除连接
    pub async fn clear(&self) {
        *self.inner.lock().await = None;
    }
}
