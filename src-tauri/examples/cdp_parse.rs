// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 定位引擎发出的 Network 事件与 chromiumoxide 强类型之间的字段差异。
//! 外层 untagged enum 的报错会掩盖真实原因，这里直接反序列化到具体类型，
//! 让 serde 明确说出缺哪个字段。
//!
//! 用法: cargo run --example cdp_parse -- <jsonl 文件>

use chromiumoxide::cdp::browser_protocol::network::{
    EventRequestWillBeSent, EventResponseReceived,
};
use serde_json::Value;
use std::collections::BTreeMap;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("需要 jsonl 文件路径");
    let text = std::fs::read_to_string(&path)?;

    let mut stats: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut ok = BTreeMap::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(envelope) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(method) = envelope.get("method").and_then(Value::as_str) else {
            continue;
        };
        let Some(params) = envelope.get("params").cloned() else {
            continue;
        };

        let outcome = match method {
            "Network.requestWillBeSent" => serde_json::from_value::<EventRequestWillBeSent>(params)
                .err()
                .map(|e| e.to_string()),
            "Network.responseReceived" => serde_json::from_value::<EventResponseReceived>(params)
                .err()
                .map(|e| e.to_string()),
            _ => continue,
        };

        match outcome {
            Some(error) => {
                let entry = stats
                    .entry(method.to_string())
                    .or_insert((0, error.clone()));
                entry.0 += 1;
            }
            None => {
                *ok.entry(method.to_string()).or_insert(0usize) += 1;
            }
        }
    }

    println!("=== 反序列化成功 ===");
    for (m, n) in &ok {
        println!("  {n:>5}  {m}");
    }
    if ok.is_empty() {
        println!("  （无）");
    }

    println!("\n=== 反序列化失败 ===");
    if stats.is_empty() {
        println!("  （无）");
    }
    for (m, (n, error)) in &stats {
        println!("  {n:>5}  {m}");
        println!("         原因: {error}");
    }
    Ok(())
}
