//! コメントの永続化(F07コメント作成・F08コメント削除・F10スレッド詳細表示、
//! issues/07・issues/08・issues/10)。
//!
//! `insert`はF07(web/thread_detail.rs)が使う。`find_ownership`/`delete`はF08が使う
//! ―― `formal/Bbs/Op.lean`の`deleteComment`(`findComment`→作成者検査→未削除検査→
//! `modify`)に対応し、認可判定に必要な列(`author_id`・`deleted`)だけを返す
//! `find_ownership`と、実際に論理削除する`delete`とを分けている
//! (Action/Calculationの分離: 判定はweb層、書き込みはここ)。

use sqlx::PgExecutor;

/// 新規コメントを1件挿入し、採番されたIDを返す。
///
/// `executor`は`&PgPool`にも`&mut Transaction`にも当てはまるようジェネリックに
/// してある(decision 0002: web/thread_detail.rsのハンドラは`db::with_transaction`で
/// 開いたトランザクション越しにこれを呼ぶ、`db/threads.rs::insert`と同じ形)。
pub async fn insert<'e, E>(
    executor: E,
    thread_id: i64,
    author_id: i64,
    body: &str,
) -> Result<i64, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    sqlx::query_scalar!(
        "insert into comments (thread_id, author_id, body) values ($1, $2, $3) returning id",
        thread_id,
        author_id,
        body,
    )
    .fetch_one(executor)
    .await
}

/// スレッド詳細のコメント一覧1件ぶんの行。本文は削除済みでも生の値をそのまま返す
/// ―― 固定文言(`＜このコメントは削除されました＞`、C-01)への差し替えは
/// `domain::query::render_comment_body`(呼び出し元のweb層)の責務であり、
/// DB層は「何が起きたか」の事実(Data)を渡すだけにとどめる。
/// `id`・`author_id`はF08で追加(削除リンクのURL生成・「自分のコメントか」の判定に
/// web層が必要とする、S1調査で判明した不足分)。
pub struct CommentRow {
    pub id: i64,
    pub author_id: i64,
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
        select comments.id,
               comments.author_id,
               users.display_name as author_display_name,
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

/// F08(コメント削除)の認可判定に要る最小限の列。本文は不要(削除は本文を
/// 一切書き換えないため、C-07)。`formal/Bbs/Op.lean`の`findComment`に対応する
/// 読み取りで、`thread_id`は削除ハンドラがURLの`thread_id`セグメントとの整合を
/// 検査する(不一致は`comments/{thread_id}`のネスト構造が壊れているためC-10の404)のに使う。
pub struct CommentOwnership {
    pub thread_id: i64,
    pub author_id: i64,
    pub deleted: bool,
}

/// IDでコメント1件の認可判定用の列を取得する。存在しない場合は`None`
/// (`findComment`が`none`なら`notFound`で失敗することに対応、C-10)。
pub async fn find_ownership<'e, E>(
    executor: E,
    comment_id: i64,
) -> Result<Option<CommentOwnership>, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    sqlx::query_as!(
        CommentOwnership,
        r#"
        select thread_id, author_id, (deleted_at is not null) as "deleted!"
        from comments
        where id = $1
        "#,
        comment_id
    )
    .fetch_optional(executor)
    .await
}

/// コメントを論理削除し、**この呼び出しが実際に削除したかどうか**を返す
/// (C-07: 行は残し`deleted_at`を立てるのみ、本文は保持する)。
/// `true` = このトランザクションが未削除→削除済みへ遷移させた、
/// `false` = 既に削除済みだった(AC08-4の再削除)。
///
/// **`where`に`deleted_at is null`を含めるのが本質**(F08レビュー指摘のTOCTOU修正)。
/// 以前は`where id = $1`だけで、未削除であることの確認は呼び出し側の
/// `find_ownership`に委ねていたが、`find_ownership`と`delete`の間には行ロックが無く、
/// 同一コメントへの同時削除が**双方とも**`!deleted`検査を通過しうる。その場合
/// 両方が「削除しました」を返し、片方が`AlreadyDeleted`にならない
/// (認可バイパスは無く結果も冪等だが、画面上のフィードバックが誤る)。
/// 判定を`update`の`where`と`rows_affected`に一本化することで、
/// 「未削除であることの確認」と「削除」を1文=1原子操作にまとめ、この窓を閉じる。
/// 呼び出し側の`find_ownership`による事前検査は早期リターンのための最適化に
/// 格下げし、**真偽の決定権はこの関数の戻り値が持つ**。
///
/// F06(スレッド削除)も同じ形を踏襲できる ―― 「条件を満たすときだけ書き換える
/// `update`/`delete`を1文で撃ち、`rows_affected`で成否を判定する」であり、
/// スレッド削除なら`delete from threads where id = $1 and not exists (コメント)`が
/// 同じ役割を果たす(条件検査と削除の間に他トランザクションが割り込めない)。
pub async fn delete<'e, E>(executor: E, comment_id: i64) -> Result<bool, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    let result = sqlx::query!(
        "update comments set deleted_at = now() where id = $1 and deleted_at is null",
        comment_id
    )
    .execute(executor)
    .await?;
    Ok(result.rows_affected() > 0)
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
    async fn insert_returns_a_fresh_id(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = crate::db::threads::insert(&pool, uid, "タイトル", "本文")
            .await
            .unwrap();

        let cid = insert(&pool, tid, uid, "コメント本文").await.unwrap();
        assert!(cid > 0);
    }

    #[sqlx::test]
    async fn insert_persists_body_verbatim_and_is_not_deleted(pool: PgPool) {
        // C-05: 保存時点の値がそのまま入ること(トリム等はdomain層の責務でここでは検証しない)。
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = crate::db::threads::insert(&pool, uid, "タイトル", "本文")
            .await
            .unwrap();

        insert(&pool, tid, uid, "コメント本文です").await.unwrap();

        let rows = list_by_thread(&pool, tid).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].body, "コメント本文です");
        assert_eq!(rows[0].author_display_name, "テストユーザー01");
        assert!(!rows[0].deleted);
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

    /// F08: 削除ボタンのURL生成・所有者判定に要る`id`・`author_id`が返ること。
    #[sqlx::test]
    async fn list_by_thread_includes_id_and_author_id(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = crate::db::threads::insert(&pool, uid, "タイトル", "本文")
            .await
            .unwrap();
        let cid = insert(&pool, tid, uid, "コメント本文").await.unwrap();

        let rows = list_by_thread(&pool, tid).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, cid);
        assert_eq!(rows[0].author_id, uid);
    }

    /// F08: `find_ownership`は作成者id・スレッドid・削除済みフラグを返す。
    #[sqlx::test]
    async fn find_ownership_returns_the_matching_comment(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = crate::db::threads::insert(&pool, uid, "タイトル", "本文")
            .await
            .unwrap();
        let cid = insert(&pool, tid, uid, "コメント本文").await.unwrap();

        let ownership = find_ownership(&pool, cid).await.unwrap().unwrap();
        assert_eq!(ownership.thread_id, tid);
        assert_eq!(ownership.author_id, uid);
        assert!(!ownership.deleted);
    }

    /// C-10: 存在しないコメントIDは`None`(呼び出し側で404に倒す)。
    #[sqlx::test]
    async fn find_ownership_returns_none_for_a_nonexistent_id(pool: PgPool) {
        let ownership = find_ownership(&pool, 999_999).await.unwrap();
        assert!(ownership.is_none());
    }

    /// F08: `find_ownership`は削除済みかどうかを正しく反映する。
    #[sqlx::test]
    async fn find_ownership_reflects_deleted_state(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = crate::db::threads::insert(&pool, uid, "タイトル", "本文")
            .await
            .unwrap();
        let t0 = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        insert_comment(&pool, tid, uid, "削除済み", t0, true).await;
        let cid: i64 = sqlx::query_scalar("select id from comments where thread_id = $1")
            .bind(tid)
            .fetch_one(&pool)
            .await
            .unwrap();

        let ownership = find_ownership(&pool, cid).await.unwrap().unwrap();
        assert!(ownership.deleted);
    }

    /// C-07: `delete`は行を消さず`deleted_at`を立てるのみ。本文は保持される
    /// (固定文言への差し替えは呼び出し元の責務、C-01)。
    #[sqlx::test]
    async fn delete_marks_deleted_but_keeps_the_row_and_body(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = crate::db::threads::insert(&pool, uid, "タイトル", "本文")
            .await
            .unwrap();
        let cid = insert(&pool, tid, uid, "削除される本文").await.unwrap();

        assert!(
            delete(&pool, cid).await.unwrap(),
            "未削除のコメントを削除したので`true`(実際に遷移させた)"
        );

        let rows = list_by_thread(&pool, tid).await.unwrap();
        assert_eq!(rows.len(), 1, "行そのものは消えない(C-07)");
        assert!(rows[0].deleted);
        assert_eq!(
            rows[0].body, "削除される本文",
            "元の本文はDB上に保持される(C-07、固定文言への差し替えは呼び出し元の責務)"
        );
    }

    /// AC08-4 / TOCTOU修正(F08レビュー指摘): 既に削除済みのコメントへの`delete`は
    /// `false`を返す。`where deleted_at is null`により2度目の`update`が0行になることが
    /// 根拠で、これが「同時削除で双方が成功を返す」ことを防ぐ仕組みそのもの
    /// ―― 事前検査(`find_ownership`)を通り抜けた2つ目のトランザクションも、
    /// ここで`false`を受け取って`AlreadyDeleted`へ倒れる。
    #[sqlx::test]
    async fn delete_returns_false_when_the_comment_is_already_deleted(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = crate::db::threads::insert(&pool, uid, "タイトル", "本文")
            .await
            .unwrap();
        let cid = insert(&pool, tid, uid, "削除される本文").await.unwrap();

        assert!(delete(&pool, cid).await.unwrap(), "1回目は実際に削除する");
        assert!(
            !delete(&pool, cid).await.unwrap(),
            "2回目は0行更新なので`false`(AC08-4)"
        );

        // 冪等性: 2回目の呼び出しは`deleted_at`を上書きしない(C-08 削除は不可逆・非破壊)。
        let rows = list_by_thread(&pool, tid).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].deleted);
        assert_eq!(rows[0].body, "削除される本文");
    }

    /// TOCTOU修正の核心: **2つの並行トランザクション**が同じ未削除コメントに対して
    /// 同時に削除を試みても、`update ... where deleted_at is null`により
    /// 「実際に削除した」と判定されるのは片方だけになる。
    /// 修正前(`where id = $1`のみ)はこのテストで両方が`rows_affected = 1`を返し、
    /// 双方が「削除しました」を表示していた。
    #[sqlx::test]
    async fn concurrent_deletes_report_success_only_once(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = crate::db::threads::insert(&pool, uid, "タイトル", "本文")
            .await
            .unwrap();
        let cid = insert(&pool, tid, uid, "削除される本文").await.unwrap();

        // 2本のトランザクションを開き、片方をコミットしてからもう片方が撃つ。
        // (同時に撃つと後発が行ロックで待ち、先発のコミット後に0行を見る ――
        //  待ちを挟まず決定的に同じ状態を作るため、逐次のコミット順で再現する。)
        let mut tx1 = pool.begin().await.unwrap();
        let mut tx2 = pool.begin().await.unwrap();

        let first = delete(&mut *tx1, cid).await.unwrap();
        tx1.commit().await.unwrap();

        let second = delete(&mut *tx2, cid).await.unwrap();
        tx2.commit().await.unwrap();

        assert!(first, "先に削除を確定させた側は`true`");
        assert!(!second, "競り負けた側は`false`(AlreadyDeletedへ倒れる)");
    }
}
