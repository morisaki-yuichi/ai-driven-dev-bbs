//! ユーザーの永続化(F01登録)。
//!
//! ユニークID重複はDBの`unique`制約に任せる(事前SELECTしてからINSERTすると
//! 競合するリクエスト間でTOCTOUが起こりうるため)。Postgresのunique_violation
//! (SQLSTATE 23505)を`is_unique_violation`で判別し、呼び出し側(ハンドラ)が
//! `domain::model::Error::DuplicateUniqueId`へ写像する。

/// 新規ユーザーを1件挿入し、採番されたIDを返す。
///
/// `executor`は`&PgPool`(単発クエリ)にも`&mut Transaction`(呼び出し元が開いた
/// トランザクション)にも当てはまるようにジェネリックにしてある(decision 0002:
/// register.rsのハンドラは`db::with_transaction`で開いたトランザクション越しに
/// これを呼ぶ)。
pub async fn insert<'e, E>(
    executor: E,
    unique_id: &str,
    password_hash: &str,
    display_name: &str,
) -> Result<i64, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query_scalar!(
        "insert into users (unique_id, password_hash, display_name) values ($1, $2, $3) returning id",
        unique_id,
        password_hash,
        display_name,
    )
    .fetch_one(executor)
    .await
}

/// F04プロフィール編集(issue 04): ログイン中のユーザー自身の表示名を更新する。
/// `user_id`はセッションから解決した認証済みユーザーのID(`db::sessions::AuthenticatedUser`)
/// であり、他人のIDを渡す経路がそもそも存在しない(URLにIDを含まない、C-09のガード配下)。
/// `formal/Bbs/Op.lean`の`updateDisplayName`(`users.map`による対象ユーザーのみの書き換え)に
/// 対応する。
///
/// 戻り値は`rows_affected`を見て対象ユーザーが実在したかを表す(`true`=更新した、
/// `false`=該当する`id`が無く何も書き換えなかった)。現状の唯一の呼び出し元
/// (`web/profile.rs::submit`)は`require_auth`配下で`user_id`が必ず実在するため
/// 常に`true`になり、戻り値を見ずに捨てている(ユーザーに見える振る舞いは変えない)。
/// 将来別経路から呼ばれたときに「存在しないidでもOk(())」で沈黙しないためのガード。
pub async fn update_display_name<'e, E>(
    executor: E,
    user_id: i64,
    display_name: &str,
) -> Result<bool, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    let result = sqlx::query!(
        "update users set display_name = $1 where id = $2",
        display_name,
        user_id,
    )
    .execute(executor)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// PostgreSQLのunique_violation(23505)かどうかを判定する。
pub fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(db_error) if db_error.code().as_deref() == Some("23505")
    )
}

/// F02ログインの認証情報照合に使う最小限の列。
pub struct UserCredentials {
    pub id: i64,
    pub password_hash: String,
    pub display_name: String,
}

/// ユニークIDでユーザーを引く。存在しなければ`None`(呼び出し側はこれと
/// パスワード不一致を同一の`InvalidCredentials`に潰す。formal/Bbs/Op.leanの
/// `login`が同じ判断をしている: 列挙攻撃を避けるため区別しない)。
pub async fn find_by_unique_id<'e, E>(
    executor: E,
    unique_id: &str,
) -> Result<Option<UserCredentials>, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query_as!(
        UserCredentials,
        "select id, password_hash, display_name from users where unique_id = $1",
        unique_id
    )
    .fetch_optional(executor)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    #[sqlx::test]
    async fn insert_returns_a_fresh_id(pool: PgPool) {
        let id = insert(&pool, "testuser_01", "hash", "テストユーザー01")
            .await
            .unwrap();
        assert!(id > 0);
    }

    #[sqlx::test]
    async fn insert_duplicate_unique_id_fails_with_unique_violation(pool: PgPool) {
        insert(&pool, "testuser_01", "hash", "テストユーザー01")
            .await
            .unwrap();

        let err = insert(&pool, "testuser_01", "other-hash", "別の表示名")
            .await
            .unwrap_err();
        assert!(is_unique_violation(&err));
    }

    #[sqlx::test]
    async fn insert_allows_duplicate_display_names(pool: PgPool) {
        // C-03: 表示名は重複可能。
        insert(&pool, "testuser_01", "hash", "同じ表示名")
            .await
            .unwrap();
        let second = insert(&pool, "testuser_02", "hash", "同じ表示名").await;
        assert!(second.is_ok());
    }

    #[sqlx::test]
    async fn update_display_name_changes_only_the_target_user(pool: PgPool) {
        let target = insert(&pool, "testuser_01", "hash", "旧表示名")
            .await
            .unwrap();
        insert(&pool, "testuser_02", "hash", "別のユーザー")
            .await
            .unwrap();

        let updated_existing = update_display_name(&pool, target, "新表示名")
            .await
            .unwrap();
        assert!(updated_existing);

        let updated = find_by_unique_id(&pool, "testuser_01")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.display_name, "新表示名");

        // 他のユーザーの表示名は変わらない。
        let unaffected = find_by_unique_id(&pool, "testuser_02")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(unaffected.display_name, "別のユーザー");
    }

    /// レビュー指摘: 存在しない`user_id`を渡してもDBは0行更新のまま`Ok`を返す
    /// (`update users ... where id = $2`が単に何もマッチさせない)。呼び出し側が
    /// `rows_affected`を見れば、対象が存在しなかったことをここで区別できる。
    #[sqlx::test]
    async fn update_display_name_returns_false_when_user_does_not_exist(pool: PgPool) {
        let nonexistent_user_id = 999_999;
        let updated = update_display_name(&pool, nonexistent_user_id, "新表示名")
            .await
            .unwrap();
        assert!(!updated);
    }
}
