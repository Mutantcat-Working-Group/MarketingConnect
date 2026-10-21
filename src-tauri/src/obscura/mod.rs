//! Obscura sidecar 进程管理
//!
//! 通过 tauri-plugin-shell 启动 `obscura serve` CDP 服务进程，
//! 提供健康检查与生命周期管理。

mod client;

pub use client::{CdpBrowser, CdpState};

use anyhow::{Context, Result};
use std::net::TcpStream;
use std::sync::Mutex;
use tauri::AppHandle;
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;
use tracing::{info, warn};

/// Obscura CDP 服务管理器
pub struct ObscuraManager {
    port: u16,
    child: Mutex<Option<CommandChild>>,
}

impl ObscuraManager {
    pub fn new(port: u16) -> Self {
        ObscuraManager {
            port,
            child: Mutex::new(None),
        }
    }

    /// 启动 obscura serve（幂等：已在运行则直接返回）
    pub fn start(&self, app: &AppHandle) -> Result<()> {
        // 已在运行？
        if self.child.lock().unwrap().is_some() {
            return Ok(());
        }
        // 端口已被占用（可能是上次残留进程）？
        if self.is_port_open() {
            warn!("端口 {} 已被占用，跳过启动", self.port);
            return Ok(());
        }

        info!("启动 obscura serve，端口 {}", self.port);
        let (_, child) = app
            .shell()
            .sidecar("obscura")?
            .args(["serve", "--port", &self.port.to_string(), "--stealth"])
            .spawn()
            .context("启动 obscura sidecar 失败")?;

        *self.child.lock().unwrap() = Some(child);

        // 等待端口就绪（最多 10s）
        for _ in 0..50 {
            if self.is_port_open() {
                info!("obscura serve 就绪，端口 {}", self.port);
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        anyhow::bail!("obscura serve 启动超时（端口 {} 未就绪）", self.port)
    }

    /// 停止 obscura 进程
    pub fn stop(&self) {
        if let Some(child) = self.child.lock().unwrap().take() {
            info!("停止 obscura serve");
            let _ = child.kill();
        }
    }

    /// 检查端口是否可连接
    pub fn is_port_open(&self) -> bool {
        TcpStream::connect_timeout(
            &format!("127.0.0.1:{}", self.port).parse().unwrap(),
            std::time::Duration::from_millis(500),
        )
        .is_ok()
    }

    /// CDP WebSocket 地址
    pub fn ws_url(&self) -> String {
        format!("ws://127.0.0.1:{}/devtools/browser", self.port)
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for ObscuraManager {
    fn drop(&mut self) {
        self.stop();
    }
}
