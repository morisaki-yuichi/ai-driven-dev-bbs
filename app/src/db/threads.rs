//! スレッドの永続化(F05スレッド作成・F09スレッド一覧)。
//!
//! `insert`はF05(web/thread_create.rs)が使う。`list_all`はP03スレッド一覧
//! (web/thread_list.rs)が使う読み取りで、F09が要求する表示項目のうち
//! コメント数・最終更新日時（decision 0010）まで含めて返す。ページネーション
//! (LIMIT/OFFSET)はここに持ち込まない——全件取得して`domain::query::paginate`
//! (純粋関数)に渡す方針（ユーザー承認済みのスコープ）。ソート・検索(F11〜F12)は
//! 引き続き範囲外。

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

/// P03スレッド一覧に表示する1件ぶんの行。AC09-2が要求する項目
/// (タイトル・本文・作成日時・作成者・コメント数・最終更新日時)を過不足なく持つ。
/// 本文の冒頭抜粋整形(任意要件)・ページネーションはここでは行わない
/// (前者は表示側、後者は`domain::query::paginate`の責務)。
pub struct ThreadListRow {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub author_display_name: String,
    pub created_at: sqlx::types::time::OffsetDateTime,
    /// D13: 削除済みコメントも数える(C-16とソート基準を揃える解釈、decision 0010)。
    pub comment_count: i64,
    /// C-15/decision 0010: `GREATEST(スレッド作成日時, 全コメント作成日時の最大)`。
    /// 削除済みコメントも計算に含める(投稿された事実は消えない)ので、
    /// コメント削除ではこの値は動かない(単調性が保たれる)。
    pub last_updated_at: sqlx::types::time::OffsetDateTime,
}

/// 全スレッドを作成日時の降順(decision 0009の初期表示順)で返す。
/// ページネーションは掛けない(全件取得して`domain::query::paginate`に渡す方針。
/// 上記struct docコメント参照)。
pub async fn list_all<'e, E>(executor: E) -> Result<Vec<ThreadListRow>, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    sqlx::query_as!(
        ThreadListRow,
        r#"
        select threads.id, threads.title, threads.body,
               users.display_name as author_display_name, threads.created_at,
               count(comments.id) as "comment_count!",
               greatest(
                   threads.created_at,
                   coalesce(max(comments.created_at), threads.created_at)
               ) as "last_updated_at!"
        from threads
        join users on users.id = threads.author_id
        left join comments on comments.thread_id = threads.id
        group by threads.id, users.display_name
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

    /// F07(コメント作成)実装前なので、`comments`テーブルへは直接INSERTする
    /// (このテストファイル自身が`insert_test_user`で`users`に直接INSERTしているのと同じ扱い)。
    /// `created_at`を明示的な未来時刻に固定し、`now()`のタイミング差による
    /// テストのフレーク化を避ける。
    fn far_future() -> sqlx::types::time::OffsetDateTime {
        // 2099-01-01T00:00:00Z。実行環境の実時計がいつであっても確実に未来。
        sqlx::types::time::OffsetDateTime::from_unix_timestamp(4_070_908_800).unwrap()
    }

    async fn insert_comment(
        pool: &PgPool,
        thread_id: i64,
        author_id: i64,
        body: &str,
        created_at: sqlx::types::time::OffsetDateTime,
        deleted: bool,
    ) {
        if deleted {
            sqlx::query!(
                "insert into comments (thread_id, author_id, body, created_at, deleted_at) \
                 values ($1, $2, $3, $4, $4)",
                thread_id,
                author_id,
                body,
                created_at,
            )
            .execute(pool)
            .await
            .unwrap();
        } else {
            sqlx::query!(
                "insert into comments (thread_id, author_id, body, created_at) values ($1, $2, $3, $4)",
                thread_id,
                author_id,
                body,
                created_at,
            )
            .execute(pool)
            .await
            .unwrap();
        }
    }

    #[sqlx::test]
    async fn list_all_comment_count_is_zero_without_comments(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        insert(&pool, uid, "タイトル", "本文").await.unwrap();

        let rows = list_all(&pool).await.unwrap();
        assert_eq!(rows[0].comment_count, 0);
    }

    /// D13/C-16(decision 0010): 一覧のコメント数は削除済みも数える。
    #[sqlx::test]
    async fn list_all_comment_count_includes_deleted_comments(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = insert(&pool, uid, "タイトル", "本文").await.unwrap();
        insert_comment(&pool, tid, uid, "c1", far_future(), false).await;
        insert_comment(&pool, tid, uid, "c2", far_future(), true).await;

        let rows = list_all(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].comment_count, 2);
    }

    /// C-15/decision 0010: コメントが無ければ最終更新日時はスレッド作成日時と一致する。
    #[sqlx::test]
    async fn list_all_last_updated_at_defaults_to_thread_created_at_without_comments(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        insert(&pool, uid, "タイトル", "本文").await.unwrap();

        let rows = list_all(&pool).await.unwrap();
        assert_eq!(rows[0].last_updated_at, rows[0].created_at);
    }

    /// AC09-4/decision 0010: 最終更新日時はコメントの作成日時まで進む。
    #[sqlx::test]
    async fn list_all_last_updated_at_reflects_latest_comment_time(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = insert(&pool, uid, "タイトル", "本文").await.unwrap();
        insert_comment(&pool, tid, uid, "c1", far_future(), false).await;

        let rows = list_all(&pool).await.unwrap();
        assert_eq!(rows[0].last_updated_at, far_future());
    }

    /// decision 0010 決定2/3: 削除済みコメントの投稿時刻も最終更新日時の計算に含める
    /// (投稿された事実は消えない ＝ 唯一のコメントを削除しても値が過去に巻き戻らない)。
    #[sqlx::test]
    async fn list_all_last_updated_at_still_reflects_a_deleted_comment(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = insert(&pool, uid, "タイトル", "本文").await.unwrap();
        insert_comment(&pool, tid, uid, "c1", far_future(), true).await;

        let rows = list_all(&pool).await.unwrap();
        assert_eq!(rows[0].last_updated_at, far_future());
    }
}
