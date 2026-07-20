//! コメントの読み取り(F10スレッド詳細表示、issues/10)。
//!
//! F07(コメント作成)・F08(コメント削除)がまだ無いため、挿入用のヘルパはここには
//! 置かない(結合テストは`db/threads.rs`のテストと同じく`comments`テーブルへ
//! 直接INSERTする)。論理削除(C-07)は`deleted_at`の有無で判定する。

use sqlx::PgExecutor;

/// スレッド詳細のコメント一覧1件ぶんの行。本文は削除済みでも生の値をそのまま返す
/// ―― 固定文言(`＜このコメントは削除されました＞`、C-01)への差し替えは
/// `domain::query::render_comment_body`(呼び出し元のweb層)の責務であり、
/// DB層は「何が起きたか」の事実(Data)を渡すだけにとどめる。
pub struct CommentRow {
    pub author_display_name: String,
    pub body: String,
    pub created_at: sqlx::types::time::OffsetDateTime,
    pub deleted: bool,
}

/// スレッド1件ぶんのコメントを作成日時の昇順(idタイブレーク、decision 0009)で返す。
/// 削除済みも含める ―― `formal/Bbs/Query.threadDetail`が`c.deleted`で場合分けせず
/// 全コメントを写すことに対応する(AC10-3: 削除済みでも作成者・日時は表示に残る)。
pub async fn list_by_thread<'e, E>(
    executor: E,
    thread_id: i64,
) -> Result<Vec<CommentRow>, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    sqlx::query_as!(
        CommentRow,
        r#"
        select users.display_name as author_display_name,
               comments.body,
               comments.created_at,
               (comments.deleted_at is not null) as "deleted!"
        from comments
        join users on users.id = comments.author_id
        where comments.thread_id = $1
        order by comments.created_at asc, comments.id asc
        "#,
        thread_id
    )
    .fetch_all(executor)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;
    use sqlx::types::time::OffsetDateTime;

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

    async fn insert_comment(
        pool: &PgPool,
        thread_id: i64,
        author_id: i64,
        body: &str,
        created_at: OffsetDateTime,
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
    async fn list_by_thread_is_empty_without_comments(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = crate::db::threads::insert(&pool, uid, "タイトル", "本文")
            .await
            .unwrap();

        let rows = list_by_thread(&pool, tid).await.unwrap();
        assert!(rows.is_empty());
    }

    /// AC10-3: 削除済みコメントも一覧に残り、本文は生のまま返す
    /// (固定文言への差し替えは呼び出し元の責務、上記docコメント参照)。
    #[sqlx::test]
    async fn list_by_thread_includes_deleted_comments_with_raw_body(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = crate::db::threads::insert(&pool, uid, "タイトル", "本文")
            .await
            .unwrap();
        let t0 = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        insert_comment(&pool, tid, uid, "削除される本文", t0, true).await;

        let rows = list_by_thread(&pool, tid).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].deleted);
        assert_eq!(rows[0].body, "削除される本文");
    }

    /// decision 0009: 作成日時昇順(古い順)で返す ―― 会話の文脈順(F10 issue参照)。
    #[sqlx::test]
    async fn list_by_thread_orders_by_created_at_ascending(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = crate::db::threads::insert(&pool, uid, "タイトル", "本文")
            .await
            .unwrap();
        let t0 = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let t1 = OffsetDateTime::from_unix_timestamp(1_700_000_100).unwrap();
        insert_comment(&pool, tid, uid, "2番目", t1, false).await;
        insert_comment(&pool, tid, uid, "1番目", t0, false).await;

        let rows = list_by_thread(&pool, tid).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].body, "1番目");
        assert_eq!(rows[1].body, "2番目");
    }

    /// 他スレッドのコメントを混ぜない。
    #[sqlx::test]
    async fn list_by_thread_does_not_include_other_threads_comments(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid1 = crate::db::threads::insert(&pool, uid, "スレッド1", "本文")
            .await
            .unwrap();
        let tid2 = crate::db::threads::insert(&pool, uid, "スレッド2", "本文")
            .await
            .unwrap();
        let t0 = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        insert_comment(&pool, tid2, uid, "スレッド2向けコメント", t0, false).await;

        let rows = list_by_thread(&pool, tid1).await.unwrap();
        assert!(rows.is_empty());
    }
}
