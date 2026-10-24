// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! Obscura sidecar 进程管理（多实例）
//!
//! Obscura 的 `--proxy` 是进程级参数，因此每账号独立代理的方案是
//! **按代理分组运行多个 obscura serve 实例**：
//! - 每个唯一代理地址（含"直连"）对应一个实例
//! - 实例自动分配端口（9222 起）
//! - 账号会话绑定到与其代理匹配的实例
//!
//! 注意：不使用 `--storage-dir`——同一进程内的多个 BrowserContext
//! 会共享磁盘存储，破坏多账号隔离。会话持久化由加密的
//! CDP cookie 导出/恢复机制负责（见 session.rs）。

mod client;

pub use client::{CdpBrowser, CdpState};

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;
use tracing::info;

/// 直连实例的 key
pub const DIRECT_KEY: &str = "direct";

/// 起始端口
const BASE_PORT: u16 = 9222;

/// 单个 Obscura 实例
struct ObscuraInstance {
    port: u16,
    child: CommandChild,
}

/// Obscura 多实例管理器
pub struct ObscuraManager {
    /// proxy_key → 实例（"direct" 表示直连）
    instances: Mutex<HashMap<String, ObscuraInstance>>,
    /// 下一个可用端口
    next_port: AtomicU16,
}

impl ObscuraManager {
    pub fn new() -> Self {
        ObscuraManager {
            instances: Mutex::new(HashMap::new()),
            next_port: AtomicU16::new(BASE_PORT),
        }
    }

    /// 获取或启动指定代理的实例，返回端口
    ///
    /// proxy 为 None 时使用直连实例。
    pub fn get_or_start(&self, app: &AppHandle, proxy: Option<&str>) -> Result<u16> {
        let key = proxy.unwrap_or(DIRECT_KEY).to_string();

        // 已存在且进程存活：直接返回
        {
            let instances = self.instances.lock().unwrap();
            if let Some(inst) = instances.get(&key) {
                if is_port_open(inst.port) {
                    return Ok(inst.port);
                }
            }
        }

        // 分配端口（避开已占用端口）
        let port = self.allocate_port();

        info!(
            "启动 obscura 实例（代理: {}，端口: {}）",
            proxy.unwrap_or("直连"),
            port
        );

        // 构建 sidecar 命令
        let mut cmd = app
            .shell()
            .sidecar("obscura")?
            .args(["serve", "--port", &port.to_string(), "--stealth"]);
        if let Some(p) = proxy {
            cmd = cmd.args(["--proxy", p]);
        }

        let (_, child) = cmd.spawn().context("启动 obscura sidecar 失败")?;

        // 等待端口就绪（最多 10s）
        let mut ready = false;
        for _ in 0..50 {
            if is_port_open(port) {
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        if !ready {
            let _ = child.kill();
            anyhow::bail!("obscura 实例启动超时（端口 {} 未就绪）", port);
        }

        info!("obscura 实例就绪（端口 {}）", port);
        self.instances
            .lock()
            .unwrap()
            .insert(key, ObscuraInstance { port, child });

        Ok(port)
    }

    /// 分配一个未被占用的端口
    fn allocate_port(&self) -> u16 {
        loop {
            let port = self.next_port.fetch_add(1, Ordering::SeqCst);
            if !is_port_open(port) {
                return port;
            }
        }
    }

    /// 停止指定代理的实例
    pub fn stop(&self, proxy: Option<&str>) {
        let key = proxy.unwrap_or(DIRECT_KEY);
        if let Some(inst) = self.instances.lock().unwrap().remove(key) {
            info!("停止 obscura 实例（端口 {}）", inst.port);
            let _ = inst.child.kill();
        }
    }

    /// 停止所有实例
    pub fn stop_all(&self) {
        let instances: Vec<_> = self
            .instances
            .lock()
            .unwrap()
            .drain()
            .map(|(_, inst)| inst)
            .collect();
        for inst in instances {
            info!("停止 obscura 实例（端口 {}）", inst.port);
            let _ = inst.child.kill();
        }
    }

    /// 引擎状态（供前端展示）
    pub fn status(&self) -> serde_json::Value {
        let instances = self.instances.lock().unwrap();
        let list: Vec<_> = instances
            .iter()
            .map(|(key, inst)| {
                serde_json::json!({
                    "proxy": key,
                    "port": inst.port,
                    "running": is_port_open(inst.port),
                })
            })
            .collect();
        serde_json::json!({
            "instances": list,
            "count": list.len(),
        })
    }
}

impl Drop for ObscuraManager {
    fn drop(&mut self) {
        self.stop_all();
    }
}

/// 检查端口是否可连接
fn is_port_open(port: u16) -> bool {
    TcpStream::connect_timeout(
        &format!("127.0.0.1:{}", port)
            .parse()
            .expect("地址格式合法"),
        Duration::from_millis(500),
    )
    .is_ok()
}

/// 构造 CDP WebSocket 地址
pub fn ws_url(port: u16) -> String {
    format!("ws://127.0.0.1:{}/devtools/browser", port)
}
