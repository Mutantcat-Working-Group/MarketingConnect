// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
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
use tracing::{error, info, warn};

/// 发布调度器
pub struct PublishScheduler {
    scheduler: Arc<JobScheduler>,
}

impl PublishScheduler {
    /// 创建并启动调度器
    ///
    /// `JobScheduler::new()` 只创建存储与通道，必须再调用 `start()`
    /// 才会启动 500ms 的 tick 循环；漏掉这一步时已注册的作业永远不会触发。
    pub async fn new() -> Result<Self> {
        let scheduler = JobScheduler::new().await?;
        scheduler.start().await?;
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
        // 先克隆用于日志（闭包会 move 走原值）
        let task_id_log = task_id.clone();
        let job = Job::new_async(cron_expr, move |_uuid, _lock| {
            let app = app.clone();
            let task_id = task_id.clone();
            let account_id = account_id.clone();
            Box::pin(async move {
                execute_publish(&app, &task_id, &account_id).await;
            })
        })?;
        self.scheduler.add(job).await?;
        info!("已注册定时任务 {}（cron: {}）", task_id_log, cron_expr);
        Ok(())
    }
}

/// 启动时重建所有已存在的 cron 任务
///
/// 调度器的作业只存在于内存中，重启后必须按数据库里的 cron_expr 重新注册，
/// 否则用户之前创建的定时任务会静默失效。
pub async fn reload_jobs(
    app: &AppHandle,
    scheduler: &PublishScheduler,
    db: &Db,
) -> (usize, usize) {
    let mut registered = 0;
    let mut failed = 0;

    let accounts = match db.list_accounts() {
        Ok(accounts) => accounts,
        Err(error) => {
            error!("重建定时任务失败，读取账号出错: {}", error);
            return (0, 0);
        }
    };

    for account in accounts {
        let tasks = match db.list_tasks(&account.id) {
            Ok(tasks) => tasks,
            Err(error) => {
                warn!("读取账号 {} 的任务失败: {}", account.name, error);
                continue;
            }
        };
        for task in tasks {
            let Some(cron) = task.cron_expr.clone().filter(|c| !c.trim().is_empty()) else {
                continue;
            };
            match scheduler
                .add_job(app.clone(), &cron, task.id.clone(), account.id.clone())
                .await
            {
                Ok(()) => registered += 1,
                Err(error) => {
                    failed += 1;
                    error!("重建定时任务 {} 失败: {}", task.id, error);
                }
            }
        }
    }

    info!("定时任务重建完成：成功 {} 个，失败 {} 个", registered, failed);
    (registered, failed)
}

/// 执行一次发布任务
pub(crate) async fn execute_publish(app: &AppHandle, task_id: &str, account_id: &str) {
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

    // 2. 构造发布内容
    //
    // 内容只依赖任务记录、不依赖浏览器会话，因此上移到「解析适配器 / 打开会话」
    // 之前：令牌型平台必须能在完全不打开会话的情况下发布。
    let content = content_from_task(&task);

    // 3. 先解析适配器，才能判断该平台是否需要浏览器会话
    let adapter = match platforms::find_adapter(&account.platform, Some(&db)) {
        Some(a) => a,
        None => {
            fail_task(&db, task_id, &format!("不支持的平台: {}", account.platform));
            return;
        }
    };

    // 3.5 发布前能力校验：在打开任何浏览器会话之前完成，令牌分支与浏览器分支
    //     共用。这样 `publish: false` 的平台（如抖音 / 公众号）会直接判失败，
    //     而不是先启动一个注定失败的浏览器实例。本函数返回 `()`，不能用 `?`。
    if let Err(e) = adapter.capabilities().validate(&content) {
        fail_task(&db, task_id, &format!("内容超出平台能力: {}", e));
        return;
    }

    // 成功收尾只有这一处：令牌分支与浏览器分支都调用它，避免两条路径行为不一致。
    // （本函数返回 `()`，所以令牌分支用 `match` + `fail_task` 显式处理错误，不能用 `?`。）
    let mark_success = || {
        let _ = db.update_task_status(task_id, "done", Some("发布成功"));
        let _ = db.log_event(Some(account_id), "info", "publish", Some("发布成功"));
        info!("任务 {} 发布成功", task_id);
    };

    // 4. 令牌型平台：取令牌 → publish_with_token。
    //    全程不调用 sessions.open / get_page，不依赖浏览器引擎。
    if adapter.token_hint().is_some() {
        let token = match db.get_api_token(account_id) {
            Ok(Some(t)) => t,
            Ok(None) => {
                fail_task(&db, task_id, "该账号尚未配置 API 令牌，请在账号设置中粘贴");
                return;
            }
            Err(e) => {
                fail_task(&db, task_id, &format!("读取 API 令牌失败: {}", e));
                return;
            }
        };

        match adapter.publish_with_token(&token, &content).await {
            Ok(()) => mark_success(),
            Err(e) => fail_task(&db, task_id, &format!("发布失败: {}", e)),
        }
        return;
    }

    // 5. 浏览器族：到这里才需要会话（按代理自动启动实例）
    let sessions = app.state::<SessionRegistry>();

    // 交互式登录期间不要抢占页面；这是暂态，保留任务等下一次调度重试，
    // 不能标记失败，否则用户完成登录后任务已永久丢失。
    if sessions.is_login_active(account_id) {
        warn!("账号 {} 正在登录验证，跳过本次调度", account_id);
        return;
    }

    if let Err(e) = sessions.open(app, &db, &account).await {
        // 打开会话失败多为暂态（引擎未就绪、端口占用、网络），同样保留任务。
        warn!("账号 {} 打开会话失败，跳过本次调度: {}", account_id, e);
        return;
    }

    let page = match sessions.get_page(account_id) {
        Some(p) => p,
        None => {
            fail_task(&db, task_id, "会话页面不存在");
            return;
        }
    };

    if sessions.is_login_active(account_id) {
        warn!("账号 {} 登录验证中，取消本次发布（任务保留，等待下次调度）", account_id);
        return;
    }

    match adapter.publish(&page, &content).await {
        Ok(()) => mark_success(),
        Err(e) => {
            fail_task(&db, task_id, &format!("发布失败: {}", e));
        }
    }
}

/// 从任务记录构造发布内容（纯函数，不依赖浏览器会话）
///
/// 令牌型与浏览器族两条发布路径共用。解析规则与 `create_publish_task` 的写入一致：
/// 媒体以 `,` 分隔、话题以 `,` 分隔、线程分段以 `||` 分隔，`scheduled_at` 单位是秒。
fn content_from_task(task: &crate::db::PublishTask) -> PublishContent {
    let media_paths: Vec<String> = task
        .media_paths
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let topics: Vec<String> = task
        .topics
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let thread_segments: Vec<String> = task
        .thread_segments
        .as_deref()
        .unwrap_or("")
        .split("||")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    PublishContent {
        title: task.title.clone(),
        text: task.content.clone().unwrap_or_default(),
        media_paths,
        cover_path: task.cover_path.clone(),
        topics,
        thread_segments,
        // 注意：任务表里的 `scheduled_at` 是**本应用的调度时间**（何时该由
        // scheduler 轮询发起发布），而 `PublishContent.schedule_at` 是**要求
        // 平台在其内部定时发布**，两者语义不同。本批次所有平台的 schedule
        // 能力均为 false，`validate()` 会拒绝非 None 的值，直接传下去会让任何
        // 带 scheduled_at 的任务永久发布失败；平台内定时发布尚未实现，故这里
        // 一律为 None。不要用 task.scheduled_at 填充。
        schedule_at: None,
    }
}

/// 标记任务失败并记录日志
fn fail_task(db: &Db, task_id: &str, reason: &str) {
    error!("任务 {} 失败: {}", task_id, reason);
    let _ = db.update_task_status(task_id, "failed", Some(reason));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::PublishTask;

    fn task_with(scheduled_at: Option<i64>) -> PublishTask {
        PublishTask {
            id: "task-1".into(),
            account_id: "account-1".into(),
            title: Some("标题".into()),
            content: Some("正文".into()),
            media_paths: Some("/tmp/a.png, /tmp/b.png".into()),
            cover_path: Some("/tmp/cover.png".into()),
            topics: Some("话题A, 话题B".into()),
            thread_segments: Some("第一段 || 第二段".into()),
            cron_expr: Some("0 9 * * *".into()),
            status: "pending".into(),
            result: None,
            scheduled_at,
            executed_at: None,
            created_at: 0,
        }
    }

    #[test]
    fn content_from_task_never_forwards_schedule_at() {
        // 任务表的 scheduled_at 是本应用的调度时间，不是「平台内定时发布」。
        // 所有平台 schedule 能力均为 false，validate() 会拒绝非 None 值，
        // 因此 content_from_task 必须一律产出 None，否则带 scheduled_at 的任务
        // 会在发布时永久失败。这里用有值和空值两种情况守护该契约。
        assert_eq!(content_from_task(&task_with(Some(1_800_000_000))).schedule_at, None);
        assert_eq!(content_from_task(&task_with(None)).schedule_at, None);
    }

    #[test]
    fn content_from_task_parses_extended_fields() {
        // 顺带守护解析规则：媒体/话题以 `,` 分隔、线程以 `||` 分隔。
        let content = content_from_task(&task_with(Some(1_800_000_000)));
        assert_eq!(content.title.as_deref(), Some("标题"));
        assert_eq!(content.text, "正文");
        assert_eq!(content.media_paths, vec!["/tmp/a.png", "/tmp/b.png"]);
        assert_eq!(content.topics, vec!["话题A", "话题B"]);
        assert_eq!(content.thread_segments, vec!["第一段", "第二段"]);
        assert_eq!(content.cover_path.as_deref(), Some("/tmp/cover.png"));
    }
}
