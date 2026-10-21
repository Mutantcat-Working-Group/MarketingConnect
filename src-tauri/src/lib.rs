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
mod session_monitor;
mod sensitive_words;
mod sms;

use db::Db;
use obscura::{CdpState, ObscuraManager};
use session::SessionRegistry;
use std::sync::Arc;
use tauri::Manager;

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
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // 数据库：存于应用数据目录
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("无法获取应用数据目录");
            let db_path = app_dir.join("marketing-connect.db");
            let db = Db::open(&db_path).expect("数据库初始化失败");
            app.manage(db);

            // Obscura 引擎管理器（多实例：按代理分组）
            app.manage(ObscuraManager::new());

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

            // 会话健康监控（后台周期检测掉号，默认 5 分钟）
            session_monitor::spawn_monitor(app.handle().clone(), None);

            // 系统托盘（最小化到托盘 + 快捷菜单）
            setup_tray(app)?;

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
            accounts::capture_screenshot,
            accounts::create_publish_task,
            accounts::batch_publish,
            accounts::list_publish_tasks,
            accounts::recent_logs,
            accounts::list_platforms,
            accounts::get_sms_config,
            accounts::save_sms_config,
            accounts::test_sms,
            accounts::list_custom_platforms,
            accounts::create_custom_platform,
            accounts::delete_custom_platform,
            accounts::check_sensitive_words,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 系统托盘：显示主窗口 / 引擎状态 / 退出
///
/// 关闭窗口时隐藏到托盘（保持后台会话监控运行），
/// 通过托盘菜单「退出」或左键单击托盘图标恢复窗口。
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::WindowEvent;

    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let status = MenuItem::with_id(app, "status", "引擎状态", false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &status, &quit])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .expect("缺少应用图标");

    // 关闭窗口 → 隐藏到托盘（不退出，保持后台监控）
    if let Some(window) = app.get_webview_window("main") {
        let w = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = w.hide();
            }
        });
    }

    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("营销通")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 左键单击显示主窗口
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}
