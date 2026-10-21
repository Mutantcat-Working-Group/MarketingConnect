//! 营销通 - 多平台矩阵营销号统一管理与发布工具
//!
//! 架构：Tauri 2 (Rust) + Vue 3 前端 + Obscura 无头浏览器引擎

mod accounts;
mod crypto;
mod db;
mod obscura;
mod platforms;
mod scheduler;
mod session;

use db::Db;
use obscura::{CdpState, ObscuraManager};
use session::SessionRegistry;
use std::sync::Arc;
use tauri::Manager;

/// Obscura CDP 服务端口
const OBSCURA_PORT: u16 = 9222;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("marketing_connect=info".parse().unwrap()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // 数据库：存于应用数据目录
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("无法获取应用数据目录");
            let db_path = app_dir.join("marketing-connect.db");
            let db = Db::open(&db_path).expect("数据库初始化失败");
            app.manage(db);

            // Obscura 引擎管理器
            app.manage(ObscuraManager::new(OBSCURA_PORT));

            // CDP 连接状态与账号会话注册表
            app.manage(CdpState::new());
            app.manage(SessionRegistry::new());

            // 定时调度器（后台启动）
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match scheduler::PublishScheduler::new().await {
                    Ok(s) => {
                        handle.manage(Arc::new(s));
                    }
                    Err(e) => {
                        tracing::error!("调度器启动失败: {}", e);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            accounts::list_accounts,
            accounts::create_account,
            accounts::delete_account,
            accounts::start_engine,
            accounts::stop_engine,
            accounts::engine_status,
            accounts::open_account_session,
            accounts::close_account_session,
            accounts::check_account_session,
            accounts::save_account_cookies,
            accounts::create_publish_task,
            accounts::list_publish_tasks,
            accounts::recent_logs,
            accounts::list_platforms,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
