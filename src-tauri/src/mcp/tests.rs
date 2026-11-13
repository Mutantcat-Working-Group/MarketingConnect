// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
use super::{config::McpConfig, protocol, tools};

#[test]
fn defaults_are_opt_in() {
    let config = McpConfig::default();
    assert!(!config.enabled);
    assert!(!config.allow_write);
    assert_eq!(config.url(), "http://127.0.0.1:8931/mcp");
}

#[test]
fn registry_has_unique_tools_and_schemas() {
    let definitions = tools::definitions(&McpConfig::default());
    // 不写死数量：断言具体个数只会让每次新增工具都要改测试，
    // 却证明不了任何东西。这里断言真正的不变量——工具集合非空、
    // 名字唯一、schema 合法，以及关键工具确实注册了。
    assert!(!definitions.is_empty());
    let mut names = std::collections::HashSet::new();
    for tool in definitions {
        assert!(names.insert(tool["name"].as_str().unwrap().to_owned()));
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
    for required in [
        "list_accounts",
        "open_account_session",
        "open_manual_window",
        "check_account_session",
        "batch_publish",
        "start_engine",
        "stop_engine",
    ] {
        assert!(names.contains(required), "缺少必需工具: {required}");
    }
}

#[test]
fn protocol_negotiation_has_fallback() {
    assert_eq!(protocol::negotiate_protocol_version(Some("2025-03-26")), "2025-03-26");
    assert_eq!(protocol::negotiate_protocol_version(Some("unknown")), protocol::PREFERRED_PROTOCOL_VERSION);
}
