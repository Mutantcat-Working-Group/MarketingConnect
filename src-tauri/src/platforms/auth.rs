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

/// 渲染时机导致的「还没画出来」最多等几次；每次间隔见 `PROBE_RETRY_DELAY`。
///
/// 各平台登录态的顶栏都是 SPA 在 `user/info` 之类的接口返回后才挂载的。
/// 单次求值会把「顶栏还没渲染」误当成「没登录」，进而在监控里误报掉号。
const PROBE_ATTEMPTS: usize = 3;
const PROBE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(1200);

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

    let mut last_error = None;
    for attempt in 0..PROBE_ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(PROBE_RETRY_DELAY).await;
        }
        let evidence: Evidence = match crate::obscura::evaluate(page, script.clone()).await {
            Ok(evidence) => evidence,
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };
        if evidence.origin != origin || !same_origin(&evidence.url, creator_url) || evidence.login {
            return Ok(false);
        }
        if evidence.logout
            || evidence.account
            || custom_pattern_matches(&evidence.url, creator_url, custom_pattern)
        {
            return Ok(true);
        }
    }
    match last_error {
        // 三次都是连接/求值失败：这是检测失败，不是掉号。
        Some(_) => Err(anyhow!("登录状态检测连接失败")),
        None => Ok(false),
    }
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
        if (!candidate.test((text(el) || '').replace(/\s+/g, ''))) return false;
        return visible(el) && getComputedStyle(el).cursor === 'pointer';
    }));
    // 小红书登录按钮文本是「登 录」（含半角空格）、知乎顶栏是「登录/注册」，
    // 直接精确匹配会全部漏掉，于是「页面还在登录 UI」这道防误判保护失效。
    // 先去掉所有空白再比对。
    const flat = el => (text(el) || el.value || '').replace(/\s+/g, '');
    base.login = [...document.querySelectorAll('input[type="password"],input[autocomplete="one-time-code"],input[autocomplete="current-password"]')].some(visible)
        || controls.some(el => /^(登录|登陆|立即登录|立即登陆|扫码登录|扫码登陆|短信登录|密码登录|验证码登录|获取验证码|发送验证码|登录\/注册|登陆\/注册|Log in|Sign in)$/i.test(flat(el)));
    // 知乎 PC 端顶栏退出项文案只有「退出」两字（bundle 原文），不带「登录」；
    // 同时这些菜单常驻 DOM 外（hover/点击才挂载），所以 logout 只能当兜底。
    base.logout = controls.some(el => /^(退出登录|退出登陆|退出账号|退出帐号|退出|登出|Log out|Logout|Sign out)$/i.test((text(el) || '').replace(/\s+/g, '')));
    const identity = selector => [...document.querySelectorAll(selector)].some(el =>
        visible(el) && text(el).length > 0 && !/登录|登陆|未登录|游客|log in|sign in/i.test(text(el)));
    const avatar = selector => [...document.querySelectorAll(selector)].some(el => {
        const src = el.currentSrc || el.getAttribute('src') || '';
        return visible(el) && src.length > 0 && !/default|placeholder|anonymous|not[-_]?login/i.test(src);
    });
    switch (window.location.hostname) {
        case 'mp.weixin.qq.com':
            // 实测（2026-11-20，真 Chrome 153，未登录页 + 官方前端 bundle 模板原文）：
            // 账号框模板 `mp_account_box.tpl` 的真实 class 是
            // `.acount_box-nickname`（少个 c，官方拼写错误）与
            // `img.weui-desktop-account__thumb`（class 挂在 img 自身，不是父容器）。
            // 旧选择器里的 `.weui-desktop-account__nickname` / `.account_nickname` /
            // `.account_meta .avatar img` 在真实 DOM 上一个都不存在，且旧逻辑是 AND，
            // 昵称恒不命中 → 头像选对了也白搭，已登录账号恒判未登录。
            // 退出登录项藏在点击头像后展开的 `account_box-panel` 里（v-show 默认隐藏），
            // 探测不点击，所以这个平台只能靠 account，没有兜底。
            base.account = identity('.acount_box-nickname')
                || avatar('img.weui-desktop-account__thumb');
            break;
        case 'creator.douyin.com':
            // 实测（2026-11-09，真 Chrome 156，已登录账号）：
            // 顶部导航的用户信息在 `[class*="navigation-header"]` 下，昵称元素是
            // `.name-xxxxxx`（无稳定语义 class），头像是 `aweme/100x100/aweme-avatar/`
            // 域名的 <img>。原有 `[class*="header"] [class*="account-name"]` 前缀
            // 选择器在真实 DOM 上一个都匹配不到，导致已登录账号恒判为未登录。
            // 这里改为「昵称元素」或「aweme 头像」任一成立即算登录（真实 DOM 上
            // 昵称 class 无稳定语义，靠头像域名反而更稳）。
            // 实测（2026-11-20，真 Chrome 153，未登录页 + `main.a20c35af.js` 模板原文）：
            // 登录态顶栏头像是 **div 的 background-image**（`#header-avatar`），
            // 根本不是 <img>，所以 `img[src*="aweme-avatar"]` 恒不命中；
            // `[class*="name-"]` 命中的是「快速导航」里写死的站点名（抖音官网等），
            // 与账号无关；`user-name-xxxx` 只在多账号冲突弹窗里出现。
            // 未登录页上 `#header-avatar` / `[class*="avatar-wrapper"]` / `[class*="logout"]`
            // / `[class*="notification-item"]` 实测命中数全为 0，可安全当作登录证据。
            // 退出账号项（`[class*="logout"]`）在初始 DOM 里没有，点开头像才渲染，只能兜底。
            base.account = !!document.querySelector('#header-avatar')
                || !!document.querySelector('[class*="avatar-wrapper"]')
                || !!document.querySelector('[class*="notification-item"]')
                || !!document.querySelector('[class*="logout"]');
            break;
        case 'creator.xiaohongshu.com':
            // 实测（2026-11-20，真 Chrome 153，未登录页 + 官方 bundle 模板原文）：
            // 登录后顶栏 `AppLayout` 渲染的是
            // `div.user-info > img.user_avatar` + `span.name-box`，
            // 旧选择器 `.user-info .user-name` / `.user-info .name` /
            // `.account-info .nickname` 与头像三条在真实 DOM 上全部命中 0，
            // 且旧逻辑是 AND → 已登录账号恒判未登录。
            // 未登录时登录按钮文本是「登 录」（含空格），上面已统一去空白后比对。
            // 头像用 OR：`userAvatar` 可能为空串，不能单独依赖头像。
            base.account = identity('#header-area .user-info span.name-box,.user-info span.name-box')
                || avatar('#header-area .user-info img.user_avatar,.user-info img.user_avatar');
            break;
        case 'mp.csdn.net':
            // 实测（2026-11-20，真 Chrome 158，同一台机器分别取「已登录」与「未登录」两份 DOM）：
            // 已登录时顶栏是 `#csdn-toolbar a.hasAvatar`，href 指向自己的
            // `blog.csdn.net/<用户 id>`；未登录时 `a.hasAvatar` 根本不存在（顶栏只有「登录」）。
            // 旧的 `.toolbar-btn-user .user-name` / `.user-info .nickname` 在真实 DOM 上一个都
            // 匹配不到，且已登录、未上传头像的账号 src 仍是
            // `profile-avatar.csdnimg.cn/default.jpg!2`，所以「非默认头像」这条也不能用作证据。
            // 改用「顶栏头像链接指向自己的博客」或「创作中心用户卡昵称」。
            base.account = (() => {
                const ownBlog = [...document.querySelectorAll('#csdn-toolbar a.hasAvatar[href*="blog.csdn.net/"]')]
                    .some(el => visible(el) && /\/\/blog\.csdn\.net\/[0-9A-Za-z_-]+/.test(el.getAttribute('href') || ''));
                const card = identity('.home-exp-user-card__head,.home-exp-user-card__name');
                return ownBlog || card;
            })();
            break;
        case 'www.zhihu.com':
            // 实测（2026-11-20，真 Chrome 153，未登录页 + `main.app.*.js` 组件原文）：
            // 原先这个 hostname 没有 case，`base.account` 恒为初值 false，
            // 只能靠 logout 兜底——而知乎 PC 端退出项文案只有「退出」两字、
            // 且藏在点击后才挂载的 Popover 里，等于已登录账号恒判未登录。
            // 登录后顶栏渲染 `img.AppHeader-profileAvatar`（className 由 Avatar 组件
            // 合并到 img 上）与 `/notifications`、`/messages` 两个入口。
            // 这些选择器在未登录页实测命中数全为 0。
            base.account = avatar('header img.AppHeader-profileAvatar,img.AppHeader-profileAvatar')
                || [...document.querySelectorAll('header a[href="/notifications"],header a[href="/messages"]')].some(el => visible(el));
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

    /// 五个平台的探测分支都必须给出可判定的证据，否则已登录账号恒判掉号。
    ///
    /// 这条测试守的是 2026-11-20 修掉的那批故障：CSDN / 抖音 / 小红书 /
    /// 公众号的选择器在真实 DOM 上一个都匹配不到，知乎连 case 都没有。
    /// 只断言「分支存在且非空」，不校验选择器本身是否命中真实页面
    /// （那需要真实登录态 DOM，放到端到端手测）。
    #[test]
    fn every_browser_platform_has_an_account_evidence_branch() {
        let script = PROBE.replace("__TRUSTED_ORIGIN__", "\"https://example.test\"");
        for (host, needle) in [
            ("mp.weixin.qq.com", "acount_box-nickname"),
            // 公众号真实 class 是 acount（少个 c），写错就等于没写
            ("creator.douyin.com", "#header-avatar"),
            ("creator.xiaohongshu.com", "name-box"),
            ("mp.csdn.net", "blog.csdn.net"),
            ("www.zhihu.com", "AppHeader-profileAvatar"),
        ] {
            let marker = format!("case '{host}':");
            assert!(script.contains(&marker), "探测脚本缺少 {host} 的登录证据分支");
            let branch = script.split(&marker).nth(1).unwrap_or_else(|| panic!("分支为空: {host}"));
            assert!(branch.contains("break;"), "{host} 分支没有 break");
            let body = branch.split("break;").next().unwrap_or_default();
            assert!(
                body.contains(needle),
                "{host} 分支里找不到证据 {needle}，真实 DOM 将恒判未登录"
            );
            assert!(body.contains("base.account ="), "{host} 分支没有写 base.account");
        }
    }

    /// 登录 UI 文本带空白（小红书「登 录」）或带斜杠（知乎「登录/注册」）时必须仍能识别
    #[test]
    fn login_text_is_matched_after_removing_whitespace() {
        let script = PROBE.replace("__TRUSTED_ORIGIN__", "\"https://example.test\"");
        assert!(script.contains("replace(/\\s+/g, '')"), "登录文本比对前未去空白");
        assert!(script.contains("登录\\/注册"), "登录 UI 词表缺少「登录/注册」");
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
