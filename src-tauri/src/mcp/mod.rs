// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 内置 MCP 服务与设置命令。
pub mod config;
mod protocol;
mod server;
mod tools;
#[cfg(test)]
mod tests;

use crate::db::Db;
use config::McpConfig;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct McpState {
    operation: Mutex<()>,
    server: Mutex<Option<server::ServerHandle>>,
}

// 调用者持有 operation；不同端口先启动新服务，同端口停服后失败则恢复原配置。
async fn replace(app: &AppHandle, config: McpConfig, previous: McpConfig) -> Result<(), String> {
    let state = app.state::<McpState>();
    let old = state.server.lock().await.take();
    let same_port = old.as_ref().is_some_and(|handle| handle.port == config.port);
    if config.enabled && !same_port {
        match server::start(app.clone(), config).await {
            Ok(next) => {
                *state.server.lock().await = Some(next);
                if let Some(old) = old { old.stop().await; }
                return Ok(());
            }
            Err(error) => {
                *state.server.lock().await = old;
                return Err(error);
            }
        }
    }
    if let Some(old) = old { old.stop().await; }
    if config.enabled {
        match server::start(app.clone(), config).await {
            Ok(next) => *state.server.lock().await = Some(next),
            Err(error) => {
                if previous.enabled {
                    *state.server.lock().await = server::start(app.clone(), previous).await.ok();
                }
                return Err(error);
            }
        }
    }
    Ok(())
}

pub async fn apply(app: &AppHandle, config: McpConfig) -> Result<(), String> {
    let state = app.state::<McpState>();
    let _operation = state.operation.lock().await;
    replace(app, config, McpConfig::default()).await
}

#[tauri::command]
pub async fn get_mcp_config(app: AppHandle, db: State<'_, Db>) -> Result<Value, String> {
    let state = app.state::<McpState>();
    let _operation = state.operation.lock().await;
    let config = McpConfig::load(&db);
    let current = state.server.lock().await;
    let running = current.as_ref().is_some_and(|handle| handle.is_running());
    Ok(json!({"config":config,"running":running,"url":config.url(),
        "running_port":current.as_ref().map(|handle| handle.port)}))
}

#[tauri::command]
pub async fn save_mcp_config(app: AppHandle, db: State<'_, Db>, config: McpConfig) -> Result<(), String> {
    if config.port == 0 { return Err("端口必须在 1 到 65535 之间".into()); }
    if config.token.contains('\r') || config.token.contains('\n') {
        return Err("令牌不能包含换行".into());
    }
    let state = app.state::<McpState>();
    let _operation = state.operation.lock().await;
    let previous = McpConfig::load(&db);
    config.save(&db)?;
    if let Err(error) = replace(&app, config.clone(), previous.clone()).await {
        let _ = previous.save(&db);
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub async fn test_mcp(db: State<'_, Db>) -> Result<String, String> {
    let config = McpConfig::load(&db);
    if !config.enabled { return Err("请先启用并保存 MCP 服务".into()); }
    let response = reqwest::Client::new().post(config.url())
        .bearer_auth(&config.token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
        .timeout(std::time::Duration::from_secs(5)).send().await.map_err(|e|e.to_string())?;
    let response = response.error_for_status().map_err(|e|e.to_string())?;
    let body: Value = response.json().await.map_err(|e|e.to_string())?;
    let count = body.pointer("/result/tools").and_then(Value::as_array).ok_or("响应中缺少工具列表")?.len();
    Ok(format!("连接成功，共 {} 个工具", count))
}
