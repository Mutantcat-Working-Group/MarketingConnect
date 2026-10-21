//! 短信提醒模块（阿里云短信服务）
//!
//! 掉号时通过阿里云短信 API（dysmsapi.aliyuncs.com）向配置的
//! 手机号发送提醒。API 密钥加密存储在本地 settings 表中。
//!
//! 签名算法（阿里云 RPC 风格）：
//! 1. 收集所有参数（公共参数 + 业务参数）
//! 2. 按参数名排序，RFC3986 百分比编码，构造规范化查询字符串
//! 3. StringToSign = "GET&%2F&" + encode(canonicalQueryString)
//! 4. Signature = Base64(HMAC-SHA1(AccessKeySecret + "&", StringToSign))
//! 5. 带 Signature 重新构造查询串，POST 到 API 端点

use anyhow::{Context, Result};
use base64::Engine;
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use sha1::Sha1;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha1 = Hmac<Sha1>;

/// 阿里云短信 API 端点
const SMS_ENDPOINT: &str = "https://dysmsapi.aliyuncs.com";

/// API 版本
const API_VERSION: &str = "2017-05-25";

/// RFC3986 百分比编码字符集
///
/// 不编码：A-Za-z0-9、`-`、`_`、`.`、`~`（阿里云要求）
const ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'!')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// 短信配置
#[derive(Debug, Clone, Default)]
pub struct SmsConfig {
    /// 阿里云 AccessKey ID
    pub access_key_id: String,
    /// 阿里云 AccessKey Secret（加密存储）
    pub access_key_secret: String,
    /// 短信签名（如「营销通」）
    pub sign_name: String,
    /// 短信模板 CODE（如 SMS_123456789）
    pub template_code: String,
    /// 接收手机号（多个用逗号分隔）
    pub phone_numbers: String,
}

impl SmsConfig {
    /// 配置是否完整可用
    pub fn is_configured(&self) -> bool {
        !self.access_key_id.trim().is_empty()
            && !self.access_key_secret.trim().is_empty()
            && !self.sign_name.trim().is_empty()
            && !self.template_code.trim().is_empty()
            && !self.phone_numbers.trim().is_empty()
    }
}

/// RFC3986 百分比编码
fn encode(s: &str) -> String {
    utf8_percent_encode(s, ENCODE_SET).to_string()
}

/// ISO8601 UTC 时间戳（阿里云要求格式：2026-10-21T12:00:00Z）
fn iso8601_utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 手工格式化，避免引入 chrono 的时区依赖问题
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let (year, month, day) = civil_from_days(days as i64);
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, minute, second
    )
}

/// 从 Unix 天数推算公历日期（Howard Hinnant 算法）
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 发送短信（阿里云 SendSms）
///
/// template_params 为模板变量键值对，如 `[("account", "测试账号")]`。
pub async fn send_sms(config: &SmsConfig, template_params: &[(&str, &str)]) -> Result<()> {
    if !config.is_configured() {
        anyhow::bail!("短信配置不完整，请先在设置中填写阿里云短信参数");
    }

    // 1. 公共参数 + 业务参数
    let timestamp = iso8601_utc_now();
    let nonce = uuid::Uuid::new_v4().to_string();

    // 模板参数 JSON（如 {"account":"xxx","platform":"douyin"}）
    let mut param_map = serde_json::Map::new();
    for (k, v) in template_params {
        param_map.insert(k.to_string(), serde_json::Value::String(v.to_string()));
    }
    let template_param_json = serde_json::Value::Object(param_map).to_string();

    let mut params: BTreeMap<String, String> = BTreeMap::new();
    params.insert("AccessKeyId".into(), config.access_key_id.clone());
    params.insert("Action".into(), "SendSms".into());
    params.insert("Format".into(), "JSON".into());
    params.insert("PhoneNumbers".into(), config.phone_numbers.clone());
    params.insert("RegionId".into(), "cn-hangzhou".into());
    params.insert("SignName".into(), config.sign_name.clone());
    params.insert("SignatureMethod".into(), "HMAC-SHA1".into());
    params.insert("SignatureNonce".into(), nonce);
    params.insert("SignatureVersion".into(), "1.0".into());
    params.insert("TemplateCode".into(), config.template_code.clone());
    params.insert("TemplateParam".into(), template_param_json);
    params.insert("Timestamp".into(), timestamp);
    params.insert("Version".into(), API_VERSION.into());

    // 2. 规范化查询字符串（BTreeMap 已按 key 排序）
    let canonical: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    // 3. 待签名字符串
    let string_to_sign = format!("GET&%2F&{}", encode(&canonical));

    // 4. HMAC-SHA1 签名
    let signing_key = format!("{}&", config.access_key_secret);
    let mut mac = HmacSha1::new_from_slice(signing_key.as_bytes())
        .context("HMAC 密钥初始化失败")?;
    mac.update(string_to_sign.as_bytes());
    let signature = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    // 5. 带签名重新构造查询串并发送
    params.insert("Signature".into(), signature);
    let query: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let url = format!("{}?{}", SMS_ENDPOINT, query);

    let resp = reqwest::Client::new()
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .await
        .context("短信 API 请求失败")?;

    let body = resp.text().await.context("读取短信 API 响应失败")?;
    let json: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| serde_json::json!({"raw": body}));

    if json["Code"].as_str() == Some("OK") {
        Ok(())
    } else {
        let msg = json["Message"]
            .as_str()
            .or_else(|| json["raw"].as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("短信发送失败: {}", msg)
    }
}

/// 发送掉号提醒短信
///
/// 模板变量约定：account（账号名）、platform（平台名）
pub async fn send_offline_alert(config: &SmsConfig, account_name: &str, platform: &str) -> Result<()> {
    send_sms(
        config,
        &[("account", account_name), ("platform", platform)],
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_reserved_chars() {
        assert_eq!(encode("abc"), "abc");
        assert_eq!(encode("a b"), "a%20b");
        assert_eq!(encode("a+b"), "a%2Bb");
        assert_eq!(encode("a/b"), "a%2Fb");
        assert_eq!(encode("a~b"), "a~b");
        assert_eq!(encode("中文"), "%E4%B8%AD%E6%96%87");
    }

    #[test]
    fn test_civil_from_days() {
        // 2026-10-21 是 Unix 天数 20704（近似验证）
        let (y, m, d) = civil_from_days(20704);
        assert_eq!((y, m, d), (2026, 10, 21));
    }

    #[test]
    fn test_iso8601_format() {
        let ts = iso8601_utc_now();
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.len(), 20);
    }
}
