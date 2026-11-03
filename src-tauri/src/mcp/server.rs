// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! MCP 的本地 HTTP 传输层。

use super::config::McpConfig;
use super::protocol::{self, RpcError, RpcRequest, RpcResponse, INSTRUCTIONS, SERVER_NAME, SERVER_VERSION};
use super::tools;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tauri::AppHandle;

pub struct ServerContext {
    pub app: AppHandle,
    pub config: McpConfig,
}

pub struct ServerHandle {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
    pub port: u16,
}

impl ServerHandle {
    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }

    pub async fn stop(mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
        let mut task = self.task;
        if tokio::time::timeout(std::time::Duration::from_secs(3), &mut task).await.is_err() {
            task.abort();
            let _ = task.await;
        }
    }
}

pub async fn start(app: AppHandle, config: McpConfig) -> Result<ServerHandle, String> {
    let port = config.port;
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|error| format!("MCP 无法监听 127.0.0.1:{}：{}", port, error))?;
    let context = Arc::new(ServerContext { app, config });
    let router = Router::new()
        .route("/mcp", post(handle_post).get(handle_get))
        .with_state(context);
    let (shutdown, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = receiver.await;
            })
            .await;
        if let Err(error) = result {
            tracing::error!("MCP HTTP 服务退出: {}", error);
        }
    });
    Ok(ServerHandle {
        shutdown: Some(shutdown),
        task,
        port,
    })
}

async fn handle_get() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, "POST")],
        "MCP 服务只接受 POST JSON-RPC 请求",
    )
        .into_response()
}

async fn handle_post(
    State(context): State<Arc<ServerContext>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // CORS 本身不能阻止请求执行。拒绝浏览器 Origin，防止网页向本地服务发起操作。
    if headers.contains_key(header::ORIGIN) {
        return (StatusCode::FORBIDDEN, "不接受浏览器跨源请求").into_response();
    }
    let expected_host = format!("127.0.0.1:{}", context.config.port);
    let localhost_host = format!("localhost:{}", context.config.port);
    if !headers.get(header::HOST).and_then(|v| v.to_str().ok())
        .is_some_and(|host| host == expected_host || host == localhost_host) {
        return (StatusCode::FORBIDDEN, "Host 不受信任").into_response();
    }
    if !headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok())
        .is_some_and(|value| value.split(';').next().unwrap_or("").trim() == "application/json") {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "需要 application/json").into_response();
    }
    if !authorized(&headers, &context.config.token) {
        return (StatusCode::UNAUTHORIZED, "MCP token 无效或缺失").into_response();
    }

    let request: RpcRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => return rpc_response(StatusCode::BAD_REQUEST, RpcResponse::err(Value::Null, RpcError::parse(error.to_string()))),
    };

    let id = request.id.clone().unwrap_or(Value::Null);
    if request.jsonrpc != "2.0" {
        return rpc_response(StatusCode::BAD_REQUEST, RpcResponse::err(id, RpcError { code: -32600, message: "jsonrpc 必须为 2.0".into() }));
    }
    // 通知不执行工具，避免没有响应 ID 的请求触发发布等副作用。
    if request.is_notification() {
        return StatusCode::ACCEPTED.into_response();
    }
    let result = match request.method.as_str() {
        "initialize" => initialize_result(request.params.as_ref()),
        "notifications/initialized" | "notifications/cancelled" => {
            if request.is_notification() {
                return StatusCode::ACCEPTED.into_response();
            }
            Err(RpcError::invalid_params("通知方法不能带 id"))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({
            "tools": tools::definitions(&context.config),
        })),
        "tools/call" => call_tool(&context, request.params.as_ref()).await,
        method => Err(RpcError::method_not_found(method)),
    };

    if request.is_notification() {
        return StatusCode::ACCEPTED.into_response();
    }
    let response = match result {
        Ok(value) => RpcResponse::ok(id, value),
        Err(error) => RpcResponse::err(id, error),
    };
    rpc_response(StatusCode::OK, response)
}

fn authorized(headers: &HeaderMap, expected: &str) -> bool {
    if expected.is_empty() {
        return true;
    }
    let expected = format!("Bearer {}", expected);
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value == expected)
        .unwrap_or(false)
}

fn initialize_result(params: Option<&Value>) -> Result<Value, RpcError> {
    let requested = params
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str);
    Ok(json!({
        "protocolVersion": protocol::negotiate_protocol_version(requested),
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION},
        "instructions": INSTRUCTIONS,
    }))
}

async fn call_tool(
    context: &ServerContext,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params = params.ok_or_else(|| RpcError::invalid_params("tools/call 缺少 params"))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("tools/call 缺少 name"))?;
    let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    match tools::call(&context.app, name, &arguments, &context.config).await {
        Ok(value) => Ok(json!({
            "content": [{"type": "text", "text": value.to_string()}],
            "isError": false,
        })),
        Err(error) => Ok(json!({
            "content": [{"type": "text", "text": error}],
            "isError": true,
        })),
    }
}

fn rpc_response(status: StatusCode, response: RpcResponse) -> Response {
    (status, Json(response)).into_response()
}
