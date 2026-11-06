// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! SQLite 数据层
//!
//! 表结构：
//! - accounts: 营销账号（含加密的帐密与 cookie）
//! - publish_tasks: 发布任务（支持定时）
//! - event_logs: 事件日志（掉号、发布结果等）

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// 账号状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AccountStatus {
    Offline,
    Online,
    Error,
}

impl AccountStatus {
    fn as_str(&self) -> &'static str {
        match self {
            AccountStatus::Offline => "offline",
            AccountStatus::Online => "online",
            AccountStatus::Error => "error",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "online" => AccountStatus::Online,
            "error" => AccountStatus::Error,
            _ => AccountStatus::Offline,
        }
    }
}

/// 账号记录（不含敏感字段，用于前端展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub platform: String,
    pub name: String,
    pub username: String,
    pub proxy_url: Option<String>,
    pub group_name: Option<String>,
    pub status: AccountStatus,
    pub last_check_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 新建账号的输入
#[derive(Debug, Clone, Deserialize)]
pub struct NewAccount {
    pub platform: String,
    pub name: String,
    #[serde(default)]
    pub username: String,
    pub password: Option<String>,
    pub proxy_url: Option<String>,
    pub group_name: Option<String>,
}

/// 修改账号输入。平台不可变；空密码保留原值，显式清除优先。
#[derive(Clone, Deserialize)]
pub struct UpdateAccount {
    pub name: String,
    #[serde(default)]
    pub username: String,
    pub password: Option<String>,
    #[serde(default)]
    pub clear_password: bool,
    pub proxy_url: Option<String>,
    pub group_name: Option<String>,
}

/// 统一可选文本字段，空白表示清除。
pub(crate) fn normalize_account_field(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// 发布任务
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishTask {
    pub id: String,
    pub account_id: String,
    pub title: Option<String>,
    pub content: Option<String>,
    pub media_paths: Option<String>,
    /// 封面图路径（视频/文章头图）
    pub cover_path: Option<String>,
    /// 话题标签，以 `,` 分隔存储
    pub topics: Option<String>,
    /// 线程分段，以 `||` 分隔存储（与 scheduler 的解析保持一致）
    pub thread_segments: Option<String>,
    pub cron_expr: Option<String>,
    pub status: String,
    pub result: Option<String>,
    pub scheduled_at: Option<i64>,
    pub executed_at: Option<i64>,
    pub created_at: i64,
}

/// 事件日志
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLog {
    pub id: i64,
    pub account_id: Option<String>,
    pub level: String,
    pub event: String,
    pub detail: Option<String>,
    pub created_at: i64,
}

/// 数据库封装（Mutex 保证线程安全）
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// 打开（或创建）数据库并执行迁移
    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path).context("打开 SQLite 失败")?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;",
        )?;
        let db = Db {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS accounts (
                id TEXT PRIMARY KEY,
                platform TEXT NOT NULL,
                name TEXT NOT NULL,
                username TEXT NOT NULL,
                password_enc BLOB,
                proxy_url TEXT,
                group_name TEXT,
                cookies_enc BLOB,
                status TEXT NOT NULL DEFAULT 'offline',
                last_check_at INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS publish_tasks (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                title TEXT,
                content TEXT,
                media_paths TEXT,
                cron_expr TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                result TEXT,
                scheduled_at INTEGER,
                executed_at INTEGER,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS event_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id TEXT,
                level TEXT NOT NULL,
                event TEXT NOT NULL,
                detail TEXT,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS custom_platforms (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                creator_url TEXT NOT NULL,
                publish_url TEXT NOT NULL,
                title_selector TEXT,
                content_selector TEXT,
                submit_selector TEXT,
                logged_in_pattern TEXT,
                created_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_tasks_account ON publish_tasks(account_id);
            CREATE INDEX IF NOT EXISTS idx_logs_account ON event_logs(account_id);
            ",
        )
        .context("数据库迁移失败")?;

        // 增量迁移：为已有数据库补充 group_name 列（已存在则忽略错误）
        let _ = conn.execute("ALTER TABLE accounts ADD COLUMN group_name TEXT", []);

        // 增量迁移：发布内容扩展字段（已存在则忽略错误）。
        // `scheduled_at` 已在建表语句里，不要重复添加。
        let _ = conn.execute("ALTER TABLE publish_tasks ADD COLUMN cover_path TEXT", []);
        let _ = conn.execute("ALTER TABLE publish_tasks ADD COLUMN topics TEXT", []);
        let _ = conn.execute("ALTER TABLE publish_tasks ADD COLUMN thread_segments TEXT", []);
        // 令牌型平台的登录凭据（加密存储），由 Task 8 的令牌登录读写。
        let _ = conn.execute("ALTER TABLE accounts ADD COLUMN api_token_enc BLOB", []);

        Ok(())
    }

    // ---------- 账号 ----------

    /// 新增账号（密码加密存储）
    pub fn create_account(&self, input: &NewAccount) -> Result<Account> {
        anyhow::ensure!(!input.name.trim().is_empty(), "备注名称不能为空");
        let supported = crate::platforms::is_builtin_platform_id(&input.platform)
            || self.list_custom_platforms()?.iter().any(|p| p.id == input.platform);
        anyhow::ensure!(supported, "不支持的平台");
        let input = NewAccount {
            platform: input.platform.clone(),
            name: input.name.trim().to_string(),
            username: input.username.trim().to_string(),
            password: input.password.clone(),
            proxy_url: normalize_account_field(input.proxy_url.as_deref()),
            group_name: normalize_account_field(input.group_name.as_deref()),
        };
        let conn = self.conn.lock().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();

        let password_enc = match &input.password {
            Some(p) if !p.is_empty() => Some(super::crypto::encrypt_str(p)?),
            _ => None,
        };

        conn.execute(
            "INSERT INTO accounts (id, platform, name, username, password_enc, proxy_url, group_name, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'offline', ?8, ?8)",
            params![
                id,
                input.platform,
                input.name,
                input.username,
                password_enc,
                input.proxy_url,
                input.group_name,
                now
            ],
        )
        .context("插入账号失败")?;

        Ok(Account {
            id,
            platform: input.platform.clone(),
            name: input.name.clone(),
            username: input.username.clone(),
            proxy_url: input.proxy_url.clone(),
            group_name: input.group_name.clone(),
            status: AccountStatus::Offline,
            last_check_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    /// 修改账号；不读取/解密旧密码，空密码保留原加密字节。
    pub fn update_account(&self, id: &str, input: &UpdateAccount) -> Result<Account> {
        anyhow::ensure!(!input.name.trim().is_empty(), "备注名称不能为空");
        let proxy_url = normalize_account_field(input.proxy_url.as_deref());
        let group_name = normalize_account_field(input.group_name.as_deref());
        let replace_password = input.clear_password
            || input.password.as_deref().is_some_and(|p| !p.is_empty());
        let password_enc = if input.clear_password {
            None
        } else {
            match input.password.as_deref().filter(|p| !p.is_empty()) {
                Some(password) => Some(super::crypto::encrypt_str(password)?),
                None => None,
            }
        };
        let now = chrono::Utc::now().timestamp();
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE accounts SET name = ?1, username = ?2,
                password_enc = CASE WHEN ?3 THEN ?4 ELSE password_enc END,
                status = CASE WHEN proxy_url IS NOT ?5 THEN 'offline' ELSE status END,
                last_check_at = CASE WHEN proxy_url IS NOT ?5 THEN NULL ELSE last_check_at END,
                proxy_url = ?5, group_name = ?6, updated_at = ?7 WHERE id = ?8",
            params![input.name.trim(), input.username.trim(), replace_password,
                password_enc, proxy_url, group_name, now, id],
        ).context("更新账号失败")?;
        anyhow::ensure!(changed > 0, "账号不存在");
        conn.query_row(
            "SELECT id, platform, name, username, proxy_url, group_name, status,
                    last_check_at, created_at, updated_at FROM accounts WHERE id = ?1",
            params![id],
            |row| Ok(Account {
                id: row.get(0)?,
                platform: row.get(1)?,
                name: row.get(2)?,
                username: row.get(3)?,
                proxy_url: row.get(4)?,
                group_name: row.get(5)?,
                status: AccountStatus::from_str(&row.get::<_, String>(6)?),
                last_check_at: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            }),
        ).context("读取更新后的账号失败")
    }

    /// 列出所有账号
    pub fn list_accounts(&self) -> Result<Vec<Account>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, platform, name, username, proxy_url, group_name, status, last_check_at, created_at, updated_at
             FROM accounts ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Account {
                id: row.get(0)?,
                platform: row.get(1)?,
                name: row.get(2)?,
                username: row.get(3)?,
                proxy_url: row.get(4)?,
                group_name: row.get(5)?,
                status: AccountStatus::from_str(&row.get::<_, String>(6)?),
                last_check_at: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?;
        let mut accounts = Vec::new();
        for row in rows {
            accounts.push(row?);
        }
        Ok(accounts)
    }

    /// 删除账号（级联删除任务）
    pub fn delete_account(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 更新账号状态
    pub fn update_account_status(&self, id: &str, status: AccountStatus) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "UPDATE accounts SET status = ?1, last_check_at = ?2, updated_at = ?2 WHERE id = ?3",
            params![status.as_str(), now, id],
        )?;
        Ok(())
    }

    /// 获取账号解密后的密码
    pub fn get_account_password(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let enc: Option<Vec<u8>> = conn
            .query_row(
                "SELECT password_enc FROM accounts WHERE id = ?1",
                params![id],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()?
            .flatten();
        match enc {
            Some(bytes) => Ok(Some(super::crypto::decrypt_str(&bytes)?)),
            None => Ok(None),
        }
    }

    /// 保存账号 cookie（加密）
    pub fn save_account_cookies(&self, id: &str, cookies_json: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let enc = super::crypto::encrypt_str(cookies_json)?;
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "UPDATE accounts SET cookies_enc = ?1, updated_at = ?2 WHERE id = ?3",
            params![enc, now, id],
        )?;
        Ok(())
    }

    /// 读取账号 cookie（解密）
    pub fn get_account_cookies(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let enc: Option<Vec<u8>> = conn
            .query_row(
                "SELECT cookies_enc FROM accounts WHERE id = ?1",
                params![id],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()?
            .flatten();
        match enc {
            Some(bytes) => Ok(Some(super::crypto::decrypt_str(&bytes)?)),
            None => Ok(None),
        }
    }

    /// 保存平台 API 令牌（加密存储）
    ///
    /// 列 `api_token_enc` 由 Task 7 的迁移添加（见 `migrate()`），此处不再建列。
    pub fn save_api_token(&self, account_id: &str, token: &str) -> Result<()> {
        let encrypted = super::crypto::encrypt(token.as_bytes())?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET api_token_enc = ?1, updated_at = ?2 WHERE id = ?3",
            params![encrypted, chrono::Utc::now().timestamp(), account_id],
        )?;
        Ok(())
    }

    /// 读取平台 API 令牌（解密），未配置时返回 None
    pub fn get_api_token(&self, account_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let blob: Option<Vec<u8>> = conn
            .query_row(
                "SELECT api_token_enc FROM accounts WHERE id = ?1",
                params![account_id],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()?
            .flatten();
        match blob {
            Some(bytes) if !bytes.is_empty() => {
                let plain = super::crypto::decrypt(&bytes)?;
                Ok(Some(String::from_utf8(plain).context("令牌不是合法文本")?))
            }
            _ => Ok(None),
        }
    }

    // ---------- 发布任务 ----------

    /// 创建发布任务
    ///
    /// `scheduled_at` 单位是**秒**（与 `chrono::Utc::now().timestamp()` 一致），
    /// 直接落库，不做任何毫秒换算。
    pub fn create_task(
        &self,
        account_id: &str,
        title: Option<&str>,
        content: Option<&str>,
        media_paths: Option<&str>,
        cron_expr: Option<&str>,
        scheduled_at: Option<i64>,
        cover_path: Option<&str>,
        topics: Option<&str>,
        thread_segments: Option<&str>,
    ) -> Result<PublishTask> {
        let conn = self.conn.lock().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO publish_tasks (id, account_id, title, content, media_paths, cover_path,
                                        topics, thread_segments, cron_expr, status, scheduled_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', ?10, ?11)",
            params![
                id,
                account_id,
                title,
                content,
                media_paths,
                cover_path,
                topics,
                thread_segments,
                cron_expr,
                scheduled_at,
                now
            ],
        )?;
        Ok(PublishTask {
            id,
            account_id: account_id.to_string(),
            title: title.map(String::from),
            content: content.map(String::from),
            media_paths: media_paths.map(String::from),
            cover_path: cover_path.map(String::from),
            topics: topics.map(String::from),
            thread_segments: thread_segments.map(String::from),
            cron_expr: cron_expr.map(String::from),
            status: "pending".into(),
            result: None,
            scheduled_at,
            executed_at: None,
            created_at: now,
        })
    }

    /// 列出某账号的任务
    pub fn list_tasks(&self, account_id: &str) -> Result<Vec<PublishTask>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, account_id, title, content, media_paths, cover_path, topics,
                    thread_segments, cron_expr, status, result, scheduled_at, executed_at, created_at
             FROM publish_tasks WHERE account_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![account_id], |row| {
            Ok(PublishTask {
                id: row.get(0)?,
                account_id: row.get(1)?,
                title: row.get(2)?,
                content: row.get(3)?,
                media_paths: row.get(4)?,
                cover_path: row.get(5)?,
                topics: row.get(6)?,
                thread_segments: row.get(7)?,
                cron_expr: row.get(8)?,
                status: row.get(9)?,
                result: row.get(10)?,
                scheduled_at: row.get(11)?,
                executed_at: row.get(12)?,
                created_at: row.get(13)?,
            })
        })?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row?);
        }
        Ok(tasks)
    }

    /// 更新任务状态
    pub fn update_task_status(&self, id: &str, status: &str, result: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "UPDATE publish_tasks SET status = ?1, result = ?2, executed_at = ?3 WHERE id = ?4",
            params![status, result, now, id],
        )?;
        Ok(())
    }

    // ---------- 事件日志 ----------

    /// 记录事件
    pub fn log_event(
        &self,
        account_id: Option<&str>,
        level: &str,
        event: &str,
        detail: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO event_logs (account_id, level, event, detail, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![account_id, level, event, detail, now],
        )?;
        Ok(())
    }

    /// 查询最近的事件日志
    pub fn recent_logs(&self, limit: i64) -> Result<Vec<EventLog>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, account_id, level, event, detail, created_at
             FROM event_logs ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(EventLog {
                id: row.get(0)?,
                account_id: row.get(1)?,
                level: row.get(2)?,
                event: row.get(3)?,
                detail: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        let mut logs = Vec::new();
        for row in rows {
            logs.push(row?);
        }
        Ok(logs)
    }

    // ---------- 设置（键值对） ----------

    /// 读取设置项
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let value = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(value)
    }

    /// 写入设置项（upsert）
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = ?3",
            params![key, value, now],
        )?;
        Ok(())
    }

    // ---------- 短信配置 ----------

    /// 保存短信配置（AccessKeySecret 加密存储）
    pub fn save_sms_config(&self, config: &crate::sms::SmsConfig) -> Result<()> {
        // 非敏感字段明文 JSON
        let plain = serde_json::json!({
            "access_key_id": config.access_key_id,
            "sign_name": config.sign_name,
            "template_code": config.template_code,
            "phone_numbers": config.phone_numbers,
        });
        self.set_setting("sms_config", &plain.to_string())?;

        // 敏感字段加密存储（hex 编码）
        let enc = crate::crypto::encrypt_str(&config.access_key_secret)?;
        self.set_setting("sms_secret_enc", &hex::encode(enc))?;
        Ok(())
    }

    /// 读取短信配置（解密 AccessKeySecret）
    pub fn get_sms_config(&self) -> Result<Option<crate::sms::SmsConfig>> {
        let Some(json) = self.get_setting("sms_config")? else {
            return Ok(None);
        };
        let v: serde_json::Value =
            serde_json::from_str(&json).context("短信配置 JSON 解析失败")?;

        // 解密密钥（失败时返回空，由 is_configured 判定）
        let secret = self
            .get_setting("sms_secret_enc")?
            .and_then(|hex_str| hex::decode(hex_str).ok())
            .and_then(|bytes| crate::crypto::decrypt_str(&bytes).ok())
            .unwrap_or_default();

        Ok(Some(crate::sms::SmsConfig {
            access_key_id: v["access_key_id"].as_str().unwrap_or("").to_string(),
            access_key_secret: secret,
            sign_name: v["sign_name"].as_str().unwrap_or("").to_string(),
            template_code: v["template_code"].as_str().unwrap_or("").to_string(),
            phone_numbers: v["phone_numbers"].as_str().unwrap_or("").to_string(),
        }))
    }

    // ---------- 自定义平台 ----------

    /// 列出所有自定义平台
    pub fn list_custom_platforms(&self) -> Result<Vec<crate::platforms::CustomPlatform>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, creator_url, publish_url, title_selector,
                    content_selector, submit_selector, logged_in_pattern
             FROM custom_platforms ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(crate::platforms::CustomPlatform {
                id: row.get(0)?,
                name: row.get(1)?,
                creator_url: row.get(2)?,
                publish_url: row.get(3)?,
                title_selector: row.get(4)?,
                content_selector: row.get(5)?,
                submit_selector: row.get(6)?,
                logged_in_pattern: row.get(7)?,
            })
        })?;
        let mut platforms = Vec::new();
        for row in rows {
            platforms.push(row?);
        }
        Ok(platforms)
    }

    /// 新增自定义平台
    #[allow(clippy::too_many_arguments)]
    pub fn create_custom_platform(
        &self,
        id: &str,
        name: &str,
        creator_url: &str,
        publish_url: &str,
        title_selector: Option<&str>,
        content_selector: Option<&str>,
        submit_selector: Option<&str>,
        logged_in_pattern: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO custom_platforms
                (id, name, creator_url, publish_url, title_selector,
                 content_selector, submit_selector, logged_in_pattern, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                name,
                creator_url,
                publish_url,
                title_selector,
                content_selector,
                submit_selector,
                logged_in_pattern,
                now
            ],
        )?;
        Ok(())
    }

    /// 删除自定义平台
    pub fn delete_custom_platform(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM custom_platforms WHERE id = ?1", params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod account_tests {
    use super::*;

    fn db() -> Db {
        let db = Db {
            conn: Mutex::new(Connection::open_in_memory().unwrap()),
        };
        db.migrate().unwrap();
        db
    }

    fn new_account() -> NewAccount {
        serde_json::from_value(serde_json::json!({
            "platform": "douyin", "name": "测试账号"
        }))
        .unwrap()
    }

    fn edit() -> UpdateAccount {
        serde_json::from_value(serde_json::json!({
            "name": "修改后的名称", "username": ""
        }))
        .unwrap()
    }

    #[test]
    fn creates_account_without_credentials_and_reads_nullable_secrets() {
        let db = db();
        let account = db.create_account(&new_account()).unwrap();
        assert_eq!(account.username, "");
        assert_eq!(db.get_account_password(&account.id).unwrap(), None);
        assert_eq!(db.get_account_cookies(&account.id).unwrap(), None);
        assert_eq!(db.get_account_password("missing").unwrap(), None);
        assert_eq!(db.get_account_cookies("missing").unwrap(), None);
    }

    #[test]
    fn rejects_blank_name_and_unknown_platform() {
        let db = db();
        let mut input = new_account();
        input.name = "  ".into();
        assert!(db.create_account(&input).is_err());
        input.name = "名称".into();
        input.platform = "unknown".into();
        assert!(db.create_account(&input).is_err());
        assert!(db.list_accounts().unwrap().is_empty());
    }

    #[test]
    fn edits_metadata_preserve_password_platform_and_session_status() {
        let db = db();
        let original = db.create_account(&new_account()).unwrap();
        db.update_account_status(&original.id, AccountStatus::Online).unwrap();
        // 使用不透明字节验证保留行为，不访问操作系统 keyring。
        let opaque_password = vec![1_u8, 2, 3];
        db.conn.lock().unwrap().execute(
            "UPDATE accounts SET password_enc = ?1 WHERE id = ?2",
            params![opaque_password, original.id],
        ).unwrap();
        for password in [None, Some(String::new())] {
            let mut input = edit();
            input.password = password;
            input.group_name = Some("矩阵A组".into());
            let updated = db.update_account(&original.id, &input).unwrap();
            assert_eq!(updated.name, "修改后的名称");
            assert_eq!(updated.platform, original.platform);
            assert_eq!(updated.status, AccountStatus::Online);
            assert_eq!(updated.group_name.as_deref(), Some("矩阵A组"));
            assert_eq!(updated.created_at, original.created_at);
            let stored: Vec<u8> = db.conn.lock().unwrap().query_row(
                "SELECT password_enc FROM accounts WHERE id = ?1",
                params![original.id], |row| row.get(0),
            ).unwrap();
            assert_eq!(stored, opaque_password);
        }
    }

    #[test]
    fn explicit_password_clear_takes_precedence_without_keyring_access() {
        let db = db();
        let account = db.create_account(&new_account()).unwrap();
        db.conn.lock().unwrap().execute(
            "UPDATE accounts SET password_enc = X'010203' WHERE id = ?1",
            params![account.id],
        ).unwrap();
        let mut input = edit();
        input.clear_password = true;
        input.password = Some("ignored when clearing".into());
        db.update_account(&account.id, &input).unwrap();
        assert_eq!(db.get_account_password(&account.id).unwrap(), None);
    }

    #[test]
    fn proxy_change_resets_status_and_clears_optional_fields() {
        let db = db();
        let account = db.create_account(&new_account()).unwrap();
        db.update_account_status(&account.id, AccountStatus::Online).unwrap();
        let mut input = edit();
        input.proxy_url = Some(" socks5://127.0.0.1:1080 ".into());
        let updated = db.update_account(&account.id, &input).unwrap();
        assert_eq!(updated.proxy_url.as_deref(), Some("socks5://127.0.0.1:1080"));
        assert_eq!(updated.status, AccountStatus::Offline);
        assert_eq!(updated.last_check_at, None);
        input.proxy_url = Some("  ".into());
        input.group_name = Some("  ".into());
        let updated = db.update_account(&account.id, &input).unwrap();
        assert_eq!(updated.proxy_url, None);
        assert_eq!(updated.group_name, None);
    }

    #[test]
    fn update_rejects_blank_name_and_missing_account() {
        let db = db();
        let account = db.create_account(&new_account()).unwrap();
        let mut input = edit();
        input.name = "  ".into();
        assert!(db.update_account(&account.id, &input).is_err());
        assert_eq!(db.list_accounts().unwrap()[0].name, account.name);
        assert!(db.update_account("missing", &edit()).is_err());
    }

    #[test]
    fn publish_task_persists_extended_fields() {
        let db = db();
        let account = db.create_account(&new_account()).unwrap();
        // 计划原文这里写的是 1_800_000_000_000（毫秒），但 scheduled_at 全链路
        // 统一为秒且不做换算，故用秒值；断言与计划一致（1_800_000_000）。
        let task = db.create_task(
            &account.id,
            Some("标题"),
            Some("正文"),
            None,
            None,
            Some(1_800_000_000),
            Some("/tmp/cover.png"),
            Some("话题A,话题B"),
            Some("第一段||第二段"),
        ).unwrap();

        let loaded = db.list_tasks(&account.id).unwrap()
            .into_iter().find(|t| t.id == task.id).expect("任务应存在");
        assert_eq!(loaded.cover_path.as_deref(), Some("/tmp/cover.png"));
        assert_eq!(loaded.topics.as_deref(), Some("话题A,话题B"));
        assert_eq!(loaded.thread_segments.as_deref(), Some("第一段||第二段"));
        assert_eq!(loaded.scheduled_at, Some(1_800_000_000));
    }

    // 不触碰系统钥匙串：未配置令牌时读写路径都不需要加解密，可安全进默认测试。
    #[test]
    fn api_token_absent_returns_none_without_keyring() {
        let db = db();
        let account = db.create_account(&new_account()).unwrap();
        assert_eq!(db.get_api_token(&account.id).unwrap(), None);
        assert_eq!(db.get_api_token("missing").unwrap(), None);
    }

    // 该测试会经由 crypto::encrypt/decrypt → 系统钥匙串（keyring），与
    // crypto::tests::test_encrypt_decrypt_roundtrip 是同一条会阻塞等待授权的路径。
    // 故标记 #[ignore]，CI 的 `cargo test --lib` 不会执行它（编译仍会校验签名）。
    // 本地手动运行：cargo test --lib -- --ignored api_token_round_trips_encrypted
    #[ignore = "依赖系统钥匙串，只在本地手动运行"]
    #[test]
    fn api_token_round_trips_encrypted() {
        let db = db();
        let account = db.create_account(&new_account()).unwrap();
        db.save_api_token(&account.id, "secret-token-123").unwrap();
        assert_eq!(
            db.get_api_token(&account.id).unwrap().as_deref(),
            Some("secret-token-123")
        );

        // 落库必须是密文
        let raw: Vec<u8> = {
            let conn = db.conn.lock().unwrap();
            conn.query_row(
                "SELECT api_token_enc FROM accounts WHERE id = ?1",
                params![account.id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert!(
            !raw.windows(16).any(|w| w == b"secret-token-123"),
            "令牌不得以明文落库"
        );
    }
}
