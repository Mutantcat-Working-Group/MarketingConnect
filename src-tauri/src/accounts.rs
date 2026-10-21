//! 账号管理 Tauri 命令
//!
//! 前端通过这些命令与后端交互：账号 CRUD、Obscura 引擎控制、
//! 会话管理（开关/检测/cookie 持久化）、发布任务管理。

use crate::db::{Account, AccountStatus, Db, NewAccount, PublishTask};
use crate::obscura::{CdpState, ObscuraManager};
use crate::session::SessionRegistry;
use anyhow::Result;
use tauri::State;

/// 列出所有账号
#[tauri::command]
pub fn list_accounts(db: State<Db>) -> Result<Vec<Account>, String> {
    db.list_accounts().map_err(|e| e.to_string())
}

/// 新增账号
#[tauri::command]
pub fn create_account(db: State<Db>, input: NewAccount) -> Result<Account, String> {
    db.create_account(&input).map_err(|e| e.to_string())
}

/// 删除账号
#[tauri::command]
pub async fn delete_account(
    db: State<'_, Db>,
    cdp: State<'_, CdpState>,
    sessions: State<'_, SessionRegistry>,
    obscura: State<'_, ObscuraManager>,
    id: String,
) -> Result<(), String> {
    // 先关闭会话（若引擎在运行）
    if let Some(browser) = cdp.get().await {
        let _ = sessions.close(&browser, &id).await;
    }
    let _ = obscura; // 保持引用，避免提前释放
    db.delete_account(&id).map_err(|e| e.to_string())
}

/// 启动 Obscura CDP 引擎（进程 + CDP 连接）
#[tauri::command]
pub async fn start_engine(
    app: tauri::AppHandle,
    obscura: State<'_, ObscuraManager>,
    cdp: State<'_, CdpState>,
) -> Result<String, String> {
    // 1. 启动 obscura serve 进程
    obscura.start(&app).map_err(|e| e.to_string())?;

    // 2. 建立 CDP 连接
    if cdp.get().await.is_none() {
        let browser = crate::obscura::client::CdpBrowser::connect(&obscura.ws_url())
            .await
            .map_err(|e| format!("CDP 连接失败: {}", e))?;
        cdp.set(std::sync::Arc::new(browser)).await;
    }

    Ok(format!("引擎已启动，端口 {}", obscura.port()))
}

/// 停止 Obscura CDP 引擎
#[tauri::command]
pub async fn stop_engine(
    cdp: State<'_, CdpState>,
    obscura: State<'_, ObscuraManager>,
) -> Result<(), String> {
    cdp.clear().await;
    obscura.stop();
    Ok(())
}

/// 引擎状态查询
#[tauri::command]
pub fn engine_status(obscura: State<ObscuraManager>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "running": obscura.is_port_open(),
        "port": obscura.port(),
        "ws_url": obscura.ws_url(),
    }))
}

/// 打开账号会话（创建独立 context，恢复 cookie）
#[tauri::command]
pub async fn open_account_session(
    db: State<'_, Db>,
    cdp: State<'_, CdpState>,
    sessions: State<'_, SessionRegistry>,
    account_id: String,
) -> Result<(), String> {
    let browser = cdp
        .get()
        .await
        .ok_or("引擎未启动，请先启动引擎")?;

    let accounts = db.list_accounts().map_err(|e| e.to_string())?;
    let account = accounts
        .iter()
        .find(|a| a.id == account_id)
        .ok_or("账号不存在")?;

    sessions
        .open(&browser, &db, account)
        .await
        .map_err(|e| e.to_string())
}

/// 关闭账号会话
#[tauri::command]
pub async fn close_account_session(
    cdp: State<'_, CdpState>,
    sessions: State<'_, SessionRegistry>,
    account_id: String,
) -> Result<(), String> {
    if let Some(browser) = cdp.get().await {
        sessions
            .close(&browser, &account_id)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 账号会话检测（掉号检测）
///
/// 通过平台适配器的 check_session 判断会话是否有效，
/// 并更新账号状态。
#[tauri::command]
pub async fn check_account_session(
    db: State<'_, Db>,
    cdp: State<'_, CdpState>,
    sessions: State<'_, SessionRegistry>,
    account_id: String,
) -> Result<bool, String> {
    let browser = cdp
        .get()
        .await
        .ok_or("引擎未启动，请先启动引擎")?;

    let accounts = db.list_accounts().map_err(|e| e.to_string())?;
    let account = accounts
        .iter()
        .find(|a| a.id == account_id)
        .ok_or("账号不存在")?;

    // 确保会话已打开
    sessions
        .open(&browser, &db, account)
        .await
        .map_err(|e| e.to_string())?;

    let page = sessions
        .get_page(&account_id)
        .ok_or("会话打开失败")?;

    // 调用平台适配器检测会话
    let valid = match crate::platforms::find_adapter(&account.platform) {
        Some(adapter) => adapter
            .check_session(&page)
            .await
            .map_err(|e| e.to_string())?,
        None => {
            return Err(format!("不支持的平台: {}", account.platform));
        }
    };

    // 更新账号状态
    let status = if valid {
        AccountStatus::Online
    } else {
        AccountStatus::Offline
    };
    let _ = db.update_account_status(&account_id, status.clone());
    let _ = db.log_event(
        Some(&account_id),
        if valid { "info" } else { "warn" },
        "session_check",
        Some(if valid { "会话有效" } else { "会话已掉线" }),
    );

    Ok(valid)
}

/// 导出账号 cookie 到数据库（加密）
#[tauri::command]
pub async fn save_account_cookies(
    db: State<'_, Db>,
    sessions: State<'_, SessionRegistry>,
    account_id: String,
) -> Result<usize, String> {
    sessions
        .export_cookies(&db, &account_id)
        .await
        .map_err(|e| e.to_string())
}

/// 创建发布任务
#[tauri::command]
pub fn create_publish_task(
    db: State<'_, Db>,
    account_id: String,
    title: Option<String>,
    content: Option<String>,
    media_paths: Option<String>,
    cron_expr: Option<String>,
    scheduled_at: Option<i64>,
) -> Result<PublishTask, String> {
    db.create_task(
        &account_id,
        title.as_deref(),
        content.as_deref(),
        media_paths.as_deref(),
        cron_expr.as_deref(),
        scheduled_at,
    )
    .map_err(|e| e.to_string())
}

/// 列出账号的发布任务
#[tauri::command]
pub fn list_publish_tasks(
    db: State<'_, Db>,
    account_id: String,
) -> Result<Vec<PublishTask>, String> {
    db.list_tasks(&account_id).map_err(|e| e.to_string())
}

/// 查询最近事件日志
#[tauri::command]
pub fn recent_logs(
    db: State<'_, Db>,
    limit: Option<i64>,
) -> Result<Vec<crate::db::EventLog>, String> {
    db.recent_logs(limit.unwrap_or(50)).map_err(|e| e.to_string())
}

/// 获取支持的平台列表
#[tauri::command]
pub fn list_platforms() -> Result<Vec<serde_json::Value>, String> {
    Ok(crate::platforms::all_adapters()
        .iter()
        .map(|a| serde_json::json!({
            "id": a.platform_id(),
            "name": a.display_name(),
        }))
        .collect())
}
