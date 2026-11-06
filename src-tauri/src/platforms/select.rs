// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 多候选选择器与媒体上传内核
//!
//! 平台发布页在登录态之后，无法预先验证选择器，因此统一采用「按顺序尝试
//! 多个候选，全部失败则报出尝试过的全部选择器」。

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chromiumoxide::Page;

/// 元素操作抽象，便于用假实现做单元测试
#[async_trait]
pub trait ElementOps: Send + Sync {
    async fn fill(&self, selector: &str, value: &str) -> Result<()>;
    async fn click(&self, selector: &str) -> Result<()>;
    /// 上传文件。媒体上传要到 Task 9 的知乎适配器才接线，先落地接口；
    /// 加 allow 以免为尚未接线的骨架引入新的编译警告（同 `Capabilities` 的处理）。
    #[allow(dead_code)]
    async fn upload(&self, selector: &str, paths: &[String]) -> Result<()>;
}

/// 依次尝试候选选择器填入文本，返回命中的选择器
pub async fn fill_any<O: ElementOps + ?Sized>(
    ops: &O,
    selectors: &[&str],
    value: &str,
) -> Result<String> {
    let mut tried = Vec::new();
    for selector in selectors {
        tried.push(*selector);
        if ops.fill(selector, value).await.is_ok() {
            return Ok((*selector).to_string());
        }
    }
    Err(anyhow!("找不到可填入的输入框，已尝试: {}", tried.join(" | ")))
}

/// 依次尝试候选选择器点击，返回命中的选择器
pub async fn click_any<O: ElementOps + ?Sized>(
    ops: &O,
    selectors: &[&str],
) -> Result<String> {
    let mut tried = Vec::new();
    for selector in selectors {
        tried.push(*selector);
        if ops.click(selector).await.is_ok() {
            return Ok((*selector).to_string());
        }
    }
    Err(anyhow!("找不到可点击的元素，已尝试: {}", tried.join(" | ")))
}

/// 依次尝试候选选择器上传文件
///
/// 媒体上传要到 Task 9 的知乎适配器才接线，先落地接口；
/// 加 allow 以免为尚未接线的骨架引入新的编译警告（同 `Capabilities` 的处理）。
#[allow(dead_code)]
pub async fn upload_any<O: ElementOps + ?Sized>(
    ops: &O,
    selectors: &[&str],
    paths: &[String],
) -> Result<String> {
    if paths.is_empty() {
        return Err(anyhow!("没有需要上传的文件"));
    }
    let mut tried = Vec::new();
    for selector in selectors {
        tried.push(*selector);
        if ops.upload(selector, paths).await.is_ok() {
            return Ok((*selector).to_string());
        }
    }
    Err(anyhow!("找不到文件上传控件，已尝试: {}", tried.join(" | ")))
}

/// 基于真实页面的元素操作
pub struct PageOps<'a> {
    pub page: &'a Page,
}

#[async_trait]
impl ElementOps for PageOps<'_> {
    async fn fill(&self, selector: &str, value: &str) -> Result<()> {
        let element = self.page.find_element(selector).await?;
        element.click().await?;
        element.type_str(value).await?;
        Ok(())
    }

    async fn click(&self, selector: &str) -> Result<()> {
        self.page.find_element(selector).await?.click().await?;
        Ok(())
    }

    async fn upload(&self, selector: &str, paths: &[String]) -> Result<()> {
        let element = self.page.find_element(selector).await?;
        let mut params = chromiumoxide::cdp::browser_protocol::dom::SetFileInputFilesParams::new(
            paths.to_vec(),
        );
        params.backend_node_id = Some(element.backend_node_id);
        self.page.execute(params).await?;
        Ok(())
    }
}

/// 发送一个按键。`chromiumoxide` 的 `Page` **没有** `keyboard()`，
/// 必须直接发 `Input.dispatchKeyEvent`；实现照 `src-tauri/src/login.rs:42-52` 的既有写法。
///
/// 按键要到 Task 9 的知乎适配器（话题输入后回车）才接线，先落地接口；
/// 加 allow 以免为尚未接线的骨架引入新的编译警告（同 `Capabilities` 的处理）。
#[allow(dead_code)]
pub async fn press_key(page: &Page, key: &str) -> Result<()> {
    let code = match key {
        "Enter" => 13,
        "Tab" => 9,
        "Escape" => 27,
        other => anyhow::bail!("不支持的按键: {other}"),
    };
    for event in [if key == "Enter" { "keyDown" } else { "rawKeyDown" }, "keyUp"] {
        let mut raw = serde_json::json!({
            "type": event, "key": key, "code": key,
            "windowsVirtualKeyCode": code, "nativeVirtualKeyCode": code
        });
        if key == "Enter" && event == "keyDown" {
            raw["text"] = serde_json::json!("\r");
        }
        let params: chromiumoxide::cdp::browser_protocol::input::DispatchKeyEventParams =
            serde_json::from_value(raw).context("按键参数无效")?;
        page.execute(params).await.context("发送按键失败")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 只对白名单内的选择器成功，并记录尝试顺序
    #[derive(Default)]
    struct FakeOps {
        fill_ok: Vec<String>,
        click_ok: Vec<String>,
        upload_ok: Vec<String>,
        tried: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ElementOps for FakeOps {
        async fn fill(&self, selector: &str, _value: &str) -> Result<()> {
            self.tried.lock().unwrap().push(format!("fill:{selector}"));
            if self.fill_ok.iter().any(|s| s == selector) { Ok(()) } else { Err(anyhow!("no")) }
        }
        async fn click(&self, selector: &str) -> Result<()> {
            self.tried.lock().unwrap().push(format!("click:{selector}"));
            if self.click_ok.iter().any(|s| s == selector) { Ok(()) } else { Err(anyhow!("no")) }
        }
        async fn upload(&self, selector: &str, _paths: &[String]) -> Result<()> {
            self.tried.lock().unwrap().push(format!("upload:{selector}"));
            if self.upload_ok.iter().any(|s| s == selector) { Ok(()) } else { Err(anyhow!("no")) }
        }
    }

    #[tokio::test]
    async fn fill_uses_first_working_candidate() {
        let ops = FakeOps { fill_ok: vec!["#third".into()], ..Default::default() };
        let hit = fill_any(&ops, &["#first", "#second", "#third"], "内容").await.unwrap();
        assert_eq!(hit, "#third");
        let tried = ops.tried.lock().unwrap().clone();
        assert_eq!(tried, vec!["fill:#first", "fill:#second", "fill:#third"], "必须按顺序尝试");
    }

    #[tokio::test]
    async fn fill_stops_after_first_success() {
        let ops = FakeOps { fill_ok: vec!["#first".into(), "#second".into()], ..Default::default() };
        fill_any(&ops, &["#first", "#second"], "内容").await.unwrap();
        assert_eq!(ops.tried.lock().unwrap().len(), 1, "命中后不应继续尝试");
    }

    #[tokio::test]
    async fn fill_reports_all_attempted_selectors() {
        let ops = FakeOps::default();
        let err = fill_any(&ops, &["#a", "#b"], "内容").await.unwrap_err().to_string();
        assert!(err.contains("#a") && err.contains("#b"), "错误必须列出全部尝试过的选择器: {err}");
    }

    #[tokio::test]
    async fn click_returns_matched_selector() {
        let ops = FakeOps { click_ok: vec!["button.submit".into()], ..Default::default() };
        assert_eq!(click_any(&ops, &["#x", "button.submit"]).await.unwrap(), "button.submit");
    }

    #[tokio::test]
    async fn upload_rejects_empty_paths() {
        let ops = FakeOps::default();
        assert!(upload_any(&ops, &["input[type=file]"], &[]).await.is_err());
        assert!(ops.tried.lock().unwrap().is_empty(), "空文件列表不应触碰页面");
    }

    #[tokio::test]
    async fn upload_tries_each_candidate() {
        let ops = FakeOps { upload_ok: vec!["input.upload".into()], ..Default::default() };
        let paths = vec!["/tmp/a.png".to_string()];
        assert_eq!(upload_any(&ops, &["input[type=file]", "input.upload"], &paths).await.unwrap(), "input.upload");
    }
}
