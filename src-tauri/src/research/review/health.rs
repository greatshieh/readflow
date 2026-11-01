//! 论点健康度判定（H1–H6）
//!
//! # 为什么是纯函数
//! 判定全部在 Rust 里做，SQL 只负责把"天数"和"计数"算成整数。这样边界值单测可以
//! 一行构造入参（`HealthMetrics { support_count: 3, .. }`），**不需要造库、不需要时钟**。
//! 一旦把阈值写进 SQL，测边界就得造相对时间的数据、跑 async，实际结果是"没人测边界"。
//!
//! # 为什么天数在 SQL 里算
//! 库里 `DEFAULT CURRENT_TIMESTAMP` 是 **UTC**，用 `julianday('now') - julianday(x)`
//! 得到整数天后，Rust 侧不需要解析任何时间字符串。H1 的阈值恰好骑在 14 天边界上，
//! 'now' 与 CURRENT_TIMESTAMP 同源，不会在午夜前后差一天。

use serde::Serialize;

/// 一条论点的全部原始度量（字段名与 `REVIEW_SQL` 的别名一一对应）
///
/// 全部是整数 / 布尔 / 短字符串：不含时间字符串、不含 `Option<NaiveDate>`。
/// 这是"能真正逐条测出边界"的前提。
#[derive(Debug, Clone, Default)]
pub struct HealthMetrics {
    pub status: String,
    pub confidence: i64,
    /// S：支持证据数
    pub support_count: i64,
    /// O：反驳证据数
    pub oppose_count: i64,
    /// M：存疑证据数
    pub mixed_count: i64,
    /// src：证据去重后的来源文章数
    pub source_count: i64,
    /// age：论点建立至今的天数
    pub age_days: i64,
    /// lastEv：最近一条证据距今天数；`None` = 从未挂过证据
    pub last_evidence_days: Option<i64>,
    /// ev30：该实体近 30 天的事件数
    pub event_count_30d: i64,
    /// 变更记录条数（含"建立论点"那一条）
    pub revision_count: i64,
}

/// 一条命中的健康规则
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthHit {
    /// 规则编号 "H1".."H6"
    pub rule: String,
    /// 严重级 high / mid / low
    pub severity: String,
    /// 卡片上显示的一句话（文案在 Rust 拼，前端只渲染）
    pub text: String,
    /// 是否参与提醒推送（H6 为 false：严重级太低，不值得打断）
    pub push: bool,
}

/// `lastEv` 的有效值：从未挂证据时回退为 `age`
///
/// 所有规则统一走它，别各写一遍——一处写 `unwrap_or(0)` 就会让"刚建、没证据"
/// 的论点命中最严重的规则。
fn last_ev(m: &HealthMetrics) -> i64 {
    m.last_evidence_days.unwrap_or(m.age_days)
}

/// 严重级排序权重（B 区排序：高 → 中 → 低）
pub fn severity_rank(s: &str) -> u8 {
    match s {
        "high" => 0,
        "mid" => 1,
        _ => 2,
    }
}

fn hit(rule: &str, severity: &str, push: bool, text: String) -> HealthHit {
    HealthHit {
        rule: rule.to_string(),
        severity: severity.to_string(),
        text,
        push,
    }
}

/// H1：一边倒且久未遇反例
///
/// `S >= 3 && O == 0 && age >= 14`。
/// 这是"确认偏误"最典型的形状：只收集支持自己的东西，且已经两周没再看过。
pub fn h1(m: &HealthMetrics) -> Option<HealthHit> {
    if m.support_count >= 3 && m.oppose_count == 0 && m.age_days >= 14 {
        Some(hit(
            "H1",
            "high",
            true,
            format!(
                "{} 条支持、0 条反驳，已经 {} 天没遇到反例",
                m.support_count, m.age_days
            ),
        ))
    } else {
        None
    }
}

/// H2：置信与证据量不匹配（双向）
///
/// `(C >= 4 && S+O <= 1) || (C <= 2 && S >= 5)`。
/// 两个方向都要报：前者是"没证据却很自信"，后者是"证据很多却不敢下判断"。
pub fn h2(m: &HealthMetrics) -> Option<HealthHit> {
    let overconfident = m.confidence >= 4 && m.support_count + m.oppose_count <= 1;
    let underconfident = m.confidence <= 2 && m.support_count >= 5;
    if overconfident {
        Some(hit(
            "H2",
            "mid",
            true,
            format!(
                "置信 {}，但只有 {} 条支持、{} 条反驳",
                m.confidence, m.support_count, m.oppose_count
            ),
        ))
    } else if underconfident {
        Some(hit(
            "H2",
            "mid",
            true,
            format!(
                "有一定证据（{} 条支持）却只给到置信 {}，是不是该提高？",
                m.support_count, m.confidence
            ),
        ))
    } else {
        None
    }
}

/// H3：多条证据其实同源
///
/// `S+O+M >= 3 && src <= 1`。
/// 四条"独立事件"可能只是同一篇稿子被不同站点转载——这是最容易被忽略、
/// 也最容易被纠正的一类伪证据量。
pub fn h3(m: &HealthMetrics) -> Option<HealthHit> {
    let total = m.support_count + m.oppose_count + m.mixed_count;
    if total >= 3 && m.source_count <= 1 {
        Some(hit(
            "H3",
            "high",
            true,
            format!(
                "{} 条证据只来自 {} 篇文章——可能是同一件事被反复计数",
                total, m.source_count
            ),
        ))
    } else {
        None
    }
}

/// H4：世界在动，判断没动
///
/// `status == "active" && ev30 >= 3 && ev >= 30`。
/// 注意用 `last_ev` 而不是 `last_evidence_days`：从未挂证据的老论点同样该被提醒。
pub fn h4(m: &HealthMetrics) -> Option<HealthHit> {
    if m.status == "active" && m.event_count_30d >= 3 && last_ev(m) >= 30 {
        Some(hit(
            "H4",
            "high",
            true,
            format!(
                "实体近 30 天有 {} 条事件，而这条判断已经 {} 天没动过",
                m.event_count_30d,
                last_ev(m)
            ),
        ))
    } else {
        None
    }
}

/// H5：结论与证据方向相反
///
/// `(confirmed && O > S) || (refuted && S > O)`。
/// 已经"确认"的判断被反驳证据反超（或反之），说明状态该改了。
pub fn h5(m: &HealthMetrics) -> Option<HealthHit> {
    if m.status == "confirmed" && m.oppose_count > m.support_count {
        Some(hit(
            "H5",
            "high",
            true,
            format!(
                "已确认，但反驳（{}）已经多于支持（{}）",
                m.oppose_count, m.support_count
            ),
        ))
    } else if m.status == "refuted" && m.support_count > m.oppose_count {
        Some(hit(
            "H5",
            "high",
            true,
            format!(
                "已否定，但支持（{}）已经多于反驳（{}）",
                m.support_count, m.oppose_count
            ),
        ))
    } else {
        None
    }
}

/// H6：长期未修订
///
/// `status == "active" && revision_count == 0 && ev >= 90`。
/// **不参与推送**（严重级低）：三个月没改不一定有问题，但值得在页面上问一句。
pub fn h6(m: &HealthMetrics) -> Option<HealthHit> {
    if m.status == "active" && m.revision_count == 0 && last_ev(m) >= 90 {
        Some(hit(
            "H6",
            "low",
            false,
            format!("登记已 {} 天、从未修改过——它现在还成立吗？", last_ev(m)),
        ))
    } else {
        None
    }
}

/// 判定 H1–H6
///
/// # 参数
/// * `m` - 一条论点的原始度量
///
/// # 返回
/// 命中的规则列表，顺序固定为 H1..H6（便于前端稳定排列与测试断言）
pub fn evaluate(m: &HealthMetrics) -> Vec<HealthHit> {
    [h1(m), h2(m), h3(m), h4(m), h5(m), h6(m)]
        .into_iter()
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个"完全健康"的基准：任何规则都不该命中
    fn base() -> HealthMetrics {
        HealthMetrics {
            status: "active".into(),
            confidence: 3,
            support_count: 2,
            oppose_count: 1,
            mixed_count: 0,
            source_count: 3,
            age_days: 20,
            last_evidence_days: Some(5),
            event_count_30d: 1,
            revision_count: 2,
        }
    }

    #[test]
    fn healthy_thesis_hits_nothing() {
        assert_eq!(evaluate(&base()), vec![], "基准形态不该命中任何规则");
    }

    // ─── H1：S>=3 && O==0 && age>=14 ───────────────────────────────────────

    #[test]
    fn h1_needs_all_three_conditions() {
        let mut m = base();
        m.support_count = 3;
        m.oppose_count = 0;
        m.age_days = 14;
        assert_eq!(h1(&m).expect("该命中").rule, "H1", "三个条件刚好都满足");

        m.age_days = 13;
        assert!(h1(&m).is_none(), "差一天就不算（边界在 14）");

        m.age_days = 14;
        m.oppose_count = 1;
        assert!(h1(&m).is_none(), "有反驳就不算一边倒");

        m.oppose_count = 0;
        m.support_count = 2;
        assert!(h1(&m).is_none(), "支持不足 3 条不报");
    }

    // ─── H2：置信与证据量不匹配 ────────────────────────────────────────────

    #[test]
    fn h2_fires_in_both_directions() {
        let mut m = base();
        m.confidence = 4;
        m.support_count = 1;
        m.oppose_count = 0;
        let hit = h2(&m).expect("高置信 + 证据少该命中");
        assert_eq!(hit.severity, "mid");

        m.confidence = 2;
        m.support_count = 5;
        m.oppose_count = 0;
        assert!(h2(&m).is_some(), "低置信 + 证据多也该命中");

        // 两个方向都不成立：置信 3 且证据量正常
        m.confidence = 3;
        m.support_count = 3;
        assert!(h2(&m).is_none());
    }

    #[test]
    fn h2_boundary_on_evidence_sum() {
        let mut m = base();
        m.confidence = 5;
        m.support_count = 1;
        m.oppose_count = 1; // S+O = 2 > 1
        assert!(h2(&m).is_none(), "证据条数超过 1 就不算'没证据却很自信'");

        m.oppose_count = 0; // S+O = 1
        assert!(h2(&m).is_some(), "S+O 恰好为 1 仍然命中");
    }

    // ─── H3：多证据同源 ───────────────────────────────────────────────────

    #[test]
    fn h3_counts_all_three_stances() {
        let mut m = base();
        m.support_count = 1;
        m.oppose_count = 1;
        m.mixed_count = 1; // 合计 3
        m.source_count = 1;
        let hit = h3(&m).expect("3 条证据来自 1 篇文章该命中");
        assert_eq!(hit.severity, "high");

        m.source_count = 2;
        assert!(h3(&m).is_none(), "两篇文章就不算同源");

        m.source_count = 0;
        m.mixed_count = 0; // 合计 2
        assert!(h3(&m).is_none(), "证据总数不足 3 不报，哪怕源数为 0");
    }

    // ─── H4：世界在动，判断没动 ────────────────────────────────────────────

    #[test]
    fn h4_falls_back_to_age_when_never_evidenced() {
        let mut m = base();
        m.event_count_30d = 3;
        m.last_evidence_days = None; // 从未挂证据
        m.age_days = 30;
        assert!(h4(&m).is_some(), "没有证据时 lastEv 回退为 age");

        m.age_days = 29;
        assert!(h4(&m).is_none(), "回退值不足 30 天不报");

        // 有证据但太旧
        m.age_days = 100;
        m.last_evidence_days = Some(30);
        assert!(h4(&m).is_some(), "恰好 30 天算旧");
        m.last_evidence_days = Some(29);
        assert!(h4(&m).is_none(), "29 天还不算");

        // 事件不够多
        m.last_evidence_days = Some(60);
        m.event_count_30d = 2;
        assert!(h4(&m).is_none());

        // 非 active 不报
        m.event_count_30d = 5;
        m.status = "confirmed".into();
        assert!(h4(&m).is_none(), "只有 active 才报'没跟上'");
    }

    // ─── H5：结论与证据方向相反 ────────────────────────────────────────────

    #[test]
    fn h5_fires_on_both_contradictions() {
        let mut m = base();
        m.status = "confirmed".into();
        m.support_count = 1;
        m.oppose_count = 2;
        assert!(h5(&m).is_some(), "已确认但反驳更多");

        m.status = "refuted".into();
        m.support_count = 2;
        m.oppose_count = 1;
        assert!(h5(&m).is_some(), "已否定但支持更多");

        // 相等不算"反超"
        m.support_count = 2;
        m.oppose_count = 2;
        assert!(h5(&m).is_none(), "打平不算方向相反");

        // active 不走这条
        m.status = "active".into();
        m.support_count = 1;
        m.oppose_count = 5;
        assert!(h5(&m).is_none(), "active 有反驳是正常状态");
    }

    // ─── H6：长期未修订 ───────────────────────────────────────────────────

    #[test]
    fn h6_is_low_severity_and_not_pushed() {
        let mut m = base();
        m.revision_count = 0;
        m.last_evidence_days = Some(90);
        let hit = h6(&m).expect("90 天未修订该命中");
        assert_eq!(hit.severity, "low");
        assert!(!hit.push, "H6 不参与推送：严重级太低，不值得打断用户");

        m.revision_count = 1;
        assert!(h6(&m).is_none(), "有过修订就不报");
        m.revision_count = 0;
        m.last_evidence_days = Some(89);
        assert!(h6(&m).is_none(), "不满 90 天不报");
    }

    // ─── 组合与排序 ───────────────────────────────────────────────────────

    #[test]
    fn evaluate_returns_hits_in_rule_order() {
        let mut m = base();
        // 同时触发 H1（一边倒）与 H3（同源）
        m.support_count = 3;
        m.oppose_count = 0;
        m.source_count = 1;
        m.age_days = 30;
        let hits = evaluate(&m);
        let rules: Vec<&str> = hits.iter().map(|h| h.rule.as_str()).collect();
        assert_eq!(rules, vec!["H1", "H3"], "顺序固定 H1..H6，前端据此稳定排列");
    }

    #[test]
    fn severity_rank_orders_high_first() {
        assert!(severity_rank("high") < severity_rank("mid"));
        assert!(severity_rank("mid") < severity_rank("low"));
        assert_eq!(severity_rank("unknown"), 2, "未知级别排最后，不 panic");
    }

    #[test]
    fn hits_serialize_as_camel_case() {
        // 出参漏了 rename 不会报错，只表现为前端那几个字段 undefined
        let hit = HealthHit {
            rule: "H1".into(),
            severity: "high".into(),
            text: "文案".into(),
            push: true,
        };
        let v = serde_json::to_value(&hit).expect("序列化失败");
        for key in ["rule", "severity", "text", "push"] {
            assert!(v.get(key).is_some(), "缺字段 {key}");
        }
    }
}
