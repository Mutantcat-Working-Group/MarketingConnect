// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 平台适配器
//!
//! 每个平台实现 PlatformAdapter trait 的三个核心接口：
//! - login: 登录流程（支持帐密自动登录）
//! - publish: 发布内容
//! - check_session: 会话健康检测（掉号判断）
//!
//! 适配器来源：
//! - 内置适配器（抖音 / 小红书 / 公众号 / CSDN / 知乎，静态注册）
//! - 自定义平台（用户在设置中注册，存于 DB，由 CustomPlatformAdapter 驱动）

use crate::db::Db;
use anyhow::Result;
use async_trait::async_trait;

pub(crate) mod auth;
mod csdn;
mod custom;
pub mod devto;
mod douyin;
mod wechat_mp;
mod xiaohongshu;
pub mod select;
pub mod zhihu;
pub use csdn::CsdnAdapter;
pub use custom::{CustomPlatform, CustomPlatformAdapter};
pub use douyin::DouyinAdapter;
// 再导出选择器内核：`click_any` / `fill_any` 已被 csdn / xiaohongshu 等适配器
// 经 `crate::platforms::` 路径使用。`ElementOps` / `press_key` / `upload_any`
// 本身已在 Task 9 接入知乎发布流程（知乎直接走 `super::select::` 路径），
// 但这三个名字的再导出仍无调用方，故保留 allow 以免引入新的编译警告。
#[allow(unused_imports)]
pub use select::{ElementOps, click_any, fill_any, press_key, upload_any};
pub use wechat_mp::WechatMpAdapter;
pub use xiaohongshu::XiaohongshuAdapter;

/// 内置平台 ID 列表
///
/// 这些 ID 为内置适配器保留，自定义平台不可占用，
/// 否则会与内置适配器重名导致查找结果不确定。
pub const BUILTIN_PLATFORM_IDS: &[&str] =
    &["douyin", "xiaohongshu", "wechat_mp", "csdn", "zhihu", "devto"];

/// 判断是否为内置平台 ID
pub fn is_builtin_platform_id(id: &str) -> bool {
    BUILTIN_PLATFORM_IDS.contains(&id)
}

/// 平台支持的能力，驱动前端显示与发布前校验
///
/// 发布前校验已接线（`scheduler::execute_publish` 与 `accounts::batch_publish`），
/// 前端能力 UI 留到 Task 11。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Capabilities {
    /// 自动发布是否已实现。false 表示 publish() 目前只会报错，
    /// 前端应据此提示用户改用「自定义平台」或手动发布。
    ///
    /// 注意：其余 flag 描述的是「平台的 publish() 是否真的读取并处理了
    /// 对应字段」，只有在该适配器真正实现后才可为 true；未实现发布的平台
    /// 必须把它以及其余全部能力位都设为 false，避免误导用户。
    pub publish: bool,
    pub text: bool,
    pub images: bool,
    pub video: bool,
    pub multi_image: bool,
    pub cover: bool,
    pub topics: bool,
    pub thread: bool,
    pub schedule: bool,
}

impl Capabilities {
    /// 仅文本
    // 目前只有本模块的 `#[cfg(test)]` 校验用例引用它，非测试构建下是 dead code；
    // 这里只对这一个常量做精确抑制，不再对整个 `impl` 加 allow。
    #[allow(dead_code)]
    pub const TEXT_ONLY: Self = Self {
        publish: true,
        text: true, images: false, video: false, multi_image: false,
        cover: false, topics: false, thread: false, schedule: false,
    };

    /// 校验内容是否超出平台能力，超出则返回可直接展示给用户的原因
    pub fn validate(&self, content: &PublishContent) -> anyhow::Result<()> {
        // 放在最前面：若自动发布根本未实现，任何字段校验都失去意义，
        // 直接给出最准确的原因，避免报出「不支持封面」之类的误导信息。
        if !self.publish {
            anyhow::bail!(
                "该平台自动发布尚未实现，请在「自定义平台」中配置选择器，或打开会话后手动发布"
            );
        }
        if !self.text && !content.text.trim().is_empty() {
            anyhow::bail!("该平台不支持文本发布");
        }
        if !content.media_paths.is_empty() {
            if !self.images && !self.video {
                anyhow::bail!("该平台不支持媒体文件");
            }
            if content.media_paths.len() > 1 && !self.multi_image {
                anyhow::bail!("该平台不支持一次上传多个文件");
            }
        }
        if content.cover_path.is_some() && !self.cover {
            anyhow::bail!("该平台不支持设置封面");
        }
        if !content.topics.is_empty() && !self.topics {
            anyhow::bail!("该平台不支持话题标签");
        }
        if !content.thread_segments.is_empty() && !self.thread {
            anyhow::bail!("该平台不支持线程发布");
        }
        if content.schedule_at.is_some() && !self.schedule {
            anyhow::bail!("该平台不支持平台内定时发布");
        }
        Ok(())
    }
}

/// 发布内容
#[derive(Debug, Clone)]
pub struct PublishContent {
    pub title: Option<String>,
    pub text: String,
    pub media_paths: Vec<String>,
    /// 封面（视频/文章头图）
    pub cover_path: Option<String>,
    /// 话题标签，不含 `#` 前缀
    pub topics: Vec<String>,
    /// 线程分段；非空时按顺序逐条发布，`text` 作为第一段
    pub thread_segments: Vec<String>,
    /// 平台内定时发布的绝对时间戳，单位**秒**
    ///
    /// 注意区分两个同名概念：
    /// - 数据库 `PublishTask.scheduled_at`：**本应用**何时该发起发布，由
    ///   `scheduler` 轮询判断，**不是**这个字段。
    /// - 本字段：**要求平台在其内部**定时发布。平台不支持（`schedule: false`）
    ///   时必须为 `None`，否则 `validate()` 会拒绝，任务会永久失败。
    ///
    /// 时间戳单位与数据库列（由 `chrono::Utc::now().timestamp()` 写入）一致，
    /// 任何地方都不要换算成毫秒。若某个平台接口要求毫秒，在该适配器内部换算，
    /// 不要改这里的单位。
    pub schedule_at: Option<i64>,
}

/// 平台适配器 trait
#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    /// 平台标识（douyin / xiaohongshu / ...）
    fn platform_id(&self) -> &str;

    /// 平台显示名
    fn display_name(&self) -> &str;

    /// 创作者平台首页 URL
    fn creator_url(&self) -> &str;

    /// 平台支持的能力
    fn capabilities(&self) -> Capabilities;

    /// 登录流程：使用帐密登录，返回登录后的 cookie JSON
    async fn login(&self, page: &chromiumoxide::Page, username: &str, password: &str) -> Result<String>;

    /// 发布内容
    async fn publish(&self, page: &chromiumoxide::Page, content: &PublishContent) -> Result<()>;

    /// 会话检测：返回 true 表示会话有效
    async fn check_session(&self, page: &chromiumoxide::Page) -> Result<bool>;

    /// 令牌型平台的凭据位数说明，用于前端提示（浏览器族返回 None）
    ///
    /// 默认返回 None，表示该平台走浏览器登录而非令牌登录；
    /// 令牌族适配器覆盖它，`save_platform_token` 以此判断平台是否支持令牌。
    fn token_hint(&self) -> Option<&str> {
        None
    }

    /// 校验令牌是否可用（令牌型平台实现；浏览器族返回不支持）
    ///
    /// 默认实现直接报不支持，故抖音/小红书/公众号/CSDN/自定义平台
    /// 都无需改动。
    async fn verify_token(&self, _token: &str) -> Result<()> {
        anyhow::bail!("该平台不支持令牌登录")
    }

    /// 手动发布页 URL
    ///
    /// 「打开手动发布窗口」会用这个地址。默认退回创作者首页：
    /// 抖音 / 公众号没有实现自动发布，但用户在首页点两下就能到发布页，
    /// 比给每个适配器硬编码一个可能过期的发布页地址更稳。
    fn publish_url(&self) -> Option<&str> {
        None
    }

    /// 通过 API 令牌发布（令牌型平台实现）。默认不支持。
    ///
    /// 默认实现直接报不支持，故既有适配器都无需改动；
    /// 调用方（`scheduler::execute_publish` / `accounts::batch_publish`）
    /// 只在 `token_hint()` 非空时才走这里，全程不打开浏览器会话。
    async fn publish_with_token(&self, _token: &str, _content: &PublishContent) -> Result<()> {
        anyhow::bail!("该平台不支持通过 API 令牌发布")
    }
}

/// 获取所有已注册的平台适配器
///
/// db 为 Some 时追加用户注册的自定义平台。
pub fn all_adapters(db: Option<&Db>) -> Vec<Box<dyn PlatformAdapter>> {
    let mut adapters: Vec<Box<dyn PlatformAdapter>> = vec![
        Box::new(douyin::DouyinAdapter::new()),
        Box::new(xiaohongshu::XiaohongshuAdapter::new()),
        Box::new(wechat_mp::WechatMpAdapter::new()),
        Box::new(csdn::CsdnAdapter::new()),
        Box::new(zhihu::ZhihuAdapter::new()),
        Box::new(devto::DevToAdapter::new()),
    ];

    // 追加自定义平台（DB 查询失败时静默跳过）
    if let Some(db) = db {
        if let Ok(platforms) = db.list_custom_platforms() {
            for p in platforms {
                adapters.push(Box::new(custom::CustomPlatformAdapter::new(p)));
            }
        }
    }

    adapters
}

/// 手动发布窗口应打开的地址：优先发布页，否则创作者首页
pub fn publish_page_url(adapter: &dyn PlatformAdapter) -> String {
    adapter
        .publish_url()
        .unwrap_or_else(|| adapter.creator_url())
        .to_string()
}

/// 按平台 ID 查找适配器
pub fn find_adapter(platform_id: &str, db: Option<&Db>) -> Option<Box<dyn PlatformAdapter>> {
    all_adapters(db)
        .into_iter()
        .find(|a| a.platform_id() == platform_id)
}

/// 令牌型账号的会话有效性判定结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSession {
    /// 令牌校验通过，账号在线
    Valid,
    /// 令牌校验失败（掉号），附带失败原因，便于日志与告警
    Invalid(String),
    /// 该账号尚未配置 API 令牌：**无法判定**，不是「掉号」。
    /// 调用方不得据此标记离线或发送掉号告警。
    NotConfigured,
}

/// 令牌型平台的会话有效性：读取账号令牌并调用 `verify_token`
///
/// 只应在 `adapter.token_hint().is_some()` 时调用。浏览器族平台必须继续走
/// 页面探测（`check_session`），不要用本函数。
///
/// 注意：`verify_token` 会发起真实网络请求，测试中不要调用到它。
pub async fn check_token_session(
    db: &Db,
    adapter: &dyn PlatformAdapter,
    account_id: &str,
) -> anyhow::Result<TokenSession> {
    let Some(token) = db.get_api_token(account_id)? else {
        return Ok(TokenSession::NotConfigured);
    };
    match adapter.verify_token(&token).await {
        Ok(()) => Ok(TokenSession::Valid),
        Err(e) => Ok(TokenSession::Invalid(e.to_string())),
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn builtin_adapters_declare_text_capability() {
        for adapter in all_adapters(None) {
            let caps = adapter.capabilities();
            // 只有已实现自动发布的适配器才需要声明文本能力；未实现发布的
            // （douyin / wechat_mp）全部能力位为 false，由
            // `unimplemented_publish_adapters_declare_no_capabilities` 专门守护。
            if caps.publish {
                assert!(caps.text, "{} 已实现自动发布，必须支持文本发布", adapter.platform_id());
            }
        }
    }

    #[test]
    fn validate_rejects_when_publish_is_unimplemented() {
        // 即使其余能力位都声明为 true，publish: false 也必须最先拒绝，
        // 且错误信息要明确说明「尚未实现」，而不是误报某个字段不受支持。
        let caps = Capabilities {
            publish: false,
            text: true, images: true, video: true, multi_image: true,
            cover: true, topics: true, thread: true, schedule: true,
        };
        let content = PublishContent {
            title: Some("标题".into()), text: "正文".into(), media_paths: Vec::new(),
            cover_path: None, topics: Vec::new(), thread_segments: Vec::new(), schedule_at: None,
        };
        let err = caps.validate(&content).unwrap_err().to_string();
        assert!(err.contains("尚未实现"), "错误信息应说明尚未实现，实际: {err}");
    }

    #[test]
    fn unimplemented_publish_adapters_declare_no_capabilities() {
        // 守护测试：douyin / wechat_mp 的 publish() 目前只会 bail!，
        // 因此除 publish 外所有能力位都必须为 false，防止将来只改声明不改实现。
        for id in ["douyin", "wechat_mp"] {
            let adapter = find_adapter(id, None).unwrap_or_else(|| panic!("{id} 内置适配器必须存在"));
            let caps = adapter.capabilities();
            assert!(!caps.publish, "{id} 自动发布尚未实现，publish 必须为 false");
            assert!(!caps.text, "{id} 未实现发布，text 必须为 false");
            assert!(!caps.images, "{id} 未实现发布，images 必须为 false");
            assert!(!caps.video, "{id} 未实现发布，video 必须为 false");
            assert!(!caps.multi_image, "{id} 未实现发布，multi_image 必须为 false");
            assert!(!caps.cover, "{id} 未实现发布，cover 必须为 false");
            assert!(!caps.topics, "{id} 未实现发布，topics 必须为 false");
            assert!(!caps.thread, "{id} 未实现发布，thread 必须为 false");
            assert!(!caps.schedule, "{id} 未实现发布，schedule 必须为 false");
        }
    }

    #[test]
    fn only_devto_takes_the_token_publish_branch() {
        // 两条发布路径都是「先解析 adapter、再按 `token_hint()` 分流」：
        // 只有令牌族平台走 `publish_with_token` 且完全不打开浏览器会话。
        // 这里守护该分类契约，防止新增适配器时误判分流方向。
        for adapter in all_adapters(None) {
            let is_token_branch = adapter.token_hint().is_some();
            assert_eq!(
                is_token_branch,
                adapter.platform_id() == "devto",
                "{} 是否走令牌发布分支与预期不符",
                adapter.platform_id()
            );
        }
    }

    #[test]
    fn zhihu_does_not_declare_unimplemented_cover() {
        // 知乎 publish() 不读取 cover_path，不得声明封面能力。
        let adapter = find_adapter("zhihu", None).expect("知乎内置适配器必须存在");
        assert!(!adapter.capabilities().cover, "知乎尚未实现封面，cover 必须为 false");
    }

    #[test]
    fn validate_rejects_video_when_unsupported() {
        let caps = Capabilities::TEXT_ONLY;
        let content = PublishContent {
            title: None,
            text: "正文".into(),
            media_paths: vec!["/tmp/a.mp4".into()],
            cover_path: None,
            topics: Vec::new(),
            thread_segments: Vec::new(),
            schedule_at: None,
        };
        let err = caps.validate(&content).unwrap_err().to_string();
        assert!(err.contains("不支持"), "错误信息应说明不支持，实际: {err}");
    }

    #[test]
    fn validate_rejects_topics_and_thread_when_unsupported() {
        let caps = Capabilities::TEXT_ONLY;
        let mut content = PublishContent {
            title: None, text: "正文".into(), media_paths: Vec::new(),
            cover_path: None, topics: vec!["话题".into()],
            thread_segments: Vec::new(), schedule_at: None,
        };
        assert!(caps.validate(&content).is_err(), "话题不受支持时必须报错");

        content.topics.clear();
        content.thread_segments = vec!["第二段".into()];
        assert!(caps.validate(&content).is_err(), "线程不受支持时必须报错");
    }

    #[test]
    fn validate_accepts_content_within_capabilities() {
        let caps = Capabilities::TEXT_ONLY;
        let content = PublishContent {
            title: Some("标题".into()), text: "正文".into(), media_paths: Vec::new(),
            cover_path: None, topics: Vec::new(), thread_segments: Vec::new(), schedule_at: None,
        };
        assert!(caps.validate(&content).is_ok());
    }
}

#[cfg(test)]
mod token_session_tests {
    use super::*;

    #[tokio::test]
    async fn unconfigured_token_is_not_reported_as_dropped() {
        // 账号未配置 API 令牌时必须返回 NotConfigured，而不是 Invalid：
        // 否则没有配置令牌的 dev.to 账号会被掉号监控误判为「掉号」并触发告警。
        // 未配置令牌会在调用 verify_token 之前返回，因此本测试不发任何网络请求。
        let db = Db::open(std::path::Path::new(":memory:")).expect("打开内存库失败");
        let adapter = find_adapter("devto", None).expect("devto 适配器必须存在");
        let result = check_token_session(&db, adapter.as_ref(), "no-such-account")
            .await
            .expect("读取未配置令牌不应报错");
        assert_eq!(result, TokenSession::NotConfigured);
    }
}
