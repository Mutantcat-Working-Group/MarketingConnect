// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! Platform-native login interaction. Verification codes are never persisted or logged.
use crate::db::{AccountStatus, Db};
use crate::session::SessionRegistry;
use chromiumoxide::cdp::browser_protocol::input::{DispatchKeyEventParams, DispatchMouseEventParams, InsertTextParams};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::State;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoginAction {
    Click { x: f64, y: f64 },
    Scroll { x: f64, y: f64, delta: f64 },
    Text { text: String },
    Key { key: String },
}

fn position(x: f64, y: f64) -> Result<(), String> {
    if !x.is_finite() || !y.is_finite() || !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
        return Err("点击位置超出画面".into());
    }
    Ok(())
}

#[tauri::command]
pub fn set_login_active(sessions: State<'_, SessionRegistry>, account_id: String, active: bool) {
    sessions.set_login_active(&account_id, active);
}

#[tauri::command]
pub async fn login_action(sessions: State<'_, SessionRegistry>, account_id: String, action: LoginAction) -> Result<(), String> {
    let page = sessions.get_page(&account_id).ok_or("请先打开登录会话")?;
    sessions.set_login_active(&account_id, true);
    match action {
        LoginAction::Text { text } => {
            if text.is_empty() || text.len() > 4096 { return Err("输入文本长度必须为 1 到 4096 字节".into()); }
            let params: InsertTextParams = serde_json::from_value(json!({"text":text})).map_err(|_| "输入参数无效")?;
            page.execute(params).await.map_err(|_| "文本输入失败，请先点击页面中的输入框")?;
        }
        LoginAction::Key { key } => {
            let code = match key.as_str() { "Enter" => 13, "Tab" => 9, "Backspace" => 8, "Escape" => 27, "ArrowLeft" => 37, "ArrowRight" => 39, "Delete" => 46, _ => return Err("不支持的按键".into()) };
            for event in [if key == "Enter" { "keyDown" } else { "rawKeyDown" }, "keyUp"] {
                let mut raw = json!({"type":event,"key":key,"code":key,"windowsVirtualKeyCode":code,"nativeVirtualKeyCode":code});
                if key == "Enter" && event == "keyDown" { raw["text"] = json!("\r"); }
                let params: DispatchKeyEventParams = serde_json::from_value(raw).map_err(|_| "按键参数无效")?;
                page.execute(params).await.map_err(|_| "发送按键失败")?;
            }
        }
        action => {
            let (x, y, delta) = match action {
                LoginAction::Click { x, y } => (x, y, None),
                LoginAction::Scroll { x, y, delta } => {
                    if !delta.is_finite() || delta.abs() > 2000.0 { return Err("滚动距离无效".into()); }
                    (x, y, Some(delta))
                }
                _ => unreachable!(),
            };
            position(x, y)?;
            let size: Vec<f64> = page.evaluate("[window.innerWidth,window.innerHeight]").await.map_err(|_| "获取视口失败")?.into_value().map_err(|_| "无法读取视口尺寸")?;
            if size.len() != 2 || size.iter().any(|value| !value.is_finite() || *value <= 0.0) { return Err("视口尺寸无效".into()); }
            let px = (x * size[0]).min(size[0] - 1.0);
            let py = (y * size[1]).min(size[1] - 1.0);
            let events = if delta.is_some() { vec!["mouseWheel"] } else { vec!["mousePressed", "mouseReleased"] };
            for event in events {
                let raw = if let Some(delta) = delta {
                    json!({"type":event,"x":px,"y":py,"deltaX":0,"deltaY":delta})
                } else { json!({"type":event,"x":px,"y":py,"button":"left","clickCount":1}) };
                let params: DispatchMouseEventParams = serde_json::from_value(raw).map_err(|_| "点击参数无效")?;
                page.execute(params).await.map_err(|_| "页面交互失败")?;
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn finish_account_login(db: State<'_, Db>, sessions: State<'_, SessionRegistry>, account_id: String) -> Result<Value, String> {
    let account = db.list_accounts().map_err(|_| "读取账号失败")?.into_iter().find(|account| account.id == account_id).ok_or("账号不存在")?;
    let page = sessions.get_page(&account_id).ok_or("请先打开登录会话")?;
    let adapter = crate::platforms::find_adapter(&account.platform, Some(&db)).ok_or("不支持的平台")?;
    let authenticated = adapter.check_session(&page).await.map_err(|_| "登录状态检测失败")?;
    if !authenticated {
        return Ok(json!({"authenticated":false,"saved":false,"message":"尚未确认登录成功。请完成平台验证码/扫码，并等待进入账号后台；需检测到可见的退出登录入口才会保存。"}));
    }
    let count = sessions.export_cookies(&db, &account_id).await.map_err(|_| "已检测到登录，但凭证保存失败，请重试")?;
    db.update_account_status(&account_id, AccountStatus::Online).map_err(|_| "凭证已保存，但登录状态写入失败")?;
    sessions.set_login_active(&account_id, false);
    Ok(json!({"authenticated":true,"saved":true,"cookie_count":count,"message":"已确认登录，凭证已加密保存"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalized_positions_are_bounded() {
        assert!(position(0.5, 1.0).is_ok());
        assert!(position(-0.1, 0.0).is_err());
        assert!(position(f64::NAN, 0.0).is_err());
    }
    #[test]
    fn action_supports_codes_without_storing_them() {
        let action: LoginAction = serde_json::from_value(json!({"kind":"text","text":"123456"})).unwrap();
        assert!(matches!(action, LoginAction::Text { .. }));
        assert!(serde_json::from_value::<LoginAction>(json!({"kind":"unknown"})).is_err());
    }
}
