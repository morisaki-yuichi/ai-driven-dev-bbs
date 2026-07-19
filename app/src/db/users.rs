//! ユーザーの永続化(F01登録)。
//!
//! ユニークID重複はDBの`unique`制約に任せる(事前SELECTしてからINSERTすると
//! 競合するリクエスト間でTOCTOUが起こりうるため)。Postgresのunique_violation
//! (SQLSTATE 23505)を`is_unique_violation`で判別し、呼び出し側(ハンドラ)が
//! `domain::model::Error::DuplicateUniqueId`へ写像する。

use sqlx::PgPool;

/// 新規ユーザーを1件挿入し、採番されたIDを返す。
/// 1リクエスト1文操作なので、これ自体がすでに1トランザクションとして原子的
/// (decision 0002。formal/Bbs/Invariant.leanの`register_atomic`が示す
/// 「検査を全て通過した最後に一度だけ書き込む」構造に対応する)。
pub async fn insert(
    pool: &PgPool,
    unique_id: &str,
    password_hash: &str,
    display_name: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        "insert into users (unique_id, password_hash, display_name) values ($1, $2, $3) returning id",
        unique_id,
        password_hash,
        display_name,
    )
    .fetch_one(pool)
    .await
}

/// PostgreSQLのunique_violation(23505)かどうかを判定する。
pub fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(db_error) if db_error.code().as_deref() == Some("23505")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
