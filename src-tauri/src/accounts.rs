//! 账号管理 Tauri 命令
//!
//! 前端通过这些命令与后端交互：账号 CRUD、Obscura 引擎控制、
//! 会话检测、发布任务管理。

use crate::db::{Account, AccountStatus, Db, NewAccount, PublishTask};
use crate::obscura::ObscuraManager;
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
pub fn delete_account(db: State<Db>, id: String) -> Result<(), String> {
    db.delete_account(&id).map_err(|e| e.to_string())
}

/// 启动 Obscura CDP 引擎
#[tauri::command]
pub async fn start_engine(
    app: tauri::AppHandle,
    obscura: State<'_, ObscuraManager>,
) -> Result<String, String> {
    obscura.start(&app).map_err(|e| e.to_string())?;
    Ok(format!("引擎已启动，端口 {}", obscura.port()))
}

/// 停止 Obscura CDP 引擎
#[tauri::command]
pub fn stop_engine(obscura: State<ObscuraManager>) -> Result<(), String> {
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

/// 账号会话检测（掉号检测）
#[tauri::command]
pub async fn check_account_session(
    db: State<'_, Db>,
    account_id: String,
) -> Result<bool, String> {
    // TODO: 接入 CDP 后实现真实检测
    // 当前返回 false 表示待检测
    let _ = db.update_account_status(&account_id, AccountStatus::Offline);
    Ok(false)
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
