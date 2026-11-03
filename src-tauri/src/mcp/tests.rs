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
    assert_eq!(definitions.len(), 12);
    let mut names = std::collections::HashSet::new();
    for tool in definitions {
        assert!(names.insert(tool["name"].as_str().unwrap().to_owned()));
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
}

#[test]
fn protocol_negotiation_has_fallback() {
    assert_eq!(protocol::negotiate_protocol_version(Some("2025-03-26")), "2025-03-26");
    assert_eq!(protocol::negotiate_protocol_version(Some("unknown")), protocol::PREFERRED_PROTOCOL_VERSION);
}
