// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 会话健康监控（后台周期检测掉号）
//!
//! 周期性检测所有已打开会话的账号，发现掉号时：
//! - 更新账号状态
//! - 发送系统通知
//! - 发送短信（如已配置）
//!
//! 让「掉号提示」从手动检测变为主动告警。

use crate::db::{AccountStatus, Db};
use crate::session::SessionRegistry;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tracing::{info, warn};

/// 默认检测间隔（秒）
const DEFAULT_INTERVAL_SECS: u64 = 300;

/// 启动后台会话监控
pub fn spawn_monitor(app: AppHandle, interval_secs: Option<u64>) {
    let interval = interval_secs.unwrap_or(DEFAULT_INTERVAL_SECS);
    info!("会话健康监控已启动（间隔 {} 秒）", interval);

    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval));
        // 第一个 tick 立即返回，跳过（避免启动即检测）
        ticker.tick().await;

        loop {
            ticker.tick().await;
            check_all_sessions(&app).await;
        }
    });
}

/// 检测所有已打开会话的账号
async fn check_all_sessions(app: &AppHandle) {
    let sessions = app.state::<SessionRegistry>();
    let db = app.state::<Db>();

    // 只检测有打开会话的账号（无会话的账号无需检测）
    let open_ids = sessions.open_account_ids();
    if open_ids.is_empty() {
        return;
    }

    let accounts = match db.list_accounts() {
        Ok(a) => a,
        Err(e) => {
            warn!("监控：读取账号失败: {}", e);
            return;
        }
    };

    for account in accounts.iter().filter(|a| open_ids.contains(&a.id)) {
        // 跳过已标记为掉号的账号（避免重复告警）
        if account.status != AccountStatus::Online || sessions.is_login_active(&account.id) {
            continue;
        }

        let Some(adapter) = crate::platforms::find_adapter(&account.platform, Some(&db)) else {
            continue;
        };

        // 令牌型平台没有浏览器会话：会话有效性走 verify_token，绝不能拿
        // check_session 的页面探测结果当掉号依据（dev.to 之前恒返回 false，
        // 导致每个已登录的 dev.to 账号都会被误报掉号并发通知/短信）。
        let valid = if adapter.token_hint().is_some() {
            match crate::platforms::check_token_session(&db, adapter.as_ref(), &account.id).await {
                Ok(crate::platforms::TokenSession::Valid) => true,
                Ok(crate::platforms::TokenSession::Invalid(reason)) => {
                    warn!("监控：账号 {} 令牌校验失败: {}", account.name, reason);
                    false
                }
                // 未配置令牌是「没配置」，不是「掉号」：不标离线、不告警。
                Ok(crate::platforms::TokenSession::NotConfigured) => {
                    warn!(
                        "监控：账号 {} 为令牌型平台但未配置 API 令牌，跳过掉号判定",
                        account.name
                    );
                    continue;
                }
                Err(e) => {
                    warn!("监控：账号 {} 读取 API 令牌失败: {}", account.name, e);
                    continue;
                }
            }
        } else {
            // 浏览器族：维持原有页面探测逻辑。
            let Some(page) = sessions.get_page(&account.id) else {
                continue;
            };
            match adapter.check_session(&page).await {
                Ok(v) => v,
                Err(e) => {
                    warn!("监控：账号 {} 检测出错: {}", account.name, e);
                    continue;
                }
            }
        };

        if !valid {
            warn!("监控：账号 {} 掉号", account.name);
            let _ = db.update_account_status(&account.id, AccountStatus::Offline);
            let _ = db.log_event(
                Some(&account.id),
                "warn",
                "session_dropped",
                Some("后台监控检测到掉号"),
            );
            notify_drop(app, &account.name, &account.platform, &db).await;
        }
    }
}

/// 掉号告警：系统通知 + 短信
async fn notify_drop(app: &AppHandle, account_name: &str, platform: &str, db: &Db) {
    use tauri_plugin_notification::NotificationExt;

    // 系统通知
    let _ = app
        .notification()
        .builder()
        .title("营销通 - 账号掉号提醒")
        .body(&format!(
            "账号「{}」（{}）登录已失效，请及时重新登录",
            account_name, platform
        ))
        .show();

    // 短信提醒（配置了才发送）
    if let Ok(Some(sms_config)) = db.get_sms_config() {
        if sms_config.is_configured() {
            let name = account_name.to_string();
            let platform = platform.to_string();
            match crate::sms::send_offline_alert(&sms_config, &name, &platform).await {
                Ok(()) => info!("掉号短信已发送（账号 {}）", name),
                Err(e) => warn!("掉号短信发送失败: {}", e),
            }
        }
    }
}
