// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! Conservative, non-navigating authenticated-page evidence shared by login and monitoring.

use anyhow::{Result, anyhow};
use chromiumoxide::Page;
use reqwest::Url;
use serde::Deserialize;

fn web_url(value: &str) -> Option<Url> {
    let url = Url::parse(value).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()).then_some(url)
}

fn same_origin(current: &str, creator: &str) -> bool {
    match (web_url(current), web_url(creator)) {
        (Some(current), Some(creator)) => current.origin() == creator.origin(),
        _ => false,
    }
}

/// Match explicit custom URL features only within the trusted origin's path/query/fragment.
/// Absolute patterns must themselves be same-origin and have a non-root route feature.
/// Empty, whitespace-only and origin-only patterns can never authenticate a page.
fn custom_pattern_matches(current: &str, creator: &str, pattern: Option<&str>) -> bool {
    if !same_origin(current, creator) {
        return false;
    }
    let Some(current) = web_url(current) else { return false; };
    let Some(pattern) = pattern.map(str::trim).filter(|value| !value.is_empty()) else { return false; };
    let route = if pattern.contains("://") {
        let Some(pattern_url) = web_url(pattern) else { return false; };
        if pattern_url.origin() != current.origin() { return false; }
        route_feature(&pattern_url)
    } else {
        if pattern.starts_with("//") { return false; }
        pattern.to_string()
    };
    if matches!(route.as_str(), "" | "/" | "?" | "#") {
        return false;
    }
    route_feature(&current).contains(&route)
}

fn route_feature(url: &Url) -> String {
    let mut route = url.path().to_owned();
    if let Some(query) = url.query() {
        route.push('?');
        route.push_str(query);
    }
    if let Some(fragment) = url.fragment() {
        route.push('#');
        route.push_str(fragment);
    }
    route
}

#[derive(Deserialize)]
struct Evidence {
    url: String,
    origin: String,
    login: bool,
    logout: bool,
    account: bool,
}

/// Observe the current document only: never navigate, click, reload, or alter storage.
/// Unknown/off-origin/login UI => false. CDP/evaluation failures => redacted Err.
/// A custom pattern is an explicit user assertion, not an automatic URL heuristic.
pub(crate) async fn authenticated_probe(
    page: &Page,
    creator_url: &str,
    custom_pattern: Option<&str>,
) -> Result<bool> {
    let Some(creator) = web_url(creator_url) else { return Ok(false); };
    let origin = creator.origin().ascii_serialization();
    let origin_literal = serde_json::to_string(&origin)
        .map_err(|_| anyhow!("登录状态检测参数无效"))?;
    let script = PROBE.replace("__TRUSTED_ORIGIN__", &origin_literal);
    let evidence: Evidence = crate::obscura::evaluate(page, script).await
        .map_err(|_| anyhow!("登录状态检测连接失败"))?;
    if evidence.origin != origin || !same_origin(&evidence.url, creator_url) || evidence.login {
        return Ok(false);
    }
    Ok(evidence.logout || evidence.account
        || custom_pattern_matches(&evidence.url, creator_url, custom_pattern))
}

// The selectors intentionally require paired account identity + non-placeholder avatar.
// Generic images, profile links, creator URLs and the absence of a login URL are not proof.
const PROBE: &str = r#"(() => {
    const base = { url: window.location.href, origin: window.location.origin,
        login: false, logout: false, account: false };
    if (base.origin !== __TRUSTED_ORIGIN__) return base;
    const visible = el => {
        if (!el || el.getClientRects().length === 0) return false;
        for (let node = el; node instanceof Element; node = node.parentElement) {
            const style = getComputedStyle(node);
            if (node.hidden || node.getAttribute('aria-hidden') === 'true'
                || style.display === 'none' || style.visibility === 'hidden'
                || style.visibility === 'collapse' || Number(style.opacity) === 0) return false;
        }
        return true;
    };
    const text = el => (el.innerText || el.textContent || '').trim();
    const controls = [...document.querySelectorAll('a,button,[role="button"],input[type="submit"]')].filter(visible);
    // 抖音、小红书的登录/退出控件是普通 div/span（没有 role="button"），
    // 只查语义化标签会漏判，导致「页面还显示登录 UI」这道防误判保护失效。
    // 这里把「文本精确匹配候选词 + cursor:pointer」的元素也算作控件；
    // 先做便宜的文本预筛，再算可见性与 cursor，避免在大页面上全量遍历。
    const candidate = /^(登录|登陆|立即登录|立即登陆|扫码登录|扫码登陆|短信登录|密码登录|获取验证码|发送验证码|退出登录|退出登陆|退出账号|退出帐号|登出|Log ?in|Sign ?in|Log ?out|Sign ?out)$/i;
    controls.push(...[...document.querySelectorAll('div,span')].filter(el => {
        if (!candidate.test(text(el))) return false;
        return visible(el) && getComputedStyle(el).cursor === 'pointer';
    }));
    base.login = [...document.querySelectorAll('input[type="password"],input[autocomplete="one-time-code"],input[autocomplete="current-password"]')].some(visible)
        || controls.some(el => /^(登录|登陆|立即登录|立即登陆|扫码登录|扫码登陆|短信登录|密码登录|获取验证码|发送验证码|Log in|Sign in)$/i.test(text(el) || el.value || ''));
    base.logout = controls.some(el => /^(退出登录|退出登陆|退出账号|退出帐号|登出|Log out|Logout|Sign out)$/i.test(text(el)));
    const identity = selector => [...document.querySelectorAll(selector)].some(el =>
        visible(el) && text(el).length > 0 && !/登录|登陆|未登录|游客|log in|sign in/i.test(text(el)));
    const avatar = selector => [...document.querySelectorAll(selector)].some(el => {
        const src = el.currentSrc || el.getAttribute('src') || '';
        return visible(el) && src.length > 0 && !/default|placeholder|anonymous|not[-_]?login/i.test(src);
    });
    switch (window.location.hostname) {
        case 'mp.weixin.qq.com':
            base.account = identity('.weui-desktop-account__nickname,.account_nickname')
                && avatar('.weui-desktop-account__thumb img,img.weui-desktop-account__thumb,.account_meta .avatar img');
            break;
        case 'creator.douyin.com':
            // 实测（2026-11-09，真 Chrome 156，已登录账号）：
            // 顶部导航的用户信息在 `[class*="navigation-header"]` 下，昵称元素是
            // `.name-xxxxxx`（无稳定语义 class），头像是 `aweme/100x100/aweme-avatar/`
            // 域名的 <img>。原有 `[class*="header"] [class*="account-name"]` 前缀
            // 选择器在真实 DOM 上一个都匹配不到，导致已登录账号恒判为未登录。
            // 这里改为「昵称元素」或「aweme 头像」任一成立即算登录（真实 DOM 上
            // 昵称 class 无稳定语义，靠头像域名反而更稳）。
            base.account = (() => {
                const nick = identity('[class*="name-"]');
                const dyAvatar = avatar('img[src*="aweme-avatar"],img[src*="/aweme/"]');
                return nick || dyAvatar;
            })();
            break;
        case 'creator.xiaohongshu.com':
            base.account = identity('.user-info .user-name,.user-info .name,.account-info .nickname')
                && avatar('.user-info .avatar img,.user-info img.avatar,.account-info img.avatar');
            break;
        case 'mp.csdn.net':
            base.account = identity('#csdn-toolbar .toolbar-btn-user .user-name,#csdn-toolbar .toolbar-btn-user .nickname,.user-info .nickname')
                && avatar('#csdn-toolbar .toolbar-btn-user img,.user-info img.avatar');
            break;
    }
    return base;
})()"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_exact_and_normalize_default_ports() {
        assert!(same_origin("https://EXAMPLE.com:443/dashboard", "https://example.com/"));
        for current in ["https://example.com.evil.test/dashboard", "https://evil.test/?next=https://example.com", "http://example.com/dashboard", "https://example.com:8443/dashboard", "about:blank"] {
            assert!(!same_origin(current, "https://example.com/"));
        }
    }

    #[test]
    fn empty_or_origin_only_patterns_are_not_evidence() {
        for pattern in [None, Some(""), Some("  "), Some("/"), Some("https://example.com"), Some("https://example.com/")] {
            assert!(!custom_pattern_matches("https://example.com/dashboard", "https://example.com/", pattern));
        }
    }

    #[test]
    fn custom_route_patterns_require_the_configured_origin() {
        assert!(custom_pattern_matches("https://example.com/dashboard/home", "https://example.com/", Some("/dashboard")));
        assert!(custom_pattern_matches("https://example.com/dashboard/home", "https://example.com/", Some("https://example.com/dashboard")));
        assert!(!custom_pattern_matches("https://evil.test/dashboard", "https://example.com/", Some("dashboard")));
        assert!(!custom_pattern_matches("https://example.com/dashboard", "https://example.com/", Some("https://evil.test/dashboard")));
        assert!(!custom_pattern_matches("https://example.com/dashboard", "https://example.com/", Some("//example.com")));
        assert!(!custom_pattern_matches("https://example.com/dashboard", "https://example.com/", Some("example.com")));
    }
}
