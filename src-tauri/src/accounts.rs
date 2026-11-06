// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 账号管理 Tauri 命令
//!
//! 前端通过这些命令与后端交互：账号 CRUD、Obscura 引擎控制、
//! 会话管理（开关/检测/cookie 持久化）、发布任务管理。

use crate::db::{Account, AccountStatus, Db, NewAccount, PublishTask};
use crate::obscura::ObscuraManager;
use crate::session::SessionRegistry;
use base64::Engine;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

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
///
/// 必须连同会话与 CDP 连接一起失效：只杀进程会让界面留着指向死引擎的
/// 「已打开会话」，之后的操作会以难以理解的方式失败。
#[tauri::command]
pub async fn stop_engine(
    app: AppHandle,
    obscura: State<'_, ObscuraManager>,
    sessions: State<'_, SessionRegistry>,
) -> Result<(), String> {
    for account_id in sessions.open_account_ids() {
        if let Err(error) = sessions.close(&app, &account_id).await {
            tracing::warn!("停止引擎前关闭账号 {} 的会话失败: {}", account_id, error);
        }
    }
    obscura.stop_all();
    app.state::<crate::obscura::CdpState>().clear_all().await;
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

/// 查询账号会话状态（不导航、不检测登录态）
#[tauri::command]
pub fn account_session_status(
    sessions: State<'_, SessionRegistry>,
    account_id: String,
) -> serde_json::Value {
    serde_json::json!({
        "open": sessions.get_page(&account_id).is_some(),
        "login_active": sessions.is_login_active(&account_id),
    })
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
/// - 令牌型平台：用 `db.get_api_token` 取令牌 → `verify_token` 判定，**不打开
///   浏览器会话**；未配置令牌返回错误但**不标记掉号**（那是没配置，不是掉号）。
/// - 浏览器族：打开会话后用适配器的 `check_session` 页面探测。
///
/// 检测结果会更新账号状态；确认掉号时发送系统通知（与短信提醒）。
#[tauri::command]
pub async fn check_account_session(
    app: AppHandle,
    db: State<'_, Db>,
    sessions: State<'_, SessionRegistry>,
    account_id: String,
) -> Result<bool, String> {
    use tauri_plugin_notification::NotificationExt;

    let account = find_account(&db, &account_id)?;
    let adapter = crate::platforms::find_adapter(&account.platform, Some(&db))
        .ok_or_else(|| format!("不支持的平台: {}", account.platform))?;

    let valid = if adapter.token_hint().is_some() {
        // 令牌型平台没有浏览器会话：绝不打开会话，也绝不用 check_session
        // 的页面探测结果当掉号依据（那会把已配置令牌的账号误判为掉号）。
        match crate::platforms::check_token_session(&db, adapter.as_ref(), &account_id).await {
            Ok(crate::platforms::TokenSession::Valid) => true,
            Ok(crate::platforms::TokenSession::Invalid(reason)) => {
                tracing::warn!("账号 {} 令牌校验失败: {}", account.name, reason);
                false
            }
            Ok(crate::platforms::TokenSession::NotConfigured) => {
                tracing::warn!("账号 {} 为令牌型平台但未配置 API 令牌", account.name);
                // 不更新状态、不告警：引导用户去配置令牌即可。
                return Err("该账号尚未配置 API 令牌，请在账号设置中粘贴".into());
            }
            Err(e) => return Err(format!("读取 API 令牌失败: {}", e)),
        }
    } else {
        // 浏览器族：确保会话已打开再做页面探测。
        sessions
            .open(&app, &db, &account)
            .await
            .map_err(|e| e.to_string())?;

        let page = sessions
            .get_page(&account_id)
            .ok_or("会话打开失败")?;

        if sessions.is_login_active(&account_id) {
            return Err("正在交互登录，请在登录面板点击确认登录并保存".into());
        }

        adapter
            .check_session(&page)
            .await
            .map_err(|e| e.to_string())?
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
    if !valid && account.status == AccountStatus::Online {
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

/// 保存并校验平台 API 令牌（仅令牌型平台）
///
/// 校验通过后才加密落库：先解析适配器，再看 `token_hint()` 是否声明支持
/// 令牌登录，最后调用 `verify_token`。浏览器族平台（抖音/小红书/公众号/
/// CSDN/自定义平台）的默认 `token_hint()` 返回 None，因此这里会直接报错，
/// 不会写入任何令牌。
#[tauri::command]
pub async fn save_platform_token(
    db: State<'_, Db>,
    account_id: String,
    token: String,
) -> Result<(), String> {
    // `Db` 没有 `get_account`，复用 accounts.rs 里已有的 `find_account`：
    // 它返回 Result<Account, String>，与本命令返回类型一致，可直接 `?`。
    let account = find_account(&db, &account_id)?;
    let adapter = crate::platforms::find_adapter(&account.platform, Some(&db))
        .ok_or_else(|| "平台适配器不存在".to_string())?;
    if adapter.token_hint().is_none() {
        return Err("该平台不支持令牌登录".to_string());
    }
    let token = token.trim();
    adapter
        .verify_token(token)
        .await
        .map_err(|e| e.to_string())?;
    db.save_api_token(&account_id, token).map_err(|e| e.to_string())?;
    Ok(())
}

/// 查询账号是否已配置 API 令牌（不解密，不触发系统钥匙串）
///
/// 前端据此显示「已配置 / 未配置」：只读取密文列是否存在且非空，
/// 不调用 `Db::get_api_token`，因此不会访问系统钥匙串。
/// 复用 `find_account` 先确认账号存在，账号不存在时明确报「账号不存在」，
/// 不静默返回 false——否则前端会把「账号已被删除」误判为「未配置令牌」。
#[tauri::command]
pub fn api_token_configured(db: State<'_, Db>, account_id: String) -> Result<bool, String> {
    find_account(&db, &account_id)?;
    db.has_api_token(&account_id).map_err(|e| e.to_string())
}

/// 截取账号当前页面（返回 base64 PNG）
///
/// 用于无头模式下的登录辅助：用户查看截图完成扫码/填表登录。
#[tauri::command]
pub async fn capture_screenshot(
    sessions: State<'_, SessionRegistry>,
    account_id: String,
) -> Result<String, String> {
    let png = sessions
        .screenshot(&account_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(png))
}

/// 创建发布任务
///
/// 若带 cron 表达式，同步注册到定时调度器；注册失败会明确报错，
/// 不会出现「界面提示创建成功但任务永远不会执行」的静默失败。
///
/// `cover_path` / `topics` / `thread_segments` 是发布内容扩展字段：本任务
/// 只做到「数据库列 ←→ 调度」贯通与命令/`api.ts` 可选参数，前端可视化控件
/// 留到 Task 11 与能力 UI 一起做，**有意**不在这里暴露 UI。
/// 存储格式：`topics` 以 `,` 分隔、`thread_segments` 以 `||` 分隔，与
/// `scheduler::execute_publish` 的解析用同一个分隔符。
/// `scheduled_at` 单位是**秒**（与数据库列一致），不做毫秒换算。
#[tauri::command]
pub async fn create_publish_task(
    app: AppHandle,
    db: State<'_, Db>,
    account_id: String,
    title: Option<String>,
    content: Option<String>,
    media_paths: Option<String>,
    cron_expr: Option<String>,
    scheduled_at: Option<i64>,
    cover_path: Option<String>,
    topics: Option<String>,
    thread_segments: Option<String>,
) -> Result<PublishTask, String> {
    let task = db
        .create_task(
            &account_id,
            title.as_deref(),
            content.as_deref(),
            media_paths.as_deref(),
            cron_expr.as_deref(),
            scheduled_at,
            cover_path.as_deref(),
            topics.as_deref(),
            thread_segments.as_deref(),
        )
        .map_err(|e| e.to_string())?;

    // 带 cron 表达式：立即注册定时作业（不再依赖调度器是否已就绪）
    if let Some(cron) = task.cron_expr.clone().filter(|c| !c.trim().is_empty()) {
        let scheduler = app
            .try_state::<Arc<crate::scheduler::PublishScheduler>>()
            .map(|state| state.inner().clone())
            .ok_or_else(|| "定时调度器未就绪，任务未注册，请重启应用后重试".to_string())?;
        if let Err(error) = scheduler
            .add_job(app.clone(), &cron, task.id.clone(), account_id.clone())
            .await
        {
            // 标记失败而不是留在 pending，避免用户以为任务会执行。
            let _ = db.update_task_status(
                &task.id,
                "failed",
                Some(&format!("定时表达式无效，未注册：{}", error)),
            );
            return Err(format!("定时表达式无效，任务未注册：{}", error));
        }
    } else if task.scheduled_at.is_none() {
        // 没有 cron 也没有定时时间：界面提示「立即发布」，就必须真的发布。
        // 之前只入库不执行，任务会永远停在 pending。
        let app_handle = app.clone();
        let task_id = task.id.clone();
        let account_id_clone = account_id.clone();
        tauri::async_runtime::spawn(async move {
            crate::scheduler::execute_publish(&app_handle, &task_id, &account_id_clone).await;
        });
    }

    Ok(task)
}

/// 批量发布：将同一内容发布到多个账号（矩阵营销核心工作流）
///
/// 逐个账号处理，发布结果按账号返回：令牌型平台（`token_hint()` 非空）走
/// `publish_with_token`，全程不打开浏览器会话；浏览器族平台未打开会话时
/// 自动打开。单个账号失败只记入该账号的结果 JSON，不中断整批。
#[tauri::command]
pub async fn batch_publish(
    app: AppHandle,
    db: State<'_, Db>,
    sessions: State<'_, SessionRegistry>,
    account_ids: Vec<String>,
    title: Option<String>,
    content: String,
    media_paths: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut results = Vec::new();

    // 解析媒体路径（逗号分隔）
    let media: Vec<String> = media_paths
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    for account_id in &account_ids {
        // 1. 查找账号
        let account = match find_account(&db, account_id) {
            Ok(a) => a,
            Err(e) => {
                results.push(serde_json::json!({
                    "account_id": account_id,
                    "account_name": null,
                    "success": false,
                    "error": e,
                }));
                continue;
            }
        };

        // 2. 先构造发布内容：只依赖入参、不依赖浏览器会话，因此上移到
        //    「解析适配器 / 打开会话」之前——令牌型平台必须能完全不打开
        //    会话地发布。（与 `scheduler::execute_publish` 同一模式。）
        let publish_content = crate::platforms::PublishContent {
            title: title.clone(),
            text: content.clone(),
            media_paths: media.clone(),
            cover_path: None,
            topics: Vec::new(),
            thread_segments: Vec::new(),
            schedule_at: None,
        };

        // 3. 先解析适配器，才能判断该平台是否需要浏览器会话。
        let adapter = match crate::platforms::find_adapter(&account.platform, Some(&db)) {
            Some(a) => a,
            None => {
                results.push(serde_json::json!({
                    "account_id": account_id,
                    "account_name": account.name,
                    "success": false,
                    "error": format!("不支持的平台: {}", account.platform),
                }));
                continue;
            }
        };

        // 结果 JSON 的形状只在这里构造：令牌分支与浏览器分支都调用同一个
        // 闭包，保证字段名（account_id / account_name / success / error）与
        // 类型（success 为 bool，error 为 null 或 string）完全一致。
        let make_result = |ok: bool, error: Option<String>| {
            serde_json::json!({
                "account_id": account_id,
                "account_name": account.name,
                "success": ok,
                "error": error,
            })
        };

        // 发布前能力校验：在打开任何浏览器会话之前完成（令牌分支与浏览器分支
        // 共用）。这样 publish: false 的平台（抖音 / 公众号）直接判失败，不会先
        // 启动一个注定失败的浏览器实例。
        if let Err(e) = adapter.capabilities().validate(&publish_content) {
            let reason = format!("内容超出平台能力: {}", e);
            let _ = db.log_event(Some(account_id), "error", "batch_publish", Some(&reason));
            results.push(make_result(false, Some(reason)));
            continue;
        }

        // 成功 / 失败收尾：令牌分支与浏览器分支共用，消除两份「log_event +
        // results.push」的漂移风险。闭包把 results 作为参数传入（而不是捕获），
        // 因此两个闭包可以同时存在；结果 JSON 仍由 make_result 统一构造。
        let finish_success = |results: &mut Vec<serde_json::Value>| {
            let _ = db.log_event(Some(account_id), "info", "batch_publish", Some("批量发布成功"));
            results.push(make_result(true, None));
        };
        let finish_failure = |results: &mut Vec<serde_json::Value>, reason: String| {
            let _ = db.log_event(
                Some(account_id),
                "error",
                "batch_publish",
                Some(&format!("批量发布失败: {}", reason)),
            );
            results.push(make_result(false, Some(reason)));
        };

        // 4. 令牌型平台：取令牌 → publish_with_token。
        //    全程不调用 sessions.open / get_page，不依赖浏览器引擎。
        //    该函数逐账号收集结果，因此这里不用 `?`（anyhow::Error 也不能
        //    直接转成 String）中断整批，失败同样写进该账号的结果 JSON。
        if adapter.token_hint().is_some() {
            let token = match db.get_api_token(account_id) {
                Ok(Some(t)) => t,
                Ok(None) => {
                    results.push(make_result(
                        false,
                        Some("该账号尚未配置 API 令牌，请在账号设置中粘贴".to_string()),
                    ));
                    continue;
                }
                Err(e) => {
                    results.push(make_result(
                        false,
                        Some(format!("读取 API 令牌失败: {}", e)),
                    ));
                    continue;
                }
            };

            match adapter.publish_with_token(&token, &publish_content).await {
                Ok(()) => finish_success(&mut results),
                Err(e) => finish_failure(&mut results, e.to_string()),
            }
            continue;
        }

        // 5. 浏览器族：到这里才需要会话。
        //    登录交互期间不允许发布流程接管同一页面。
        if sessions.is_login_active(account_id) {
            results.push(make_result(
                false,
                Some("账号正在登录验证，请完成登录后再发布".to_string()),
            ));
            continue;
        }

        // 打开会话（未打开时自动打开）
        if sessions.get_page(account_id).is_none() {
            if let Err(e) = sessions.open(&app, &db, &account).await {
                results.push(make_result(false, Some(format!("打开会话失败: {}", e))));
                continue;
            }
        }

        // 获取页面
        let page = match sessions.get_page(account_id) {
            Some(p) => p,
            None => {
                results.push(make_result(false, Some("会话页面不存在".to_string())));
                continue;
            }
        };

        match adapter.publish(&page, &publish_content).await {
            Ok(()) => finish_success(&mut results),
            Err(e) => finish_failure(&mut results, e.to_string()),
        }
    }

    Ok(results)
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
///
/// 返回 `id` / `name`（向后兼容既有前端）+ 能力与令牌信息：
/// - `tokenSupported`：`token_hint()` 非空即为令牌型平台；
/// - `tokenHint`：令牌获取位置提示（浏览器族为 null）；
/// - `capabilities`：平台能力矩阵（`Capabilities` 直接序列化，字段 snake_case）。
#[tauri::command]
pub fn list_platforms(db: State<'_, Db>) -> Result<Vec<serde_json::Value>, String> {
    Ok(crate::platforms::all_adapters(Some(&db))
        .iter()
        .map(|a| {
            serde_json::json!({
                "id": a.platform_id(),
                "name": a.display_name(),
                "tokenSupported": a.token_hint().is_some(),
                "tokenHint": a.token_hint(),
                "capabilities": a.capabilities(),
            })
        })
        .collect())
}

// ---------- 短信配置 ----------

/// 获取短信配置（密钥脱敏返回）
#[tauri::command]
pub fn get_sms_config(db: State<'_, Db>) -> Result<serde_json::Value, String> {
    let config = db.get_sms_config().map_err(|e| e.to_string())?;
    match config {
        Some(c) => {
            // 密钥脱敏：只显示前 4 个字符（按字符切片避免多字节 panic）
            let masked = if c.access_key_secret.chars().count() > 4 {
                let prefix: String = c.access_key_secret.chars().take(4).collect();
                format!("{}****", prefix)
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

// ---------- 自定义平台 ----------

/// 列出自定义平台
#[tauri::command]
pub fn list_custom_platforms(
    db: State<'_, Db>,
) -> Result<Vec<serde_json::Value>, String> {
    let platforms = db.list_custom_platforms().map_err(|e| e.to_string())?;
    Ok(platforms
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "creator_url": p.creator_url,
                "publish_url": p.publish_url,
                "title_selector": p.title_selector,
                "content_selector": p.content_selector,
                "submit_selector": p.submit_selector,
                "logged_in_pattern": p.logged_in_pattern,
            })
        })
        .collect())
}

/// 新增自定义平台
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn create_custom_platform(
    db: State<'_, Db>,
    id: String,
    name: String,
    creator_url: String,
    publish_url: String,
    title_selector: Option<String>,
    content_selector: Option<String>,
    submit_selector: Option<String>,
    logged_in_pattern: Option<String>,
) -> Result<(), String> {
    // 简单校验：id 不能与内置平台冲突（抖音 / 小红书 / 公众号 / CSDN / 知乎）
    if crate::platforms::is_builtin_platform_id(&id) {
        return Err(format!("平台 ID「{}」为内置平台保留，请换一个", id));
    }
    db.create_custom_platform(
        &id,
        &name,
        &creator_url,
        &publish_url,
        title_selector.as_deref(),
        content_selector.as_deref(),
        submit_selector.as_deref(),
        logged_in_pattern.as_deref(),
    )
    .map_err(|e| e.to_string())?;
    let _ = db.log_event(
        None,
        "info",
        "custom_platform_created",
        Some(&format!("自定义平台「{}」已创建", name)),
    );
    Ok(())
}

/// 删除自定义平台
#[tauri::command]
pub fn delete_custom_platform(db: State<'_, Db>, id: String) -> Result<(), String> {
    db.delete_custom_platform(&id).map_err(|e| e.to_string())
}

// ---------- 敏感词检测 ----------

/// 检测文本中的敏感词（发布前合规检查）
#[tauri::command]
pub fn check_sensitive_words(text: String) -> Result<serde_json::Value, String> {
    let hits = crate::sensitive_words::check_sensitive_words(&text);
    Ok(serde_json::json!({
        "hits": hits,
        "count": hits.len(),
        "word_count": crate::sensitive_words::word_count(),
    }))
}

/// 从数据库查找账号
fn find_account(db: &Db, account_id: &str) -> std::result::Result<Account, String> {
    db.list_accounts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| "账号不存在".to_string())
}
