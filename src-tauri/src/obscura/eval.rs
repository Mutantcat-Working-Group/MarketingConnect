// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 带重试的页面求值
//!
//! 引擎（Obscura）在页面脚本自行跳转的瞬间会销毁并重建执行上下文，
//! 而 chromiumoxide 缓存的 context id 会短暂失效，表现为
//! `Cannot find context with specified id: N`。
//!
//! 实测该错误是瞬时的（数秒内的下一次调用即成功），且上下文 id 每次都不同，
//! 因此这里只在「上下文失效」这一类错误上重试，不掩盖真实的脚本异常。

use anyhow::{Result, anyhow};
use chromiumoxide::Page;
use serde::de::DeserializeOwned;
use std::time::Duration;
use tracing::debug;

/// 重试次数（不含首次）
const RETRIES: usize = 3;

/// 引擎/浏览器表示「执行上下文已失效」的错误特征
const CONTEXT_ERRORS: [&str; 4] = [
    "Cannot find context with specified id",
    "Cannot find context",
    "Execution context was destroyed",
    "No execution context",
];

fn is_context_error(message: &str) -> bool {
    CONTEXT_ERRORS.iter().any(|needle| message.contains(needle))
}

/// 在页面上求值并解析为 `T`，遇到执行上下文失效时自动重试。
pub async fn evaluate<T>(page: &Page, expression: impl Into<String>) -> Result<T>
where
    T: DeserializeOwned,
{
    let expression = expression.into();
    let mut last_error = None;

    for attempt in 0..=RETRIES {
        if attempt > 0 {
            debug!("执行上下文失效，第 {} 次重试页面求值", attempt);
            tokio::time::sleep(Duration::from_millis(400 * attempt as u64)).await;
        }

        match page.evaluate(expression.as_str()).await {
            Ok(result) => match result.into_value::<T>() {
                Ok(value) => return Ok(value),
                Err(error) => {
                    return Err(anyhow!("无法解析求值结果: {}", error));
                }
            },
            Err(error) => {
                let message = error.to_string();
                if !is_context_error(&message) {
                    return Err(anyhow!("{}", message));
                }
                last_error = Some(message);
            }
        }
    }

    Err(anyhow!(
        "{}",
        last_error.unwrap_or_else(|| "页面求值失败".to_string())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_context_errors() {
        assert!(is_context_error("Error -32601: Cannot find context with specified id: 7"));
        assert!(is_context_error("Execution context was destroyed."));
        assert!(is_context_error("No execution context"));
    }

    #[test]
    fn ignores_unrelated_errors() {
        assert!(!is_context_error("oneshot canceled"));
        assert!(!is_context_error("net::ERR_NAME_NOT_RESOLVED"));
        assert!(!is_context_error("SyntaxError: Unexpected token"));
    }
}
