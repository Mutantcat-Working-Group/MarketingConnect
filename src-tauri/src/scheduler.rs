//! 定时发布调度器（骨架）
//!
//! 基于 tokio-cron-scheduler，到点触发发布任务。
//! 当前为骨架：任务执行逻辑待 CDP 集成后填充。

use anyhow::Result;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::info;

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

    /// 注册一个 cron 任务（骨架：仅记录日志）
    pub async fn add_job(&self, cron_expr: &str, task_id: &str) -> Result<()> {
        let task_id = task_id.to_string();
        let job = Job::new_async(cron_expr, move |_uuid, _lock| {
            let task_id = task_id.clone();
            Box::pin(async move {
                info!("定时任务触发: {}", task_id);
                // TODO: 到点执行发布流程（CDP 驱动对应账号的 context）
            })
        })?;
        self.scheduler.add(job).await?;
        Ok(())
    }
}
