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
}
