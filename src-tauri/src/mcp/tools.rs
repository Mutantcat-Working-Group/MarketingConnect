// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! MCP 工具注册与调用分发。

use crate::accounts;
use crate::db::{Db, NewAccount};
use crate::mcp::config::McpConfig;
use crate::obscura::ObscuraManager;
use crate::session::SessionRegistry;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

fn text_property(description: &str) -> Value {
    json!({"type": "string", "description": description})
}

/// 返回 MCP tools/list 的工具定义。
pub fn definitions(config: &McpConfig) -> Vec<Value> {
    let write_note = if config.allow_write {
        ""
    } else {
        "（当前设置未允许写入，调用会被拒绝）"
    };
    vec![
        json!({"name":"list_accounts","description":"列出营销通中保存的全部账号及状态。","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"list_platforms","description":"列出当前支持的平台。","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"engine_status","description":"查询 Obscura 浏览器引擎及实例状态。","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"check_account_session","description":format!("检测指定账号是否仍保持登录状态；会打开会话、更新账号状态，且可能触发掉号通知{}。", write_note),"inputSchema":{"type":"object","properties":{"account_id":text_property("账号 ID")},"required":["account_id"]}}),
        json!({"name":"recent_logs","description":"查询最近的营销通事件日志。","inputSchema":{"type":"object","properties":{"limit":{"type":"integer","minimum":1,"maximum":200,"description":"返回条数，默认 50"}}}}),
        json!({"name":"check_sensitive_words","description":"检测文案中的敏感词。","inputSchema":{"type":"object","properties":{"text":text_property("需要检测的文本")},"required":["text"]}}),
        json!({"name":"open_account_session","description":format!("打开指定账号的浏览器会话{}。", write_note),"inputSchema":{"type":"object","properties":{"account_id":text_property("账号 ID")},"required":["account_id"]}}),
        json!({"name":"batch_publish","description":format!("将同一内容发布到多个账号{}。", write_note),"inputSchema":{"type":"object","properties":{"account_ids":{"type":"array","items":{"type":"string"},"description":"账号 ID 列表"},"title":{"type":"string","description":"可选标题"},"content":text_property("正文内容"),"media_paths":{"type":"string","description":"可选，逗号分隔的媒体路径"}},"required":["account_ids","content"]}}),
        json!({"name":"create_account","description":format!("新增一个营销账号（登录账号与密码可选，可稍后在登录面板用验证码/扫码登录）{}。", write_note),"inputSchema":{"type":"object","properties":{"platform":text_property("平台 ID"),"name":text_property("备注名称"),"username":{"type":"string","description":"可选，登录账号/手机号"},"password":{"type":"string","description":"可选登录密码"},"proxy_url":{"type":"string","description":"可选代理地址"},"group_name":{"type":"string","description":"可选分组"}},"required":["platform","name"]}}),
        json!({"name":"delete_account","description":format!("删除一个账号及其会话{}。", write_note),"inputSchema":{"type":"object","properties":{"account_id":text_property("账号 ID")},"required":["account_id"]}}),
        json!({"name":"start_engine","description":format!("启动浏览器引擎{}。", write_note),"inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"stop_engine","description":format!("停止全部浏览器引擎实例{}。", write_note),"inputSchema":{"type":"object","properties":{}}}),
    ]
}

fn required_string(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("缺少必填参数: {}", key))
}

fn optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn string_list(args: &Value, key: &str) -> Result<Vec<String>, String> {
    let values = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("参数 {} 必须是字符串数组", key))?;
    if values.is_empty() {
        return Err(format!("参数 {} 不能为空", key));
    }
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|item| !item.trim().is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| format!("参数 {} 中包含非字符串值", key))
        })
        .collect()
}

fn value<T: serde::Serialize>(input: T) -> Result<Value, String> {
    serde_json::to_value(input).map_err(|error| error.to_string())
}

/// 调用一个 MCP tool，返回可直接放进 text content 的 JSON 值。
pub async fn call(
    app: &AppHandle,
    name: &str,
    args: &Value,
    config: &McpConfig,
) -> Result<Value, String> {
    let write_tool = matches!(
        name,
        "open_account_session"
            | "check_account_session"
            | "batch_publish"
            | "create_account"
            | "delete_account"
            | "start_engine"
            | "stop_engine"
    );
    if write_tool && !config.allow_write {
        return Err("MCP 写入权限未开启，请在营销通设置页打开「允许写入」".to_string());
    }

    match name {
        "list_accounts" => {
            let db = app.state::<Db>();
            value(accounts::list_accounts(db)?)
        }
        "list_platforms" => {
            let db = app.state::<Db>();
            value(accounts::list_platforms(db)?)
        }
        "engine_status" => {
            let obscura = app.state::<ObscuraManager>();
            accounts::engine_status(obscura)
        }
        "check_account_session" => {
            let account_id = required_string(args, "account_id")?;
            let db = app.state::<Db>();
            let sessions = app.state::<SessionRegistry>();
            let valid = accounts::check_account_session(app.clone(), db, sessions, account_id).await?;
            Ok(json!({"valid": valid}))
        }
        "recent_logs" => {
            let limit = args.get("limit").and_then(Value::as_i64);
            if matches!(limit, Some(value) if !(1..=200).contains(&value)) {
                return Err("limit 必须在 1 到 200 之间".to_string());
            }
            let db = app.state::<Db>();
            value(accounts::recent_logs(db, limit)?)
        }
        "check_sensitive_words" => {
            let text = required_string(args, "text")?;
            accounts::check_sensitive_words(text)
        }
        "open_account_session" => {
            let account_id = required_string(args, "account_id")?;
            let db = app.state::<Db>();
            let sessions = app.state::<SessionRegistry>();
            accounts::open_account_session(app.clone(), db, sessions, account_id).await?;
            Ok(json!({"success": true}))
        }
        "batch_publish" => {
            let account_ids = string_list(args, "account_ids")?;
            let content = required_string(args, "content")?;
            let title = optional_string(args, "title");
            let media_paths = optional_string(args, "media_paths");
            let db = app.state::<Db>();
            let sessions = app.state::<SessionRegistry>();
            value(accounts::batch_publish(
                app.clone(),
                db,
                sessions,
                account_ids,
                title,
                content,
                media_paths,
            )
            .await?)
        }
        "create_account" => {
            let input = NewAccount {
                platform: required_string(args, "platform")?,
                name: required_string(args, "name")?,
                username: optional_string(args, "username").unwrap_or_default(),
                password: optional_string(args, "password"),
                proxy_url: optional_string(args, "proxy_url"),
                group_name: optional_string(args, "group_name"),
            };
            let db = app.state::<Db>();
            value(accounts::create_account(db, input)?)
        }
        "delete_account" => {
            let account_id = required_string(args, "account_id")?;
            let db = app.state::<Db>();
            let sessions = app.state::<SessionRegistry>();
            accounts::delete_account(app.clone(), db, sessions, account_id).await?;
            Ok(json!({"success": true}))
        }
        "start_engine" => {
            let obscura = app.state::<ObscuraManager>();
            let port = accounts::start_engine(app.clone(), obscura)?;
            Ok(json!({"message": port}))
        }
        "stop_engine" => {
            let obscura = app.state::<ObscuraManager>();
            let sessions = app.state::<crate::session::SessionRegistry>();
            accounts::stop_engine(app.clone(), obscura, sessions).await?;
            Ok(json!({"success": true}))
        }
        _ => Err(format!("未知工具: {}", name)),
    }
}
