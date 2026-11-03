// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! JSON-RPC 2.0 信封与 MCP 协议数据结构
//!
//! 只实现 tools-only MCP server 需要的子集：
//! initialize / notifications/initialized / tools/list / tools/call / ping。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 服务端名称（initialize 的 serverInfo.name）
pub const SERVER_NAME: &str = "marketingconnect";

/// 服务端版本（compile-time 取自 Cargo.toml）
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 首选协议版本
pub const PREFERRED_PROTOCOL_VERSION: &str = "2025-06-18";

/// 兼容的协议版本（客户端请求的版本在其中则原样回显）
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-06-18", "2025-03-26", "2024-11-05"];

/// 握手时给模型的说明
pub const INSTRUCTIONS: &str = "\
营销通（MarketingConnect）多平台矩阵营销号管理服务的 MCP 接口。\
可查询账号、平台、引擎状态与事件日志，检测账号会话是否掉号；\
在设置中开启「允许写入」后，还可发起批量发布、新增/删除账号、开关浏览器引擎。\
调用 batch_publish 前建议先用 list_accounts 确认 account_id，并可用 check_sensitive_words 预检文案。";

/// JSON-RPC 请求或通知
#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

impl RpcRequest {
    /// 无 id 即为通知，按规范不应返回响应体
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// JSON-RPC 错误对象
#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub fn parse(message: impl Into<String>) -> Self {
        Self {
            code: -32700,
            message: message.into(),
        }
    }

    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("未知方法: {}", method),
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
        }
    }
}

/// JSON-RPC 响应
#[derive(Debug, Serialize)]
pub struct RpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Value, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// 协商协议版本：客户端给的版本受支持就沿用它，否则回落到首选版本
pub fn negotiate_protocol_version(requested: Option<&str>) -> &'static str {
    match requested {
        Some(v) if SUPPORTED_PROTOCOL_VERSIONS.contains(&v) => {
            // 回显客户端版本，避免部分客户端因版本不匹配直接断开
            SUPPORTED_PROTOCOL_VERSIONS
                .iter()
                .find(|s| **s == v)
                .copied()
                .unwrap_or(PREFERRED_PROTOCOL_VERSION)
        }
        _ => PREFERRED_PROTOCOL_VERSION,
    }
}
