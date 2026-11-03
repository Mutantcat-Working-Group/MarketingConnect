// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! Versioned plaintext representation stored only inside the encrypted cookies_enc payload.

use anyhow::{Result, anyhow};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

// Deliberately no Debug: these values contain credentials.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Credentials {
    pub version: u8,
    pub cookies: Vec<Value>,
    pub origin: Option<String>,
    #[serde(default)]
    pub local_storage: BTreeMap<String, String>,
    #[serde(default)]
    pub session_storage: BTreeMap<String, String>,
    pub url: Option<String>,
}

impl Credentials {
    pub fn new(cookies: Vec<Value>) -> Self {
        Self {
            version: 1,
            cookies,
            origin: None,
            local_storage: BTreeMap::new(),
            session_storage: BTreeMap::new(),
            url: None,
        }
    }

    pub fn parse(json: &str) -> Result<Self> {
        // Never retain serde's error: it may include cookie/storage values.
        let value: Value = serde_json::from_str(json)
            .map_err(|_| anyhow!("凭据数据格式无效"))?;
        let credentials = if value.is_array() {
            Self::new(serde_json::from_value(value)
                .map_err(|_| anyhow!("凭据数据格式无效"))?)
        } else {
            serde_json::from_value::<Self>(value)
                .map_err(|_| anyhow!("凭据数据格式无效"))?
        };
        if credentials.version != 1 {
            return Err(anyhow!("不支持的凭据版本"));
        }
        Ok(credentials)
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|_| anyhow!("凭据序列化失败"))
    }

    pub fn storage_matches(&self, trusted_origin: &str) -> bool {
        self.origin.as_deref() == Some(trusted_origin)
            && http_origin(trusted_origin).as_deref() == Some(trusted_origin)
    }
}

/// Exact scheme/host/port boundary; never trust a saved URL as a navigation target.
pub(crate) fn http_origin(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

/// Keep CDP attributes (including camelCase flags), but omit session expiration.
pub(crate) fn cookie_param(cookie: &Value) -> Result<CookieParam> {
    let mut value = cookie.clone();
    let object = value.as_object_mut().ok_or_else(|| anyhow!("cookie 格式无效"))?;
    if object.get("session").and_then(Value::as_bool) == Some(true)
        || object.get("expires").and_then(Value::as_f64).is_some_and(|v| v < 0.0)
    {
        object.remove("expires");
    }
    // Older versions accepted snake_case http_only; CDP exports httpOnly.
    if !object.contains_key("httpOnly") {
        if let Some(value) = object.remove("http_only") {
            object.insert("httpOnly".into(), value);
        }
    }
    let mut param: CookieParam = serde_json::from_value(value)
        .map_err(|_| anyhow!("cookie 格式无效"))?;
    let domain = param.domain.as_deref().ok_or_else(|| anyhow!("cookie 域无效"))?;
    let host = domain.trim_start_matches('.');
    if host.is_empty() {
        return Err(anyhow!("cookie 域无效"));
    }
    let url = format!("{}://{}/", if param.secure.unwrap_or(false) { "https" } else { "http" }, host);
    let parsed = reqwest::Url::parse(&url).map_err(|_| anyhow!("cookie 域无效"))?;
    if parsed.host_str() != Some(host) || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(anyhow!("cookie 域无效"));
    }
    param.url = Some(url);
    // A leading dot is CDP's domain-cookie marker. Otherwise preserve host-only scope.
    if !domain.starts_with('.') {
        param.domain = None;
    }
    Ok(param)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_cookie_array_remains_readable() {
        let saved = Credentials::parse(r#"[{"name":"sid","value":"secret","domain":".example.com"}]"#).unwrap();
        assert_eq!(saved.version, 1);
        assert_eq!(saved.cookies.len(), 1);
        assert!(saved.origin.is_none());
        assert!(saved.local_storage.is_empty());
    }

    #[test]
    fn version_one_round_trip_keeps_both_storage_types() {
        let mut saved = Credentials::new(vec![]);
        saved.origin = Some("https://example.com".into());
        saved.url = Some("https://example.com/creator".into());
        saved.local_storage.insert("token".into(), "secret".into());
        saved.session_storage.insert("session".into(), "value".into());
        let restored = Credentials::parse(&saved.to_json().unwrap()).unwrap();
        assert_eq!(restored.local_storage.get("token").map(String::as_str), Some("secret"));
        assert_eq!(restored.session_storage.get("session").map(String::as_str), Some("value"));
        assert!(restored.storage_matches("https://example.com"));
        assert!(!restored.storage_matches("https://example.com.evil.test"));
        assert!(!restored.storage_matches("http://example.com"));
        assert!(!restored.storage_matches("https://example.com:8443"));
    }

    #[test]
    fn malformed_and_future_payloads_have_redacted_errors() {
        for input in [r#"{"version":2,"cookies":[]}"#, r#"{"version":"secret","cookies":[]}"#, "secret", "null"] {
            let error = Credentials::parse(input).err().unwrap().to_string();
            assert!(!error.contains("secret"));
        }
    }

    #[test]
    fn cookie_flags_domain_and_expiration_survive() {
        let param = cookie_param(&json!({"name":"sid", "value":"secret", "domain":".example.com", "path":"/creator", "httpOnly":true, "secure":true, "sameSite":"None", "session":false, "expires":2000000000.0})).unwrap();
        assert_eq!(param.domain.as_deref(), Some(".example.com"));
        assert_eq!(param.http_only, Some(true));
        assert_eq!(param.secure, Some(true));
        assert!(param.same_site.is_some());
        assert!(param.expires.is_some());
    }

    #[test]
    fn session_cookie_and_host_only_scope_are_preserved() {
        let param = cookie_param(&json!({"name":"sid", "value":"secret", "domain":"example.com", "http_only":true, "session":true, "expires":-1})).unwrap();
        assert!(param.domain.is_none());
        assert!(param.expires.is_none());
        assert_eq!(param.http_only, Some(true));
        assert_eq!(param.url.as_deref(), Some("http://example.com/"));
    }

    #[test]
    fn expired_persistent_cookie_is_not_converted_to_session_cookie() {
        let param = cookie_param(&json!({"name":"sid", "value":"secret", "domain":"example.com", "session":false, "expires":0})).unwrap();
        assert!(param.expires.is_some());
    }

    #[test]
    fn origins_are_normalized_and_non_web_origins_rejected() {
        assert_eq!(http_origin("https://example.com:443/path?q=1").as_deref(), Some("https://example.com"));
        assert!(http_origin("about:blank").is_none());
        assert!(http_origin("file:///tmp/page.html").is_none());
    }
}
