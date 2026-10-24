// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 敏感词检测
//!
//! 内置常见违规词库（广告法极限词、医疗违规宣称、金融违规话术），
//! 发布前检测内容，降低平台审核驳回与限流风险。
//!
//! 说明：词库为常见高频违规词的精选子集（非全量），
//! 覆盖矩阵营销最常踩中的几类红线。

use std::sync::LazyLock;

/// 敏感词库（分类：广告法极限词 / 医疗违规 / 金融违规 / 其他）
static SENSITIVE_WORDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        // ---- 广告法极限词（《广告法》第九条）----
        "最好",
        "最佳",
        "最强",
        "最先进",
        "最高级",
        "最便宜",
        "最低价",
        "全网最低",
        "史上最低",
        "史上最",
        "第一品牌",
        "销量第一",
        "排名第一",
        "行业第一",
        "中国第一",
        "世界第一",
        "国家级",
        "世界级",
        "顶级",
        "极致",
        "绝对",
        "100%",
        "百分百",
        "万能",
        "永久",
        "独家",
        "首选",
        "金牌",
        "领袖品牌",
        "尖端",
        "至尊",
        "巅峰",
        "独一无二",
        "无可比拟",
        "空前",
        "绝后",
        "仅此一次",
        "最后一波",
        "错过不再",
        // ---- 医疗违规宣称 ----
        "治愈",
        "根治",
        "药到病除",
        "特效",
        "消炎",
        "抗癌",
        "防癌",
        "降血糖",
        "降血压",
        "医美级",
        "医学级",
        "治疗",
        "疗效",
        // ---- 金融违规话术 ----
        "稳赚",
        "稳赚不赔",
        "保本",
        "零风险",
        "无风险",
        "高收益",
        "高回报",
        "保证收益",
        "保收益",
        "翻倍赚",
        "一夜暴富",
        // ---- 其他常见违规 ----
        "刷单",
        "假一赔十",
        "假一罚万",
        "点击领钱",
        "免费领取",
        "0元购",
        "内部渠道",
        "特供",
        "专供",
    ]
});

/// 检测文本中的敏感词
///
/// 返回命中的敏感词列表（去重，按词库顺序）。
pub fn check_sensitive_words(text: &str) -> Vec<String> {
    let mut hits: Vec<String> = Vec::new();
    for word in SENSITIVE_WORDS.iter() {
        let w: &str = *word;
        if text.contains(w) && !hits.iter().any(|h| h == w) {
            hits.push(w.to_string());
        }
    }
    hits
}

/// 敏感词总数（用于前端展示词库规模）
pub fn word_count() -> usize {
    SENSITIVE_WORDS.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_absolute_terms() {
        let hits = check_sensitive_words("这是全网最好的产品，销量第一");
        assert!(hits.contains(&"最好".to_string()));
        assert!(hits.contains(&"销量第一".to_string()));
    }

    #[test]
    fn test_detect_medical_claims() {
        let hits = check_sensitive_words("本产品可以治愈各种疾病");
        assert!(hits.contains(&"治愈".to_string()));
    }

    #[test]
    fn test_detect_financial_claims() {
        let hits = check_sensitive_words("稳赚不赔，零风险高收益");
        assert!(hits.contains(&"稳赚不赔".to_string()));
        assert!(hits.contains(&"零风险".to_string()));
        assert!(hits.contains(&"高收益".to_string()));
    }

    #[test]
    fn test_clean_text_no_hits() {
        let hits = check_sensitive_words("今天天气不错，分享一个实用小技巧");
        assert!(hits.is_empty());
    }

    #[test]
    fn test_no_duplicates() {
        let hits = check_sensitive_words("最好最好最好");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], "最好");
    }
}
