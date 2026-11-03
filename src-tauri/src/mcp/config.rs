// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! MCP 服务配置
//!
//! 配置以 JSON 形式存在 `settings` 表的 `mcp_config` 键下，
//! 与短信配置同样的存储方式，便于随数据库一起迁移。

use crate::db::Db;
use serde::{Deserialize, Serialize};

const SETTING_KEY: &str = "mcp_config";

/// 默认监听端口
pub const DEFAULT_PORT: u16 = 8931;

fn default_port() -> u16 {
    DEFAULT_PORT
}

/// MCP 服务配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// 总开关；保存后立即启停服务
    #[serde(default)]
    pub enabled: bool,
    /// 监听端口（只绑定 127.0.0.1，不对外暴露）
    #[serde(default = "default_port")]
    pub port: u16,
    /// 访问令牌；非空时强制校验 `Authorization: Bearer <token>`
    #[serde(default)]
    pub token: String,
    /// 是否允许写操作（发布 / 建号 / 删号 / 引擎控制）
    #[serde(default)]
    pub allow_write: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            token: String::new(),
            allow_write: false,
        }
    }
}

impl McpConfig {
    fn validated(mut self) -> Self {
        if self.port == 0 { self.port = DEFAULT_PORT; }
        if self.token.contains('\r') || self.token.contains('\n') {
            self.enabled = false;
        }
        self
    }

    /// 从数据库读取配置；缺失或损坏时回退默认值
    pub fn load(db: &Db) -> Self {
        match db.get_setting(SETTING_KEY) {
            Ok(Some(raw)) => serde_json::from_str::<Self>(&raw)
                .map(Self::validated)
                .unwrap_or_else(|e| {
                    tracing::warn!("MCP 配置解析失败，回退默认值: {}", e);
                    Self::default()
                }),
            Ok(None) => Self::default(),
            Err(e) => {
                tracing::warn!("读取 MCP 配置失败: {}", e);
                Self::default()
            }
        }
    }

    /// 写回数据库
    pub fn save(&self, db: &Db) -> Result<(), String> {
        let raw = serde_json::to_string(self).map_err(|e| e.to_string())?;
        db.set_setting(SETTING_KEY, &raw).map_err(|e| e.to_string())
    }

    /// 对外展示的连接地址
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.port)
    }
}
