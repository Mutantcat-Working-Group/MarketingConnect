// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! CDP 事件修补中继
//!
//! 引擎发来的 `Network.requestWillBeSent` / `Network.responseReceived` 事件
//! 缺少若干 CDP 规范要求的必填字段。这不是 Obscura 独有的问题：在 Chrome for
//! Testing 157 上实测同样会收到缺字段的 `Network.requestWillBeSentExtraInfo`
//! （缺 `params.clientSecurityState`）与 `Page.frameRequestedNavigation`
//! （`reason` 取值不在枚举内）。因此本模块对默认的真实 Chrome 引擎**同样必需**，
//! 不是切换到 Chrome 之后就能删掉的包袱。chromiumoxide 的事件类型是强类型的，
//! 且对「已知方法 + 字段不符」不容错（未知方法才有兜底），因此任何一条这样
//! 的事件都会让整个 CDP handler 退出、连接中断——表现为打开会话时
//! `data did not match any variant of untagged enum Message` 后
//! "oneshot canceled"。
//!
//! 这里在客户端与引擎之间放一个只监听回环地址的中继：逐条转发消息，
//! 仅对这两个事件补齐缺失字段（已存在则原样透传，方便引擎日后自行修复）。
//!
//! 通过 `cargo run --example cdp_patch_probe` 可重新探测引擎缺哪些字段。

use anyhow::{Context, Result};
use chromiumoxide::cdp::events::CdpEventMessage;
use chromiumoxide::types::Message as CdpMessage;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

/// 每个引擎实例对应一个中继端口
#[derive(Default)]
pub struct CdpRelay {
    /// 引擎端口 → 中继端口
    ports: Mutex<HashMap<u16, u16>>,
}

impl CdpRelay {
    /// 返回该引擎端口对应的中继端口，必要时先启动中继
    pub async fn port_for(&self, engine_port: u16) -> Result<u16> {
        let mut ports = self.ports.lock().await;
        if let Some(relay_port) = ports.get(&engine_port) {
            return Ok(*relay_port);
        }

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("启动 CDP 中继失败（无法绑定回环端口）")?;
        let relay_port = listener.local_addr()?.port();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        tokio::spawn(async move {
                            if let Err(error) = relay(stream, engine_port).await {
                                debug!("CDP 中继连接结束（引擎端口 {}）: {}", engine_port, error);
                            }
                        });
                    }
                    Err(error) => {
                        warn!("CDP 中继停止接受连接（引擎端口 {}）: {}", engine_port, error);
                        break;
                    }
                }
            }
        });

        info!("CDP 中继就绪：引擎端口 {} → 中继端口 {}", engine_port, relay_port);
        ports.insert(engine_port, relay_port);
        Ok(relay_port)
    }
}

/// 解析引擎的 CDP WebSocket 地址。
///
/// obscura 固定暴露 `/devtools/browser`，而真正的 Chromium 需要在路径里带上
/// 启动时生成的 UUID（只能从 `/json/version` 拿到）。这里优先问
/// `/json/version`，失败时回退到 obscura 的固定路径。
async fn upstream_ws_url(engine_port: u16) -> String {
    let version_url = format!("http://127.0.0.1:{}/json/version", engine_port);
    if let Ok(response) = reqwest::get(&version_url).await {
        if let Ok(body) = response.json::<serde_json::Value>().await {
            if let Some(url) = body.get("webSocketDebuggerUrl").and_then(|v| v.as_str()) {
                if !url.is_empty() {
                    return url.to_string();
                }
            }
        }
    }
    format!("ws://127.0.0.1:{}/devtools/browser", engine_port)
}

/// 单个客户端连接的中继任务
async fn relay(client: TcpStream, engine_port: u16) -> Result<()> {
    let client_ws = tokio_tungstenite::accept_async(client)
        .await
        .context("CDP 中继握手失败")?;

    let upstream_url = upstream_ws_url(engine_port).await;
    let (upstream_ws, _) = tokio_tungstenite::connect_async(upstream_url.as_str())
        .await
        .context("CDP 中继连接引擎失败")?;

    let (mut client_tx, mut client_rx) = client_ws.split();
    let (mut upstream_tx, mut upstream_rx) = upstream_ws.split();

    // 客户端 → 引擎：原样转发
    let to_engine = async {
        while let Some(message) = client_rx.next().await {
            let message = message?;
            let closing = message.is_close();
            upstream_tx.send(message).await?;
            if closing {
                break;
            }
        }
        Ok::<(), anyhow::Error>(())
    };

    // 引擎 → 客户端：按需修补
    let to_client = async {
        while let Some(message) = upstream_rx.next().await {
            let message = message?;
            let closing = message.is_close();
            let outgoing = match &message {
                Message::Text(text) => match normalize(text.as_str()) {
                    RelayAction::Replace(patched) => Some(Message::Text(patched.into())),
                    RelayAction::Passthrough => Some(message),
                    RelayAction::Drop => None,
                },
                _ => Some(message),
            };
            if let Some(outgoing) = outgoing {
                dump_message(&outgoing);
                client_tx.send(outgoing).await?;
            }
            if closing {
                break;
            }
        }
        Ok::<(), anyhow::Error>(())
    };

    tokio::select! {
        result = to_engine => result.context("转发到引擎失败")?,
        result = to_client => result.context("转发到客户端失败")?,
    }
    Ok(())
}

/// 中继对一条消息应采取的动作
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayAction {
    /// 原样转发
    Passthrough,
    /// 用修补后的内容替换
    Replace(String),
    /// 丢弃：事件无法被驱动库解析，继续转发会杀掉整条 CDP 连接
    Drop,
}

/// 事件帧能否被驱动库（chromiumoxide）解析
fn parses_as_event(text: &str) -> bool {
    serde_json::from_str::<CdpMessage<CdpEventMessage>>(text).is_ok()
}

/// 同一种事件可能每秒出现上百次，避免告警刷屏
fn warn_dropped_once(method: &str) {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let seen = SEEN.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    let mut guard = seen.lock().unwrap();
    if guard.insert(method.to_string()) {
        tracing::warn!(
            "丢弃无法解析的 CDP 事件（方法 {}）：继续转发会断开整条连接",
            method
        );
    }
}

/// 规范化一条中继消息
pub fn normalize(text: &str) -> RelayAction {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return RelayAction::Passthrough;
    };
    // 响应帧（带 id）永不丢弃，否则调用方会永久等待
    if value.get("method").and_then(Value::as_str).is_none() {
        return RelayAction::Passthrough;
    }
    let patched = patch_event(text);
    let candidate = patched.as_deref().unwrap_or(text);
    if parses_as_event(candidate) {
        return match patched {
            Some(patched) => RelayAction::Replace(patched),
            None => RelayAction::Passthrough,
        };
    }
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("<未知>");
    warn_dropped_once(method);
    RelayAction::Drop
}

/// 修补引擎缺失的必填字段；返回 `None` 表示无需修改（原样透传）
pub fn patch_event(text: &str) -> Option<String> {
    let mut value: Value = serde_json::from_str(text).ok()?;
    let method = value.get("method").and_then(Value::as_str)?;

    let changed = match method {
        "Network.requestWillBeSent" => patch_request_will_be_sent(&mut value),
        "Network.responseReceived" => patch_response_received(&mut value),
        // 以下是新版 Chromium 与 chromiumoxide 0.7 的 CDP 版本差，
        // 不改会让整个 CDP 连接因反序列化失败而断开。
        "Network.requestWillBeSentExtraInfo" => patch_request_will_be_sent_extra_info(&mut value),
        "Page.frameRequestedNavigation" => patch_frame_requested_navigation(&mut value),
        _ => false,
    };
    if !changed {
        return None;
    }
    serde_json::to_string(&value).ok()
}

fn patch_request_will_be_sent(value: &mut Value) -> bool {
    let mut changed = false;
    if let Some(params) = value.get_mut("params") {
        if let Some(request) = params.get_mut("request").and_then(Value::as_object_mut) {
            changed |= insert(request, "initialPriority", json!("High"));
            changed |= insert(request, "referrerPolicy", json!("no-referrer-when-downgrade"));
        }
        if let Some(object) = params.as_object_mut() {
            changed |= insert(object, "redirectHasExtraInfo", json!(false));
        }
    }
    changed
}

fn patch_response_received(value: &mut Value) -> bool {
    let mut changed = false;
    if let Some(params) = value.get_mut("params") {
        if let Some(response) = params.get_mut("response").and_then(Value::as_object_mut) {
            changed |= insert(response, "charset", json!(""));
            changed |= insert(response, "connectionReused", json!(false));
            changed |= insert(response, "connectionId", json!(0));
            // 这两个属于 response 内部，事件层没有同名字段
            changed |= insert(response, "encodedDataLength", json!(0));
            changed |= insert(response, "securityState", json!("neutral"));
        }
        if let Some(object) = params.as_object_mut() {
            changed |= insert(object, "hasExtraInfo", json!(false));
        }
    }
    changed
}

/// 新版 Chromium 的 `clientSecurityState` 用 `initiatorIPAddressSpace: "Loopback"`
/// 和改名后的 `localNetworkAccessRequestPolicy`，chromiumoxide 0.7 的
/// `IpAddressSpace` 没有 `Loopback`，且仍要求 `privateNetworkRequestPolicy`，
/// 于是整个事件解析失败。该字段在 chromiumoxide 事件结构里是 `Option`，
/// 直接摘掉即可（本应用与 chromiumoxide 都不读它）。
fn patch_request_will_be_sent_extra_info(value: &mut Value) -> bool {
    let Some(params) = value.get_mut("params").and_then(Value::as_object_mut) else {
        return false;
    };
    params.remove("clientSecurityState").is_some()
}

/// chromiumoxide 0.7 的 `ClientNavigationReason` 只有 8 个旧取值，
/// 新版 Chromium 会发 `initialFrameNavigation` 等新原因。`reason` 是必填枚举，
/// 缺失或无法识别都会让事件解析失败，因此把未知取值归一到 `scriptInitiated`。
fn patch_frame_requested_navigation(value: &mut Value) -> bool {
    const KNOWN: [&str; 8] = [
        "formSubmissionGet",
        "formSubmissionPost",
        "httpHeaderRefresh",
        "scriptInitiated",
        "metaTagRefresh",
        "pageBlockInterstitial",
        "reload",
        "anchorClick",
    ];
    let Some(params) = value.get_mut("params").and_then(Value::as_object_mut) else {
        return false;
    };
    let current = params.get("reason").and_then(Value::as_str);
    if current.is_some_and(|reason| KNOWN.contains(&reason)) {
        return false;
    }
    params.insert("reason".to_string(), json!("scriptInitiated"));
    true
}

/// 仅在字段缺失时写入；已存在则不动（引擎修好后自动透传）
fn insert(object: &mut serde_json::Map<String, Value>, key: &str, value: Value) -> bool {
    if object.contains_key(key) {
        return false;
    }
    object.insert(key.to_string(), value);
    true
}

/// 诊断开关：设置 `MC_CDP_DUMP=<路径>` 可把经过中继的消息逐条落盘，
/// 用于再次出现「消息无法解析」时定位是哪一类事件。
/// 注意：文件中包含 CDP 原始流量（可能含 cookie），排查完请删除。
fn dump_message(message: &Message) {
    use std::io::Write;
    use std::sync::OnceLock;

    static PATH: OnceLock<Option<String>> = OnceLock::new();
    let Some(path) = PATH
        .get_or_init(|| std::env::var("MC_CDP_DUMP").ok())
        .as_deref()
    else {
        return;
    };
    let Message::Text(text) = message else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{}", text.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(method: &str, params: Value) -> String {
        json!({"method": method, "params": params, "sessionId": "s1"}).to_string()
    }

    #[test]
    fn fills_missing_required_fields() {
        let raw = envelope(
            "Network.requestWillBeSent",
            json!({
                "requestId": "r1",
                "request": {"url": "https://a.test/", "method": "GET", "headers": {}},
                "type": "Document"
            }),
        );
        let patched = patch_event(&raw).expect("应当发生修补");
        let value: Value = serde_json::from_str(&patched).unwrap();
        assert_eq!(value["params"]["request"]["initialPriority"], json!("High"));
        assert_eq!(
            value["params"]["request"]["referrerPolicy"],
            json!("no-referrer-when-downgrade")
        );
        assert_eq!(value["params"]["redirectHasExtraInfo"], json!(false));
        // 原有字段必须保留
        assert_eq!(value["params"]["requestId"], json!("r1"));
        assert_eq!(value["sessionId"], json!("s1"));
    }

    #[test]
    fn fills_response_fields() {
        let raw = envelope(
            "Network.responseReceived",
            json!({
                "requestId": "r1",
                "response": {"url": "https://a.test/", "status": 200, "headers": {}},
                "type": "Document"
            }),
        );
        let patched = patch_event(&raw).expect("应当发生修补");
        let value: Value = serde_json::from_str(&patched).unwrap();
        assert_eq!(value["params"]["response"]["charset"], json!(""));
        assert_eq!(value["params"]["response"]["connectionReused"], json!(false));
        assert_eq!(value["params"]["response"]["connectionId"], json!(0));
        assert_eq!(value["params"]["response"]["encodedDataLength"], json!(0));
        assert_eq!(value["params"]["response"]["securityState"], json!("neutral"));
        assert_eq!(value["params"]["hasExtraInfo"], json!(false));
    }

    #[test]
    fn leaves_complete_events_untouched() {
        let raw = envelope(
            "Network.requestWillBeSent",
            json!({
                "redirectHasExtraInfo": true,
                "request": {
                    "url": "https://a.test/",
                    "method": "GET",
                    "headers": {},
                    "initialPriority": "Low",
                    "referrerPolicy": "origin"
                }
            }),
        );
        assert!(patch_event(&raw).is_none(), "字段齐全时不应改写");
    }

    #[test]
    fn strips_new_client_security_state_shape() {
        // Chrome 151 的形状：IpAddressSpace 多了 Loopback，
        // 且 privateNetworkRequestPolicy 已改名为 localNetworkAccessRequestPolicy
        let raw = envelope(
            "Network.requestWillBeSentExtraInfo",
            json!({
                "requestId": "r1",
                "associatedCookies": [],
                "headers": {":method": "GET"},
                "connectTiming": {"requestTime": 1.5},
                "clientSecurityState": {
                    "initiatorIsSecureContext": true,
                    "initiatorIPAddressSpace": "Loopback",
                    "localNetworkAccessRequestPolicy": "PermissionBlock"
                },
                "siteHasCookieInOtherPartition": false
            }),
        );
        let patched = patch_event(&raw).expect("应当摘掉 clientSecurityState");
        let value: Value = serde_json::from_str(&patched).unwrap();
        assert!(value["params"].get("clientSecurityState").is_none());
        // 其余字段必须原样保留
        assert_eq!(value["params"]["requestId"], json!("r1"));
        assert_eq!(value["params"]["connectTiming"]["requestTime"], json!(1.5));
        assert_eq!(value["params"]["siteHasCookieInOtherPartition"], json!(false));
    }

    #[test]
    fn normalizes_unknown_navigation_reason() {
        let raw = envelope(
            "Page.frameRequestedNavigation",
            json!({
                "frameId": "F1",
                "reason": "initialFrameNavigation",
                "url": "https://a.test/",
                "disposition": "currentTab"
            }),
        );
        let patched = patch_event(&raw).expect("未知 reason 应当被归一");
        let value: Value = serde_json::from_str(&patched).unwrap();
        assert_eq!(value["params"]["reason"], json!("scriptInitiated"));
        // 未知取值归一后必须是 chromiumoxide 认识的枚举值
        assert_eq!(value["params"]["url"], json!("https://a.test/"));
        assert_eq!(value["params"]["disposition"], json!("currentTab"));
    }

    #[test]
    fn keeps_known_navigation_reason() {
        let raw = envelope(
            "Page.frameRequestedNavigation",
            json!({"frameId": "F1", "reason": "anchorClick", "url": "https://a.test/",
                   "disposition": "currentTab"}),
        );
        assert!(patch_event(&raw).is_none(), "已知 reason 不应改写");
    }

    #[test]
    fn ignores_unrelated_messages() {
        for method in [
            "Network.loadingFinished",
            "Page.lifecycleEvent",
            "Target.attachedToTarget",
        ] {
            let raw = envelope(method, json!({"requestId": "r1"}));
            assert!(patch_event(&raw).is_none(), "{method} 不应被改写");
        }
        assert!(patch_event("{\"id\":1,\"result\":{}}").is_none());
        assert!(patch_event("not json").is_none());
    }

    #[test]
    fn unknown_reason_is_normalized_not_dropped() {
        // 157 实测：initialFrameNavigation 是 chromiumoxide 0.7 不认识的 reason 取值
        let raw = r#"{"method":"Page.frameRequestedNavigation","params":{"frameId":"F1","reason":"initialFrameNavigation","url":"https://a.test/","disposition":"currentTab"}}"#;
        assert!(
            matches!(normalize(raw), RelayAction::Replace(_)),
            "未知 reason 应被归一，而不是丢弃"
        );
    }

    #[test]
    fn strips_client_security_state_from_157_shape() {
        // 157 实测的真实形状：同时带有 siteHasCookieInOtherPartition
        let raw = r#"{"method":"Network.requestWillBeSentExtraInfo","params":{"requestId":"r1","associatedCookies":[],"headers":{},"connectTiming":{"requestTime":1.0},"clientSecurityState":{"initiatorIsSecureContext":true,"initiatorIPAddressSpace":"Public","privateNetworkRequestPolicy":"Allow"},"siteHasCookieInOtherPartition":false}}"#;
        let RelayAction::Replace(patched) = normalize(raw) else {
            panic!("应发生修补，实际为 {:?}", normalize(raw));
        };
        assert!(!patched.contains("clientSecurityState"), "元凶字段必须被摘掉");
        assert!(
            patched.contains("siteHasCookieInOtherPartition"),
            "不相关字段不应被动到"
        );
    }

    #[test]
    fn drops_event_that_patching_cannot_fix() {
        // status 是类型错误，任何补丁都救不回来：必须丢弃而不是原样转发
        let raw = r#"{"method":"Network.responseReceived","params":{"requestId":"r1","type":"Document","response":{"url":"https://a.test/","status":"not-a-number","headers":{}},"hasExtraInfo":false}}"#;
        assert_eq!(normalize(raw), RelayAction::Drop);
    }

    #[test]
    fn never_drops_responses() {
        // 响应帧若被丢弃，调用方会永久等待
        let raw = r#"{"id":1,"result":{"something":"unexpected","nested":{"x":[]}}}"#;
        assert_eq!(normalize(raw), RelayAction::Passthrough);
    }

    #[test]
    fn passthrough_when_no_patch_needed() {
        let raw = r#"{"method":"Page.frameRequestedNavigation","params":{"frameId":"F1","reason":"scriptInitiated","url":"https://a.test/","disposition":"currentTab"}}"#;
        assert_eq!(normalize(raw), RelayAction::Passthrough);
    }

    #[test]
    fn non_json_text_is_passed_through() {
        assert_eq!(normalize("not json"), RelayAction::Passthrough);
    }
}
