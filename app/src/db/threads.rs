//! スレッドの永続化(F05スレッド作成)。
//!
//! `insert`はF05(web/thread_create.rs)が使う。`list_all`はP03スレッド一覧
//! (web/thread_list.rs)がAC05-3(作成後に一覧へ表示される)を満たすために使う
//! 最小限の読み取り。検索・ソート・ページネーション(F09〜F13)は範囲外
//! (`domain/query.rs`に純粋ロジックの土台があるが、本サイクルでは使わない
//! ——ユーザー承認済みのスコープ、dev-docs/foundation-plan.md)。

use sqlx::PgExecutor;

/// 新規スレッドを1件挿入し、採番されたIDを返す。
///
/// `executor`は`&PgPool`にも`&mut Transaction`にも当てはまるようジェネリックに
/// してある(decision 0002: thread_create.rsのハンドラは`db::with_transaction`で
/// 開いたトランザクション越しにこれを呼ぶ)。
pub async fn insert<'e, E>(
    executor: E,
    author_id: i64,
    title: &str,
    body: &str,
) -> Result<i64, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    sqlx::query_scalar!(
        "insert into threads (author_id, title, body) values ($1, $2, $3) returning id",
        author_id,
        title,
        body,
    )
    .fetch_one(executor)
    .await
}

/// P03スレッド一覧に表示する最小限の行。
/// **AC09-2が要求する項目(コメント数・本文冒頭のみの抜粋)の全ては含まない**。
/// F09の範囲(ソート・ページネーション・空状態表示・本文の抜粋整形)を先取りしない、
/// というこのサイクルのスコープ判断による(AC05-3を満たすための最小拡張)。
pub struct ThreadListRow {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub author_display_name: String,
    pub created_at: sqlx::types::time::OffsetDateTime,
}

/// 全スレッドを作成日時の降順(decision 0009の初期表示順)で返す。
/// ページネーションは掛けない(F09の範囲外、上記struct docコメント参照)。
pub async fn list_all<'e, E>(executor: E) -> Result<Vec<ThreadListRow>, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    sqlx::query_as!(
        ThreadListRow,
        r#"
        select threads.id, threads.title, threads.body,
               users.display_name as author_display_name, threads.created_at
        from threads
        join users on users.id = threads.author_id
        order by threads.created_at desc, threads.id desc
        "#
    )
    .fetch_all(executor)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    async fn insert_test_user(pool: &PgPool, unique_id: &str, display_name: &str) -> i64 {
        sqlx::query_scalar!(
            "insert into users (unique_id, password_hash, display_name) values ($1, $2, $3) returning id",
            unique_id,
            "hash",
            display_name,
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[sqlx::test]
    async fn insert_returns_a_fresh_id(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let id = insert(&pool, uid, "タイトル", "本文").await.unwrap();
        assert!(id > 0);
    }

    #[sqlx::test]
    async fn insert_persists_title_and_body_verbatim(pool: PgPool) {
        // C-05: 保存時点の値がそのまま入ること(トリム等はdomain層の責務でここでは検証しない)。
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        insert(&pool, uid, "AI駆動開発の未来について", "本文です")
            .await
            .unwrap();

        let rows = list_all(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "AI駆動開発の未来について");
        assert_eq!(rows[0].body, "本文です");
        assert_eq!(rows[0].author_display_name, "テストユーザー01");
    }

    #[sqlx::test]
    async fn list_all_orders_by_created_at_desc(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let first = insert(&pool, uid, "最初のスレッド", "本文1").await.unwrap();
        let second = insert(&pool, uid, "次のスレッド", "本文2").await.unwrap();

        let rows = list_all(&pool).await.unwrap();
        assert_eq!(rows.len(), 2);
        // decision 0009: 初期表示は作成日時降順 ＝ 新しい方が先。
        assert_eq!(rows[0].id, second);
        assert_eq!(rows[1].id, first);
    }

    #[sqlx::test]
    async fn list_all_is_empty_for_empty_db(pool: PgPool) {
        let rows = list_all(&pool).await.unwrap();
        assert!(rows.is_empty());
    }
}
