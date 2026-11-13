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

pub mod engine;
mod client;
mod eval;
mod relay;

pub use client::{CdpBrowser, CdpState};
pub use eval::evaluate;
pub use relay::CdpRelay;

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager}; // app.state::<Db>() 与 app.path()
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;
use tracing::{info, warn};

/// 直连实例的 key
pub const DIRECT_KEY: &str = "direct";

/// 手动发布窗口的实例 key 前缀
///
/// 手动发布需要一个**用户看得见、能直接点**的浏览器窗口，而 headless 实例
/// 无法承载这种交互（截图+转发坐标的方式无法完成拖拽、下拉、富文本编辑）。
/// 因此为每个账号另起一个**非 headless** 的 Chrome 实例，key 形如
/// `manual:<account_id>`，与 headless 实例互不干扰。
pub const MANUAL_KEY_PREFIX: &str = "manual:";

/// 起始端口
const BASE_PORT: u16 = 9222;

/// 引擎启动选项
///
/// `Default` 表示现有 headless 行为：按代理分组、用 `<key>` 作为 profile 目录。
/// **必须手写 Default 让 headless 默认为 true**，derive 的默认是 false，
/// 会让所有既有调用点静默变成有窗口模式。
struct LaunchOptions {
    /// 是否以 headless 模式启动
    headless: bool,
    /// 覆盖 profile 目录后缀（手动窗口用 `manual:<id>`，避免与 headless 冲突）
    profile_suffix: Option<String>,
    /// 覆盖代理设置
    proxy: Option<String>,
    /// 覆盖实例 key
    key: Option<String>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            headless: true,
            profile_suffix: None,
            proxy: None,
            key: None,
        }
    }
}

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
        self.get_or_start_with_options(app, proxy, LaunchOptions::default())
    }

    /// 启动一个**用户可见、可直接操作**的浏览器窗口实例，返回端口
    ///
    /// 用于「平台暂不支持自动发布，我想自己手动发」的场景：直接弹出真实
    /// Chrome 窗口，账号登录态与 headless 实例完全隔离（独立 profile）。
    ///
    /// `headless=false` 会去掉 `--headless=new` 并加上 `--window-size`，
    /// 让 Chrome 正常显示窗口。此时**不要再复用** headless 实例：同一个
    /// `--user-data-dir` 已被 headless 进程占用，Chrome 的 ProcessSingleton
    /// 会让第二个进程直接退出。因此这里用 `manual:<account_id>` 作为 key，
    /// 并在 profile 目录上也加同一后缀，从根上避免冲突。
    pub fn get_or_start_manual(
        &self,
        app: &AppHandle,
        account_id: &str,
        proxy: Option<&str>,
    ) -> Result<u16> {
        let key = format!("{MANUAL_KEY_PREFIX}{account_id}");
        self.get_or_start_with_options(
            app,
            None,
            LaunchOptions {
                headless: false,
                profile_suffix: Some(key.clone()),
                // **必须带上账号自己的代理**：手动窗口是用户真实操作、真实
                // 登录/发布的窗口，走错出口等于用本机公网 IP 登录矩阵账号，
                // 直接暴露账号关联。此前这里写死 None，配置了代理的账号
                // 在手动窗口里会静默走直连。
                proxy: proxy.map(str::to_string),
                key: Some(key),
            },
        )
    }

    fn get_or_start_with_options(
        &self,
        app: &AppHandle,
        proxy: Option<&str>,
        options: LaunchOptions,
    ) -> Result<u16> {
        let LaunchOptions {
            headless,
            profile_suffix,
            proxy: proxy_override,
            key: key_override,
        } = options;
        let proxy: Option<String> = proxy_override.or_else(|| proxy.map(str::to_string));
        // headless 实例按代理分组；手动窗口实例用独立的 `manual:<id>` key，
        // 否则会覆盖同一代理下正在使用的 headless 实例记录。
        let key = key_override.unwrap_or_else(|| {
            proxy
                .clone()
                .unwrap_or_else(|| DIRECT_KEY.to_string())
        });

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
            "启动浏览器引擎实例（代理: {}，端口: {}）",
            proxy.as_deref().unwrap_or("直连"),
            port
        );

        // 构建 sidecar 命令。先解析「用哪个可执行文件」：
        // 显式覆盖 → 本机 Chrome → 已下载的 Chrome for Testing → 自动下载
        //
        // 0) 设置项需要从托管状态取 Db
        let db = app.state::<crate::db::Db>();
        let data_dir = app.path().app_data_dir().context("无法确定应用数据目录")?;

        // 锁定版本：第一次下载后写入设置，之后固定复用同一版本
        let version = db
            .get_setting("engine_version")
            .ok()
            .flatten()
            .unwrap_or_else(|| engine::LOCKED_CFT_VERSION.to_string());

        // 1) 显式覆盖（MC_ENGINE_CMD 的第一段即可执行文件）
        let explicit = std::env::var("MC_ENGINE_CMD")
            .ok()
            .and_then(|raw| split_command(&raw).into_iter().next())
            .map(PathBuf::from);

        // 2) 本机 Chrome
        let system = engine::find_system_chrome();

        // 3) 应用数据目录中已下载的引擎
        let downloaded = engine::installed_chrome_path(&data_dir, &version).filter(|p| p.is_file());

        // 4) 都没有则下载
        let resolved = match engine::pick_engine(explicit, system, downloaded) {
            Some(found) => found,
            None => {
                // 后面的真 Chrome 分支还要用 data_dir 拼 profile 目录，故此处传副本
                let exe = fetch_engine_blocking(data_dir.clone(), version.clone())?;
                let _ = db.set_setting("engine_version", &version);
                (exe, engine::EngineOrigin::Downloaded)
            }
        };
        info!("使用引擎: {}（来源: {:?}）", resolved.0.display(), resolved.1);

        let cmd = match resolved.1 {
            engine::EngineOrigin::Explicit => {
                // 高级/验证用途：用外部命令充当引擎，便于把应用指向真正的
                // Chromium 做对照。模板支持 {port} 与 {proxy} 占位符，
                // 含空格的路径请用引号包起来。
                let raw = std::env::var("MC_ENGINE_CMD").unwrap_or_default();
                let mut parts = split_command(&raw);
                if parts.is_empty() {
                    anyhow::bail!("MC_ENGINE_CMD 为空");
                }
                let program = parts.remove(0);
                let args: Vec<String> = parts
                    .iter()
                    .map(|arg| {
                        arg.replace("{port}", &port.to_string())
                            .replace("{proxy}", proxy.as_deref().unwrap_or(""))
                    })
                    .collect();
                info!("启动自定义引擎（代理: {}，端口: {}）", proxy.as_deref().unwrap_or("直连"), port);
                app.shell().command(program).args(args)
            }
            engine::EngineOrigin::System | engine::EngineOrigin::Downloaded => {
                // 这是真正的 Chrome / Chromium，参数与 obscura 完全不同：
                // obscura 用 `serve --port … --stealth --font-dir …`，而 Chrome
                // 没有 `serve` 子命令，会把 serve 当成网址、永不在该端口监听。
                // 真 Chrome 原生加载系统字体，因此不需要 --font-dir。
                // 每个实例（每个代理）必须用独立的 profile 目录：
                // Chrome 对同一 --user-data-dir 的第二个进程会直接中止。
                // profile 目录：手动窗口加后缀，避免与 headless 实例的
                // 同一 user-data-dir 冲突（Chrome ProcessSingleton 会让
                // 第二个进程直接退出）。
                let profile = match profile_suffix.as_deref() {
                    Some(suffix) => engine::chrome_profile_dir(&data_dir, suffix),
                    None => engine::chrome_profile_dir(&data_dir, &key),
                };
                let mut args: Vec<String> = Vec::new();
                if headless {
                    args.push("--headless=new".to_string());
                }
                args.push(format!("--remote-debugging-port={port}"));
                // 外部 CDP 客户端（本机中继）需要被允许
                args.push("--remote-allow-origins=*".to_string());
                args.push("--no-first-run".to_string());
                args.push("--no-default-browser-check".to_string());
                args.push("--disable-blink-features=AutomationControlled".to_string());
                // Headless Chrome 默认把页面当作「后台标签页」：渲染进程被
                // 降优先级后 `document.visibilityState` 恒为 "hidden"，
                // CSS 入场动画（animation-play-state）随之被暂停。
                // 抖音登录卡正是靠 `login-enter` 动画从 opacity:0 淡入，
                // 动画一暂停就永远停在 0——截图里只剩背景和二维码外的空白，
                // 用户无法扫码也无法点击任何登录控件。这三个开关关掉后台
                // 降级，让页面按「可见前台」渲染。
                args.push("--disable-backgrounding-occluded-windows".to_string());
                args.push("--disable-background-timer-throttling".to_string());
                args.push("--disable-renderer-backgrounding".to_string());
                args.push(format!("--user-data-dir={}", profile.display()));
                // 有窗口模式给更大的默认尺寸，方便手动填写发布表单
                args.push(if headless {
                    "--window-size=1280,860".to_string()
                } else {
                    "--window-size=1440,900".to_string()
                });
                // UA 的主版本必须来自**实际启动的这个二进制**，不能直接用
                // `version`（那是锁定的 CfT 版本）：本机 Chrome 是 156 而
                // 锁定的是 157 时，会造出「UA 声明 157、内核 156」的假 UA。
                // 探测失败则整项省略，让 Chrome 用它自己的真实 UA
                // （去掉 --user-agent 后 Chrome 会补上 HeadlessChrome 标记，
                //  但那仍然比假 UA 安全，且只在探测失败时发生）。
                match engine::detect_major_version(&resolved.0) {
                    Some(major) => {
                        args.push(format!("--user-agent={}", engine::chrome_user_agent(&major)));
                    }
                    None => {
                        warn!("探测不到引擎 {} 的版本，省略 --user-agent", resolved.0.display());
                    }
                }
                if let Some(p) = proxy.as_deref() {
                    args.push(format!("--proxy-server={p}"));
                }
                app.shell()
                    .command(resolved.0.to_string_lossy().into_owned())
                    .args(args)
            }
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
            anyhow::bail!("引擎启动超时（端口 {} 未就绪）", port);
        }

        info!("浏览器引擎实例就绪（端口 {}）", port);
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
            info!("停止浏览器引擎实例（端口 {}）", inst.port);
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
            info!("停止浏览器引擎实例（端口 {}）", inst.port);
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

/// 同步函数里不能 await；又因为 `get_or_start` 会被 async 的 `resolve_browser`
/// 调用，在运行时线程上 `block_on` 会 panic。因此在独立线程上用独立运行时
/// 执行下载，与 Tauri 的运行时完全隔离。
fn fetch_engine_blocking(data_dir: std::path::PathBuf, version: String) -> Result<std::path::PathBuf> {
    std::thread::scope(|scope| {
        scope
            .spawn(move || -> Result<std::path::PathBuf> {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("创建下载运行时失败")?;
                runtime.block_on(async {
                    let key = engine::chrome_for_testing_platform_key()
                        .context("当前平台不支持自动获取引擎")?;
                    let feed: serde_json::Value = reqwest::get(engine::CFT_VERSIONS_URL)
                        .await
                        .context("获取引擎版本清单失败")?
                        .json()
                        .await
                        .context("解析引擎版本清单失败")?;
                    let url = engine::stable_download_url(&feed, key)
                        .context("版本清单中没有当前平台的引擎")?;
                    engine::download_chrome(&data_dir, &url, &version).await
                })
            })
            .join()
            .map_err(|_| anyhow!("下载引擎的线程异常退出"))?
    })
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
