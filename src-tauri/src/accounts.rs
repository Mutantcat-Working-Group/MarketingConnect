//! 账号管理 Tauri 命令
//!
//! 前端通过这些命令与后端交互：账号 CRUD、Obscura 引擎控制、
//! 会话管理（开关/检测/cookie 持久化）、发布任务管理。

use crate::db::{Account, AccountStatus, Db, NewAccount, PublishTask};
use crate::obscura::ObscuraManager;
use crate::session::SessionRegistry;
use tauri::{AppHandle, State};

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

/// 删除账号（联动关闭会话）
#[tauri::command]
pub async fn delete_account(
    app: AppHandle,
    db: State<'_, Db>,
    sessions: State<'_, SessionRegistry>,
    id: String,
) -> Result<(), String> {
    let _ = sessions.close(&app, &id).await;
    db.delete_account(&id).map_err(|e| e.to_string())
}

/// 启动 Obscura 引擎（直连实例）
///
/// 带代理的账号会在打开会话时按需自动启动对应实例。
#[tauri::command]
pub fn start_engine(
    app: AppHandle,
    obscura: State<'_, ObscuraManager>,
) -> Result<String, String> {
    let port = obscura
        .get_or_start(&app, None)
        .map_err(|e| e.to_string())?;
    Ok(format!("引擎已启动，直连实例端口 {}", port))
}

/// 停止 Obscura 引擎（所有实例）
#[tauri::command]
pub fn stop_engine(obscura: State<ObscuraManager>) -> Result<(), String> {
    obscura.stop_all();
    Ok(())
}

/// 引擎状态查询
#[tauri::command]
pub fn engine_status(obscura: State<ObscuraManager>) -> Result<serde_json::Value, String> {
    Ok(obscura.status())
}

/// 打开账号会话（创建独立 context，恢复 cookie）
#[tauri::command]
pub async fn open_account_session(
    app: AppHandle,
    db: State<'_, Db>,
    sessions: State<'_, SessionRegistry>,
    account_id: String,
) -> Result<(), String> {
    let account = find_account(&db, &account_id)?;
    sessions
        .open(&app, &db, &account)
        .await
        .map_err(|e| e.to_string())
}

/// 关闭账号会话
#[tauri::command]
pub async fn close_account_session(
    app: AppHandle,
    sessions: State<'_, SessionRegistry>,
    account_id: String,
) -> Result<(), String> {
    sessions
        .close(&app, &account_id)
        .await
        .map_err(|e| e.to_string())
}

/// 账号会话检测（掉号检测）
///
/// 通过平台适配器的 check_session 判断会话是否有效，
/// 并更新账号状态；掉号时发送系统通知。
#[tauri::command]
pub async fn check_account_session(
    app: AppHandle,
    db: State<'_, Db>,
    sessions: State<'_, SessionRegistry>,
    account_id: String,
) -> Result<bool, String> {
    use tauri_plugin_notification::NotificationExt;

    let account = find_account(&db, &account_id)?;

    // 确保会话已打开
    sessions
        .open(&app, &db, &account)
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

    // 掉号时发送系统通知 + 短信提醒
    if !valid {
        let _ = app
            .notification()
            .builder()
            .title("营销通 - 账号掉号提醒")
            .body(&format!(
                "账号「{}」（{}）登录已失效，请及时重新登录",
                account.name, account.platform
            ))
            .show();

        // 短信提醒（配置了才发送，失败不影响主流程）
        if let Ok(Some(sms_config)) = db.get_sms_config() {
            if sms_config.is_configured() {
                let name = account.name.clone();
                let platform = account.platform.clone();
                tauri::async_runtime::spawn(async move {
                    match crate::sms::send_offline_alert(&sms_config, &name, &platform).await {
                        Ok(()) => {
                            tracing::info!("掉号短信已发送（账号 {}）", name);
                        }
                        Err(e) => {
                            tracing::warn!("掉号短信发送失败: {}", e);
                        }
                    }
                });
            }
        }
    }

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
///
/// 若带 cron 表达式，自动注册到定时调度器。
#[tauri::command]
pub fn create_publish_task(
    app: AppHandle,
    db: State<'_, Db>,
    account_id: String,
    title: Option<String>,
    content: Option<String>,
    media_paths: Option<String>,
    cron_expr: Option<String>,
    scheduled_at: Option<i64>,
) -> Result<PublishTask, String> {
    let task = db
        .create_task(
            &account_id,
            title.as_deref(),
            content.as_deref(),
            media_paths.as_deref(),
            cron_expr.as_deref(),
            scheduled_at,
        )
        .map_err(|e| e.to_string())?;

    // 带 cron 表达式：注册定时作业
    if let Some(cron) = task.cron_expr.clone() {
        if let Some(state) = app.try_state::<Arc<crate::scheduler::PublishScheduler>>() {
            let scheduler = state.inner().clone();
            let app_clone = app.clone();
            let task_id = task.id.clone();
            let account_id = account_id.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = scheduler
                    .add_job(app_clone, &cron, task_id, account_id)
                    .await
                {
                    tracing::error!("注册定时任务失败: {}", e);
                }
            });
        } else {
            tracing::warn!("调度器尚未就绪，任务 {} 未注册定时", task.id);
        }
    }

    Ok(task)
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

// ---------- 短信配置 ----------

/// 获取短信配置（密钥脱敏返回）
#[tauri::command]
pub fn get_sms_config(db: State<'_, Db>) -> Result<serde_json::Value, String> {
    let config = db.get_sms_config().map_err(|e| e.to_string())?;
    match config {
        Some(c) => {
            // 密钥脱敏：只显示前 4 位
            let masked = if c.access_key_secret.len() > 4 {
                format!("{}****", &c.access_key_secret[..4])
            } else if c.access_key_secret.is_empty() {
                String::new()
            } else {
                "****".to_string()
            };
            Ok(serde_json::json!({
                "access_key_id": c.access_key_id,
                "access_key_secret_masked": masked,
                "has_secret": !c.access_key_secret.is_empty(),
                "sign_name": c.sign_name,
                "template_code": c.template_code,
                "phone_numbers": c.phone_numbers,
                "configured": c.is_configured(),
            }))
        }
        None => Ok(serde_json::json!({
            "access_key_id": "",
            "access_key_secret_masked": "",
            "has_secret": false,
            "sign_name": "",
            "template_code": "",
            "phone_numbers": "",
            "configured": false,
        })),
    }
}

/// 保存短信配置
///
/// secret 为空字符串时保留原有密钥（用于前端只更新非敏感字段）。
#[tauri::command]
pub fn save_sms_config(
    db: State<'_, Db>,
    access_key_id: String,
    access_key_secret: String,
    sign_name: String,
    template_code: String,
    phone_numbers: String,
) -> Result<(), String> {
    // 读取现有配置以保留未提供的密钥
    let existing = db.get_sms_config().map_err(|e| e.to_string())?;
    let secret = if access_key_secret.is_empty() {
        existing.map(|c| c.access_key_secret).unwrap_or_default()
    } else {
        access_key_secret
    };

    let config = crate::sms::SmsConfig {
        access_key_id,
        access_key_secret: secret,
        sign_name,
        template_code,
        phone_numbers,
    };
    db.save_sms_config(&config).map_err(|e| e.to_string())
}

/// 发送测试短信
#[tauri::command]
pub async fn test_sms(db: State<'_, Db>) -> Result<String, String> {
    let config = db
        .get_sms_config()
        .map_err(|e| e.to_string())?
        .ok_or("短信配置不存在，请先保存配置")?;

    if !config.is_configured() {
        return Err("短信配置不完整，请检查所有必填项".into());
    }

    crate::sms::send_sms(
        &config,
        &[("account", "测试"), ("platform", "营销通")],
    )
    .await
    .map_err(|e| e.to_string())?;

    let _ = db.log_event(None, "info", "sms_test", Some("测试短信发送成功"));
    Ok("测试短信已发送".into())
}

/// 从数据库查找账号
fn find_account(db: &Db, account_id: &str) -> std::result::Result<Account, String> {
    db.list_accounts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| "账号不存在".to_string())
}
