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
mod eval;
mod relay;

pub use client::{CdpBrowser, CdpState};
pub use eval::evaluate;
pub use relay::CdpRelay;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::TcpStream;
use std::path::PathBuf;
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
        let cmd = if let Some(command) = std::env::var_os("MC_ENGINE_CMD") {
            // 高级/验证用途：用外部命令充当引擎，便于把应用指向真正的
            // Chromium 做对照。模板支持 {port} 与 {proxy} 占位符，
            // 含空格的路径请用引号包起来。
            let command = command.to_string_lossy();
            let mut parts = split_command(&command);
            if parts.is_empty() {
                anyhow::bail!("MC_ENGINE_CMD 为空");
            }
            let program = parts.remove(0);
            let args: Vec<String> = parts
                .iter()
                .map(|arg| {
                    arg.replace("{port}", &port.to_string())
                        .replace("{proxy}", proxy.unwrap_or(""))
                })
                .collect();
            info!("启动自定义引擎（代理: {}，端口: {}）", proxy.unwrap_or("直连"), port);
            app.shell().command(program).args(args)
        } else {
            let mut cmd = app
                .shell()
                .sidecar("obscura")?
                .args(["serve", "--port", &port.to_string(), "--stealth"]);
            // 引擎默认不加载任何字体，不指定字体目录时页面文字（含全部中文）
            // 都会渲染成方块。字体是懒加载，实测零启动开销。
            for dir in font_dirs() {
                cmd = cmd.args(["--font-dir", &dir]);
            }
            if let Some(p) = proxy {
                cmd = cmd.args(["--proxy", p]);
            }
            cmd
        };

        let (_, child) = cmd.spawn().context("启动引擎失败")?;

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

/// 引擎需要显式指定字体目录，否则页面文字全部渲染成方块。
///
/// 返回各平台常见的系统字体目录（仅保留实际存在的），另外支持用
/// 环境变量 `MC_FONT_DIRS`（`:` 或 `;` 分隔）追加自定义目录，
/// 用于补装字体或系统字体不在默认位置的情况。
pub fn font_dirs() -> Vec<String> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(windir) = std::env::var_os("SystemRoot") {
            dirs.push(PathBuf::from(windir).join("Fonts"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft/Windows/Fonts"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
    }

    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        #[cfg(target_os = "macos")]
        dirs.push(home.join("Library/Fonts"));
        #[cfg(target_os = "linux")]
        {
            dirs.push(home.join(".local/share/fonts"));
            dirs.push(home.join(".fonts"));
        }
    }

    if let Some(extra) = std::env::var_os("MC_FONT_DIRS") {
        let extra = extra.to_string_lossy();
        for part in extra.split([':', ';']) {
            let part = part.trim();
            if !part.is_empty() {
                dirs.push(PathBuf::from(part));
            }
        }
    }

    dirs.retain(|dir| dir.is_dir());
    dirs.sort();
    dirs.dedup();
    dirs.into_iter()
        .map(|dir| dir.to_string_lossy().into_owned())
        .collect()
}

/// 按 shell 习惯切分命令模板，支持单/双引号包裹带空格的路径。
fn split_command(template: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in template.chars() {
        match quote {
            Some(open) => {
                if ch == open {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            None => current.push(ch),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// 构造 CDP WebSocket 地址
pub fn ws_url(port: u16) -> String {
    format!("ws://127.0.0.1:{}/devtools/browser", port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_dirs_are_existing_and_unique() {
        let dirs = font_dirs();
        for dir in &dirs {
            assert!(
                std::path::Path::new(dir).is_dir(),
                "字体目录必须真实存在: {dir}"
            );
        }
        let mut sorted = dirs.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(dirs, sorted, "字体目录应当去重且有序");
    }

    #[test]
    fn font_dirs_include_platform_system_fonts() {
        #[cfg(target_os = "macos")]
        assert!(
            font_dirs().iter().any(|d| d == "/System/Library/Fonts"),
            "macOS 必须带上系统字体目录，否则页面中文会渲染成方块"
        );
        #[cfg(target_os = "linux")]
        assert!(
            font_dirs().iter().any(|d| d == "/usr/share/fonts") || !font_dirs().is_empty(),
            "Linux 应带上常见字体目录"
        );
    }

    #[test]
    fn splits_command_template_respecting_quotes() {
        assert_eq!(
            split_command("/usr/bin/engine serve --port {port}"),
            vec!["/usr/bin/engine", "serve", "--port", "{port}"]
        );
        // 含空格的路径必须用引号包起来，且引号本身不能进入参数
        let parts = split_command("'/Apps/Google Chrome.app/bin' --remote-debugging-port={port}");
        assert_eq!(parts[0], "/Apps/Google Chrome.app/bin");
        assert_eq!(parts[1], "--remote-debugging-port={port}");
        assert_eq!(split_command("   "), Vec::<String>::new());
        // 引号内的空格保留，引号外的连续空白折叠
        assert_eq!(
            split_command("a \"b  c\"   d"),
            vec!["a", "b  c", "d"]
        );
    }
}
