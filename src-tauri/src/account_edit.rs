// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 账号编辑命令；代理变更时关闭旧会话，下一次打开使用新代理。

use crate::db::{normalize_account_field, Account, AccountStatus, Db, UpdateAccount};
use crate::session::SessionRegistry;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn update_account(
    app: AppHandle,
    db: State<'_, Db>,
    sessions: State<'_, SessionRegistry>,
    id: String,
    input: UpdateAccount,
) -> Result<Account, String> {
    if input.name.trim().is_empty() {
        return Err("备注名称不能为空".into());
    }
    let existing = db
        .list_accounts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| "账号不存在".to_string())?;
    let proxy_changed = existing.proxy_url
        != normalize_account_field(input.proxy_url.as_deref());
    if proxy_changed {
        // close 会先移除注册表条目，即使释放 context 失败也需同步离线状态。
        let close_result = sessions.close(&app, &id).await;
        db.update_account_status(&id, AccountStatus::Offline)
            .map_err(|e| e.to_string())?;
        close_result.map_err(|_| "关闭旧代理会话失败，请重试保存".to_string())?;
    }
    db.update_account(&id, &input).map_err(|e| e.to_string())
}
