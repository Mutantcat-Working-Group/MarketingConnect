//! 定时发布调度器
//!
//! 基于 tokio-cron-scheduler：
//! - 创建发布任务时若带 cron 表达式，注册定时作业
//! - 作业触发时执行发布流程（会话页面 → 平台适配器 → 更新任务状态）

use crate::db::Db;
use crate::platforms::{self, PublishContent};
use crate::session::SessionRegistry;
use anyhow::Result;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info};

/// 发布调度器
pub struct PublishScheduler {
    scheduler: Arc<JobScheduler>,
}

impl PublishScheduler {
    /// 创建并启动调度器
    pub async fn new() -> Result<Self> {
        let scheduler = JobScheduler::new().await?;
        info!("发布调度器已启动");
        Ok(PublishScheduler {
            scheduler: Arc::new(scheduler),
        })
    }

    /// 注册定时发布作业
    ///
    /// 到点后执行发布流程：读取任务 → 获取账号会话页面 →
    /// 调用平台适配器发布 → 更新任务状态。
    pub async fn add_job(
        &self,
        app: AppHandle,
        cron_expr: &str,
        task_id: String,
        account_id: String,
    ) -> Result<()> {
        let job = Job::new_async(cron_expr, move |_uuid, _lock| {
            let app = app.clone();
            let task_id = task_id.clone();
            let account_id = account_id.clone();
            Box::pin(async move {
                execute_publish(&app, &task_id, &account_id).await;
            })
        })?;
        self.scheduler.add(job).await?;
        info!("已注册定时任务 {}（cron: {}）", task_id, cron_expr);
        Ok(())
    }
}

/// 执行一次发布任务
async fn execute_publish(app: &AppHandle, task_id: &str, account_id: &str) {
    info!("定时任务触发: {}（账号 {}）", task_id, account_id);

    // 1. 读取任务与账号
    let db = app.state::<Db>();
    let accounts = match db.list_accounts() {
        Ok(a) => a,
        Err(e) => {
            error!("读取账号失败: {}", e);
            return;
        }
    };
    let account = match accounts.iter().find(|a| a.id == account_id) {
        Some(a) => a.clone(),
        None => {
            error!("账号 {} 不存在", account_id);
            return;
        }
    };

    // 任务内容（从任务列表里找，骨架阶段任务内容已在创建时入库）
    let tasks = match db.list_tasks(account_id) {
        Ok(t) => t,
        Err(e) => {
            error!("读取任务失败: {}", e);
            return;
        }
    };
    let task = match tasks.iter().find(|t| t.id == task_id) {
        Some(t) => t.clone(),
        None => {
            error!("任务 {} 不存在", task_id);
            return;
        }
    };

    // 2. 获取账号会话页面（按代理自动启动实例）
    let sessions = app.state::<SessionRegistry>();

    if let Err(e) = sessions.open(app, &db, &account).await {
        fail_task(&db, task_id, &format!("打开会话失败: {}", e));
        return;
    }

    let page = match sessions.get_page(account_id) {
        Some(p) => p,
        None => {
            fail_task(&db, task_id, "会话页面不存在");
            return;
        }
    };

    // 3. 构造发布内容
    let media_paths: Vec<String> = task
        .media_paths
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let content = PublishContent {
        title: task.title.clone(),
        text: task.content.clone().unwrap_or_default(),
        media_paths,
    };

    // 4. 调用平台适配器发布
    let adapter = match platforms::find_adapter(&account.platform) {
        Some(a) => a,
        None => {
            fail_task(&db, task_id, &format!("不支持的平台: {}", account.platform));
            return;
        }
    };

    match adapter.publish(&page, &content).await {
        Ok(()) => {
            let _ = db.update_task_status(task_id, "done", Some("发布成功"));
            let _ = db.log_event(Some(account_id), "info", "publish", Some("发布成功"));
            info!("任务 {} 发布成功", task_id);
        }
        Err(e) => {
            fail_task(&db, task_id, &format!("发布失败: {}", e));
        }
    }
}

/// 标记任务失败并记录日志
fn fail_task(db: &Db, task_id: &str, reason: &str) {
    error!("任务 {} 失败: {}", task_id, reason);
    let _ = db.update_task_status(task_id, "failed", Some(reason));
}
