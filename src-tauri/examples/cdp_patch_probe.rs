// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 自动探测引擎 Network 事件缺失的全部必填字段。
//!
//! serde 每次只报第一个缺失字段，且同名字段可能位于不同层级
//! （例如 `encodedDataLength` 属于 response 而非事件顶层），
//! 因此这里对每个字段维护候选注入位置，逐个尝试直到解析成功。
//!
//! 用法: cargo run --example cdp_patch_probe -- <jsonl 文件>

use chromiumoxide::cdp::browser_protocol::network::{
    EventRequestWillBeSent, EventResponseReceived,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// 字段名 → 候选注入位置（按顺序尝试）
fn candidates(field: &str) -> Vec<&'static str> {
    match field {
        "initialPriority" => vec!["request"],
        "referrerPolicy" => vec!["request"],
        "charset" | "mimeType" | "connectionReused" | "connectionId" | "securityState" => {
            vec!["response"]
        }
        "encodedDataLength" => vec!["response", ""],
        "redirectHasExtraInfo" | "hasExtraInfo" => vec![""],
        "type" => vec!["initiator"],
        _ => vec![],
    }
}

fn default_value(field: &str) -> Value {
    match field {
        "initialPriority" => json!("High"),
        "referrerPolicy" => json!("no-referrer-when-downgrade"),
        "charset" | "mimeType" => json!(""),
        "connectionReused" => json!(false),
        "connectionId" | "encodedDataLength" => json!(0),
        "securityState" => json!("neutral"),
        "redirectHasExtraInfo" | "hasExtraInfo" => json!(false),
        "type" => json!("other"),
        _ => json!(null),
    }
}

fn missing_field(error: &str) -> Option<String> {
    let start = error.find("missing field `")? + "missing field `".len();
    let rest = &error[start..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

fn try_parse(method: &str, params: &Value) -> Option<String> {
    let error = if method == "Network.requestWillBeSent" {
        serde_json::from_value::<EventRequestWillBeSent>(params.clone()).err()
    } else {
        serde_json::from_value::<EventResponseReceived>(params.clone()).err()
    };
    error.map(|e| e.to_string())
}

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("需要 jsonl 文件路径");
    let text = std::fs::read_to_string(&path)?;

    let mut needed: BTreeSet<(String, String, String)> = BTreeSet::new();
    let mut problems: BTreeSet<String> = BTreeSet::new();
    let mut samples = 0usize;

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
        if method != "Network.requestWillBeSent" && method != "Network.responseReceived" {
            continue;
        }
        let Some(mut params) = envelope.get("params").cloned() else {
            continue;
        };
        samples += 1;

        // 已尝试过的 (字段, 位置) 组合
        let mut tried: BTreeSet<(String, String)> = BTreeSet::new();
        let mut solved = false;

        for _ in 0..40 {
            let Some(error) = try_parse(method, &params) else {
                solved = true;
                break;
            };
            let Some(field) = missing_field(&error) else {
                problems.insert(format!("{method}: {error}"));
                break;
            };
            let options = candidates(&field);
            let pick = options
                .iter()
                .find(|location| !tried.contains(&(field.clone(), location.to_string())));
            match pick {
                Some(location) => {
                    tried.insert((field.clone(), location.to_string()));
                    needed.insert((method.to_string(), field.clone(), location.to_string()));
                    if location.is_empty() {
                        params[field.as_str()] = default_value(&field);
                    } else if let Some(target) = params.get_mut(*location) {
                        target[field.as_str()] = default_value(&field);
                    }
                }
                None => {
                    problems.insert(format!("{method}: 无法定位字段 `{field}`（{error}）"));
                    break;
                }
            }
        }

        if !solved && problems.is_empty() {
            problems.insert(format!("{method}: 达到尝试上限仍未解析成功"));
        }
    }

    println!("扫描样本 {samples} 条\n");
    println!("=== 需要注入的字段清单 ===");
    if needed.is_empty() {
        println!("  （无）");
    }
    for (method, field, location) in &needed {
        let where_ = if location.is_empty() {
            "事件顶层"
        } else {
            location.as_str()
        };
        println!("  {method}\n      {where_}.{field}");
    }

    println!("\n=== 未能解决 ===");
    if problems.is_empty() {
        println!("  （无）");
    }
    for item in &problems {
        println!("  {item}");
    }
    Ok(())
}
