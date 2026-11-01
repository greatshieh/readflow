//! 周复盘（阶段二）
//!
//! 本模块当前只承载 TASK-00 的一次性数据搬运：把「证据的来源身份」补进
//! `thesis_evidence_source`。周复盘本体的表与命令尚未开工，届时一并放进这里。
//!
//! # 为什么回填放在这里而不是 `thesis.rs`
//! `thesis.rs` 是论点的**日常**读写（新建、改状态、挂证据），一次性的数据搬运
//! 挂在那里会让"每次打开论点都在跑"的代码和"只在启动跑一次"的代码混在一起。
//!
//! # 为什么不在 `db::create_tables` 里跑
//! 建表函数只该建结构。一旦它带副作用，将来任何一次 schema 变更都可能顺带触发
//! 数据搬运，而建表是**每次启动**都跑的。

use sqlx::{Row, SqlitePool};

/// 回填证据来源身份（启动时跑一次，幂等）
///
/// # 为什么需要
/// 本表建出来之前挂的证据没有 source 行，而**源还在的每一小时都是回填窗口**——
/// 清理判据是 `published_at` 而非抓取时间，一篇 2019 年的旧文下次清理就没了，
/// 与"证据挂了多久"毫无关系。
///
/// # 行为
/// 只处理「还没有 source 行」的证据：
/// - 源仍在 → 写入 `article_id` + 来源名 + 发布日期；
/// - 源已失联 → **写一行空来源**（`article_id` 为 NULL、两个文本列为空串），而不是跳过。
///   跳过的代价是每次启动都重新扫一遍这些永远不会成功的行，且从此无法区分
///   「没跑过」（改代码能救）与「跑过但当时就丢了」（只能接受）。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回
/// 本次实际写入的行数。第二次启动起恒为 0。
///
/// # 错误
/// 查询或写入失败时返回错误信息
///
/// # 诊断
/// 三种状态的计数（三者之和恒等于 `thesis_evidence` 行数）：
/// ```sql
/// SELECT SUM(s.article_id IS NOT NULL)                          AS recorded,
///        SUM(s.evidence_id IS NOT NULL AND s.article_id IS NULL) AS lost,
///        SUM(s.evidence_id IS NULL)                             AS pending
/// FROM thesis_evidence v
/// LEFT JOIN thesis_evidence_source s ON s.evidence_id = v.id;
/// ```
pub async fn backfill_evidence_sources(pool: &SqlitePool) -> Result<u64, String> {
    // 一次查出全部待办。`pending` 为空时立即返回 —— 稳态下这是每次启动的主路径，
    // 不该为此再跑一轮全表 UPDATE。
    let pending = sqlx::query(
        "SELECT v.id AS evidence_id, v.kind AS kind, v.ref_id AS ref_id \
         FROM thesis_evidence v \
         LEFT JOIN thesis_evidence_source s ON s.evidence_id = v.id \
         WHERE s.evidence_id IS NULL",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询待回填证据失败：{e}"))?;

    if pending.is_empty() {
        return Ok(0);
    }

    let mut written = 0u64;
    for row in pending {
        let evidence_id: i64 = row.get("evidence_id");
        let kind: String = row.get("kind");
        let ref_id: i64 = row.get("ref_id");

        // 按证据类型倒推文章 ID。`article` 类证据的 ref_id 就是文章自身；
        // 另两类要去来源表查 —— 源可能已随文章级联删除，查不到就是 None。
        let article_id = match kind.as_str() {
            "article" => Some(ref_id),
            "event" => sqlx::query_scalar::<_, i64>("SELECT article_id FROM research_events WHERE id = ?")
                .bind(ref_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| format!("查询事件所属文章失败：{e}"))?,
            "highlight" => sqlx::query_scalar::<_, i64>("SELECT article_id FROM highlights WHERE id = ?")
                .bind(ref_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| format!("查询高亮所属文章失败：{e}"))?,
            // 未来可能出现的外部摘录（kind='external'）等：没有对应的本地文章
            _ => None,
        };

        let (aid, name, published) = match article_id {
            Some(aid) => {
                let src = sqlx::query(
                    "SELECT COALESCE(NULLIF(f.name, ''), NULLIF(a.author, ''), '') AS source_name, \
                            COALESCE(a.published_at, '') AS published_at \
                     FROM articles a LEFT JOIN feeds f ON f.id = a.feed_id \
                     WHERE a.id = ?",
                )
                .bind(aid)
                .fetch_optional(pool)
                .await
                .map_err(|e| format!("查询证据来源失败：{e}"))?;
                match src {
                    Some(r) => (
                        Some(aid),
                        r.get::<String, _>("source_name"),
                        r.get::<String, _>("published_at"),
                    ),
                    // 来源实体还在、文章行却已没了。级联删不出这种组合，但脏数据会有
                    // ——记成"失联"（状态②），不留着下次再试。
                    None => (None, String::new(), String::new()),
                }
            }
            None => (None, String::new(), String::new()),
        };

        sqlx::query(
            "INSERT OR IGNORE INTO thesis_evidence_source \
             (evidence_id, article_id, source_name, published_at) VALUES (?, ?, ?, ?)",
        )
        .bind(evidence_id)
        .bind(aid)
        .bind(&name)
        .bind(&published)
        .execute(pool)
        .await
        .map_err(|e| format!("写入证据来源失败：{e}"))?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// 建一个**真实 schema** 的内存库
    ///
    /// 刻意不手抄表结构：手抄会在 schema 演进时静默漂移，而回填逻辑横跨
    /// `thesis_evidence` / `thesis_evidence_source` / `articles` / `feeds` 四张表，
    /// 漂移的代价是"测试全绿但线上查错列"。
    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("建库失败");
        crate::db::create_tables(&pool).await.expect("建表失败");
        pool
    }

    /// 造 1 个订阅源 + 1 个实体 + 1 个论点 + n 篇文章
    ///
    /// @param feed_name - 订阅源名（同时用作 url 的一部分，避免唯一约束冲突）
    /// @param n - 文章数量（每篇文章 guid 不同）
    /// @returns `(thesis_id, article_ids)`
    async fn seed(pool: &SqlitePool, feed_name: &str, n: usize) -> (i64, Vec<i64>) {
        let feed_id: i64 = sqlx::query_scalar("INSERT INTO feeds (name, url) VALUES (?, ?) RETURNING id")
            .bind(feed_name)
            .bind(format!("https://example.com/{feed_name}"))
            .fetch_one(pool)
            .await
            .expect("插订阅源失败");
        let mut aids = Vec::new();
        for i in 0..n {
            let aid: i64 = sqlx::query_scalar(
                "INSERT INTO articles (feed_id, guid, link, title, author, published_at) \
                 VALUES (?, ?, ?, '储能出货超预期', '张三', '2026-10-28') RETURNING id",
            )
            .bind(feed_id)
            .bind(format!("guid-{feed_name}-{i}"))
            .bind(format!("https://example.com/{feed_name}/{i}"))
            .fetch_one(pool)
            .await
            .expect("插文章失败");
            aids.push(aid);
        }
        let eid: i64 = sqlx::query_scalar(
            "INSERT INTO entities (name, entity_type) VALUES ('宁德时代', 'company') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .expect("插实体失败");
        let tid: i64 = sqlx::query_scalar("INSERT INTO theses (statement, entity_id) VALUES ('判断', ?) RETURNING id")
            .bind(eid)
            .fetch_one(pool)
            .await
            .expect("插论点失败");
        (tid, aids)
    }

    /// 挂一条 article 类证据（绕开 thesis::add_evidence，模拟"本表建之前挂的老证据"）
    async fn attach_legacy(pool: &SqlitePool, thesis_id: i64, article_id: i64) {
        sqlx::query(
            "INSERT INTO thesis_evidence (thesis_id, kind, ref_id, stance) \
             VALUES (?, 'article', ?, 'support')",
        )
        .bind(thesis_id)
        .bind(article_id)
        .execute(pool)
        .await
        .expect("插老证据失败");
    }

    #[tokio::test]
    async fn backfill_is_idempotent() {
        let pool = test_pool().await;
        let (tid, aids) = seed(&pool, "财新", 3).await;
        for aid in &aids {
            attach_legacy(&pool, tid, *aid).await;
        }

        let first = backfill_evidence_sources(&pool).await.expect("首轮回填失败");
        assert_eq!(first, 3, "三条老证据都该被回填");

        let second = backfill_evidence_sources(&pool).await.expect("二轮回填失败");
        assert_eq!(second, 0, "第二次启动不该再有可回填的行");

        let recorded: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM thesis_evidence_source WHERE article_id IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("统计失败");
        assert_eq!(recorded, 3, "三行都该带着 article_id");
    }

    #[tokio::test]
    async fn backfill_distinguishes_unbackfilled_from_lost() {
        let pool = test_pool().await;
        let (tid, aids) = seed(&pool, "财新", 2).await;
        for aid in &aids {
            attach_legacy(&pool, tid, *aid).await;
        }
        // 第二篇文章被清理（`thesis_evidence.ref_id` 故意不建外键，证据行留着）
        sqlx::query("DELETE FROM articles WHERE id = ?")
            .bind(aids[1])
            .execute(&pool)
            .await
            .expect("删文章失败");

        let written = backfill_evidence_sources(&pool).await.expect("回填失败");
        assert_eq!(written, 2, "源失联的那条也要写行，否则会每次启动都重扫");

        let recorded: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM thesis_evidence_source WHERE article_id IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("统计①失败");
        let lost: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM thesis_evidence_source WHERE article_id IS NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("统计②失败");
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM thesis_evidence")
            .fetch_one(&pool)
            .await
            .expect("统计证据失败");

        assert_eq!(recorded, 1, "源还在的那条应是状态①");
        assert_eq!(lost, 1, "源已删的那条应是状态②");
        assert_eq!(recorded + lost, total, "三种状态之和必须等于证据总数");

        let again = backfill_evidence_sources(&pool).await.expect("二次回填失败");
        assert_eq!(again, 0, "状态②不该被反复重试");
    }
}
