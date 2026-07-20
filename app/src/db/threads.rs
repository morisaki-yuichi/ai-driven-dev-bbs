//! スレッドの永続化(F05スレッド作成・F09スレッド一覧・F06スレッド削除)。
//!
//! `delete`はF06(スレッド削除、issues/06)が使う。`formal/Bbs/Op.lean`の
//! `deleteThread`(`findThread`→作成者検査→`commentsOf`/未削除込みの空検査→
//! `modify`)に対応し、`db/comments.rs::delete`のTOCTOU対策(条件付き1文+
//! `rows_affected`判定)と同じ形を踏襲する。ただし所有者チェックはこの1文に
//! 含めない ―― スレッドの所有権は作成後変わらない(譲渡機能なし)のでレース対象では
//! なく、コメント有無だけがレース対象という非対称があるため(web層が事前に
//! 所有者検査を行い、この関数はコメント有無の検査のみを原子的に行う)。
//!
//! **F08のパターンはそのままでは移植できない**(F06レビューで実測): F08の条件は
//! 更新対象の行自身に付くので1文で閉じるが、F06の条件は別テーブル(`comments`)を
//! 見るため`EvalPlanQual`の再評価が効かず、同時挿入を取りこぼしてFK違反(500)になる。
//! そのため`delete`の前に`find_by_id_for_update`で対象行を排他ロックする
//! (詳細は同関数のdocコメント)。コメント作成側は`find_by_id_for_share`で対になる
//! 共有ロックを取り、両者を直列化する。
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

/// P04スレッド詳細(F10)に表示する1件。一覧(`ThreadListRow`)と異なり、
/// コメント数・最終更新日時は持たない(詳細画面はコメント自体を列挙するので不要、
/// ユーザー承認済みのスコープ)。
///
/// `author_id`はF06(スレッド削除)が追加した。「自分のスレッドか」の判定
/// (削除ボタンの表示可否・POST時の認可検査)に必要な数値ID
/// (`author_display_name`は表示用で、同姓同名がありうるため認可判定には使えない)。
pub struct ThreadDetailRow {
    pub author_id: i64,
    pub title: String,
    pub body: String,
    pub author_display_name: String,
    pub created_at: sqlx::types::time::OffsetDateTime,
}

/// IDでスレッドを1件取得する。存在しない場合は`None`。
///
/// decision 0014によりスレッド削除は物理削除なので、`threads`に行が無いことは
/// 「存在しない」「削除済み」のどちらも意味する ―― 呼び出し側(`web/thread_detail.rs`)は
/// `None`を一律`DomainError::NotFound`(C-10)に倒せばよく、削除フラグを別途見る必要が無い。
pub async fn find_by_id<'e, E>(executor: E, id: i64) -> Result<Option<ThreadDetailRow>, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    sqlx::query_as!(
        ThreadDetailRow,
        r#"
        select threads.author_id, threads.title, threads.body, threads.created_at,
               users.display_name as author_display_name
        from threads
        join users on users.id = threads.author_id
        where threads.id = $1
        "#,
        id
    )
    .fetch_optional(executor)
    .await
}

/// `find_by_id`と同じ行を返しつつ、**そのスレッド行を`for update`で排他ロックする**。
/// F06(スレッド削除)の`delete`を呼ぶ前に必ずこれを通す。
///
/// **なぜ必要か(レビューで実測した不具合)**: `delete`の`where`に`not exists`を
/// 置くだけでは同時挿入との競合を防げない。PostgresのREAD COMMITTEDでは、
/// `not exists`サブクエリは**その文の開始時点のスナップショット**で評価される。
/// 別トランザクションが未コミットのコメントを持っている場合、サブクエリはそれを
/// 見ずに「0件」と判定し、`threads`行のロック取得で待たされ、相手がコミットした後に
/// 削除へ進む。`EvalPlanQual`による条件再評価は**更新された対象行**に対して働くもので、
/// 別テーブル(`comments`)を見るサブクエリは再評価されない ―― 結果、削除が実行され、
/// 文末のFK検査(`comments_thread_id_fkey`、`on delete`指定なし＝`no action`)が
/// 初めてこれを捕まえて`23503`を投げる。孤児コメントは生じないが、ユーザーには
/// 「コメントがあるので削除できません」ではなく**500**が返る。
///
/// 先にこのロックを取れば、コメント挿入側(FKが`for key share`を取る)は待たされ、
/// ロック取得後の`delete`文は新しいスナップショットでコメントを見て0行になる。
///
/// **F08(`db/comments.rs::delete`)との違い**: あちらは`update comments ... where
/// id = $1 and deleted_at is null`で、条件が**更新対象の行自身**に付いているため
/// `EvalPlanQual`の再評価が効き、1文だけで競合が閉じる。F06は条件が別テーブルに
/// あるためこのパターンがそのまま移植できない ―― 「条件付き1文＋`rows_affected`」の
/// 形は保ちつつ、行ロックで条件の安定性を別途担保する。
pub async fn find_by_id_for_update<'e, E>(
    executor: E,
    id: i64,
) -> Result<Option<ThreadDetailRow>, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    sqlx::query_as!(
        ThreadDetailRow,
        r#"
        select threads.author_id, threads.title, threads.body, threads.created_at,
               users.display_name as author_display_name
        from threads
        join users on users.id = threads.author_id
        where threads.id = $1
        for update of threads
        "#,
        id
    )
    .fetch_optional(executor)
    .await
}

/// `find_by_id`と同じ行を返しつつ、スレッド行を`for share`で共有ロックする。
/// F07(コメント作成)の存在検査に使う。
///
/// `for share`は`find_by_id_for_update`の`for update`と衝突するので、スレッド削除と
/// コメント作成が正しく直列化される:
/// - 削除が先にロックを取った場合、この検査は待たされ、削除確定後は行が消えているので
///   `None` ＝ C-10の404になる(FK違反による500にならない)。
/// - この検査が先にロックを取った場合、削除側の`for update`が待たされ、コメント確定後に
///   `delete`の`not exists`がそれを見て0行 ＝ `has_comments`になる。
///
/// ロックを取らない素の`select`ではどちらも成立しない ―― スナップショットで
/// 「スレッドは在る」と読んだ後に削除が確定し、`insert`のFK検査が`23503`を投げる
/// (トランザクション内に読み取りを移すだけでは閉じない。`select`は行をロックしないため)。
pub async fn find_by_id_for_share<'e, E>(
    executor: E,
    id: i64,
) -> Result<Option<ThreadDetailRow>, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    sqlx::query_as!(
        ThreadDetailRow,
        r#"
        select threads.author_id, threads.title, threads.body, threads.created_at,
               users.display_name as author_display_name
        from threads
        join users on users.id = threads.author_id
        where threads.id = $1
        for share of threads
        "#,
        id
    )
    .fetch_optional(executor)
    .await
}

/// F06(スレッド削除)。**このスレッドが実際に削除されたかどうか**を返す。
/// `true` = このトランザクションが実際に削除した、
/// `false` = コメントが1件以上あった(削除済みコメントも数える、AC06-2)ため
/// 削除しなかった、あるいは対象IDが既に存在しなかった。
///
/// **呼び出し前に`find_by_id_for_update`で対象行をロックしていること**が前提。
/// `where`の`not exists (コメントの有無)`だけでは同時挿入との競合を閉じられない
/// (理由と実測は`find_by_id_for_update`のdocコメント参照)。ロック無しでこの関数を
/// 呼ぶと、競合時に`Ok(false)`ではなくFK違反(`23503`)の`Err`が返り、画面には
/// 500が出る。
///
/// サブクエリを`deleted_at is null`等で絞らないこと ―― `formal/Bbs/Op.lean`の
/// `commentsOf`が削除済みも含めて数えることに対応する(AC06-2: 削除済みコメントが
/// あるスレッドも削除できない)。
///
/// **所有者チェックはこの文に含めない**(このファイル冒頭のdocコメント参照)。
/// 呼び出し側(`web/thread_detail.rs`)が事前に`author_id`を検査し、他人のスレッドは
/// この関数を呼ぶ前に`Forbidden`へ倒す。
pub async fn delete<'e, E>(executor: E, thread_id: i64) -> Result<bool, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    let result = sqlx::query!(
        r#"
        delete from threads
        where id = $1
          and not exists (select 1 from comments where comments.thread_id = $1)
        "#,
        thread_id
    )
    .execute(executor)
    .await?;
    Ok(result.rows_affected() > 0)
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

    /// F10(スレッド詳細): IDで1件取得でき、作成者の表示名まで解決されている。
    /// F06: `author_id`(認可判定用の数値ID)も返す。
    #[sqlx::test]
    async fn find_by_id_returns_the_matching_thread(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = insert(&pool, uid, "タイトル", "本文").await.unwrap();

        let row = find_by_id(&pool, tid).await.unwrap().unwrap();
        assert_eq!(row.title, "タイトル");
        assert_eq!(row.body, "本文");
        assert_eq!(row.author_display_name, "テストユーザー01");
        assert_eq!(row.author_id, uid);
    }

    /// C-10/decision 0014: 存在しないIDは`None`(呼び出し側で404に倒す)。
    /// スレッド削除は物理削除なので、削除済みも同じ`None`経路を通る。
    #[sqlx::test]
    async fn find_by_id_returns_none_for_a_nonexistent_id(pool: PgPool) {
        let row = find_by_id(&pool, 999_999).await.unwrap();
        assert!(row.is_none());
    }

    /// F06/AC06-1/decision 0014: コメント0件のスレッドは実際に(物理)削除される。
    #[sqlx::test]
    async fn delete_removes_a_thread_that_has_no_comments(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = insert(&pool, uid, "タイトル", "本文").await.unwrap();

        assert!(
            delete(&pool, tid).await.unwrap(),
            "コメント0件のスレッドは削除できる"
        );
        assert!(find_by_id(&pool, tid).await.unwrap().is_none());
    }

    /// F06/AC06-2/C-06: 未削除コメントが1件でもあれば削除しない。
    #[sqlx::test]
    async fn delete_does_not_remove_a_thread_that_has_a_comment(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = insert(&pool, uid, "タイトル", "本文").await.unwrap();
        insert_comment(&pool, tid, uid, "c1", far_future(), false).await;

        assert!(
            !delete(&pool, tid).await.unwrap(),
            "コメントがあるスレッドは削除できない"
        );
        assert!(
            find_by_id(&pool, tid).await.unwrap().is_some(),
            "削除されずに残っているはず"
        );
    }

    /// F06/AC06-2: **削除済みコメントだけ**でも削除を阻む。`commentsOf`(Op.lean)が
    /// `deleted`で絞らないことに対応する ―― `where`のサブクエリを
    /// `deleted_at is null`で絞ってはならない、という実装上の要点そのものを
    /// 検証する回帰テスト。
    #[sqlx::test]
    async fn delete_does_not_remove_a_thread_that_has_only_a_deleted_comment(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = insert(&pool, uid, "タイトル", "本文").await.unwrap();
        insert_comment(&pool, tid, uid, "削除済みコメント", far_future(), true).await;

        assert!(
            !delete(&pool, tid).await.unwrap(),
            "削除済みコメントだけでも削除を阻むはず(AC06-2)"
        );
        assert!(find_by_id(&pool, tid).await.unwrap().is_some());
    }

    /// C-10: 存在しないIDへの削除は`false`(呼び出し側で404に倒す。ここでは
    /// エラーにならず0行更新で終わることだけを確認する)。
    #[sqlx::test]
    async fn delete_returns_false_for_a_nonexistent_thread_id(pool: PgPool) {
        assert!(!delete(&pool, 999_999).await.unwrap());
    }

    /// TOCTOU対策の核心: 削除しようとしている最中に**別トランザクションが
    /// コメントを挿入してコミットした**場合、後から`delete`を確定させる側は
    /// その新しいコメントを見て削除を拒否する。「コメント0件の確認」と「削除」を
    /// 呼び出し側で2文に分けていたら、確認時点では0件だったコメントが確認後に
    /// 増えても削除が実行されてしまう窓ができる ―― `not exists`を`delete`と
    /// 同じ1文に含めることで、実際の削除時点の状態を見て判定させ、この窓を閉じる。
    #[sqlx::test]
    async fn delete_is_blocked_by_a_comment_inserted_by_a_concurrent_transaction(pool: PgPool) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = insert(&pool, uid, "タイトル", "本文").await.unwrap();

        // 削除側のトランザクションを先に開始しておく(コメント挿入より前)。
        // それでも`delete`文自体はまだ実行していない。
        let mut tx_delete = pool.begin().await.unwrap();

        // 別トランザクション(ここでは`pool`から直接、即コミット)が
        // 割り込んでコメントを挿入する。
        insert_comment(&pool, tid, uid, "割り込みコメント", far_future(), false).await;

        // その後で`delete`文を実行すると、コミット済みの新しいコメントを見て拒否する
        // (PostgresのデフォルトはREAD COMMITTEDなので、トランザクション開始時点
        // ではなく各文の実行時点でコミット済みの変更が見える)。
        let deleted = delete(&mut *tx_delete, tid).await.unwrap();
        tx_delete.commit().await.unwrap();

        assert!(
            !deleted,
            "delete実行時点でコメントが存在するので削除されてはならない"
        );
        assert!(find_by_id(&pool, tid).await.unwrap().is_some());
    }

    /// **真の競合**(上の`..._by_a_concurrent_transaction`は挿入が先にコミット済みという
    /// 逐次ケースにすぎない)の回帰テスト。挿入側が**未コミットのまま**削除側と重なる場合、
    /// `not exists`はスナップショット評価なのでコメントを見落とす ―― `for update`で
    /// 先に行をロックしていないと、削除が実行されて文末のFK検査が`23503`を投げ、
    /// `Ok(false)`ではなく`Err`(＝画面上は500)になる。
    /// `find_by_id_for_update`を外すとこのテストは失敗する。
    #[sqlx::test]
    async fn delete_is_graceful_when_a_comment_is_inserted_by_an_overlapping_transaction(
        pool: PgPool,
    ) {
        let uid = insert_test_user(&pool, "testuser_01", "テストユーザー01").await;
        let tid = insert(&pool, uid, "タイトル", "本文").await.unwrap();

        // 挿入側: コメントを入れるがまだコミットしない(threads行にfor key shareを保持)。
        let mut tx_insert = pool.begin().await.unwrap();
        sqlx::query!(
            "insert into comments (thread_id, author_id, body, created_at) values ($1, $2, $3, $4)",
            tid,
            uid,
            "未コミットのコメント",
            far_future(),
        )
        .execute(&mut *tx_insert)
        .await
        .unwrap();

        // 削除側: ロック→削除。挿入側のロックで待たされる。
        let pool2 = pool.clone();
        let deleter = tokio::spawn(async move {
            let mut tx = pool2.begin().await.unwrap();
            let _t = find_by_id_for_update(&mut *tx, tid).await.unwrap();
            let deleted = delete(&mut *tx, tid).await;
            if deleted.is_ok() {
                tx.commit().await.unwrap();
            }
            deleted
        });

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            !deleter.is_finished(),
            "削除側は挿入側のロックでブロックしているはず"
        );
        tx_insert.commit().await.unwrap();

        let deleted = deleter
            .await
            .unwrap()
            .expect("FK違反のErrではなくOkが返るべき(500にしない)");
        assert!(!deleted, "コミットされたコメントを見て削除を拒否するはず");
        assert!(find_by_id(&pool, tid).await.unwrap().is_some());
    }
}
