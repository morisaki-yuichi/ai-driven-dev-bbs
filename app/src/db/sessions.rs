//! セッションのDB永続化(decision 0007: 多重セッション許可・DB永続化・有効期限なし)。
//!
//! `create`/`delete`の呼び出し元(F02ログイン・F03ログアウトのハンドラ)は
//! foundation-plan.md §5の範囲外(機能実装フェーズ)のため、それまでの間
//! `dead_code` を抑止する。`find_user`はweb/middleware.rsの認証ガードが使う。

#![allow(dead_code)]

use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct AuthenticatedUser {
    pub user_id: i64,
    pub display_name: String,
}

/// 新しいセッションを作成し、CSPRNG生成のセッションIDを返す(foundation-plan.md §1.6)。
pub async fn create(pool: &PgPool, user_id: i64) -> Result<String, sqlx::Error> {
    let token = Uuid::new_v4().to_string();
    sqlx::query!(
        "insert into sessions (id, user_id) values ($1, $2)",
        token,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(token)
}

/// セッションIDからユーザーを解決する。存在しない/失効済みなら`None`。
pub async fn find_user(
    pool: &PgPool,
    session_id: &str,
) -> Result<Option<AuthenticatedUser>, sqlx::Error> {
    sqlx::query_as!(
        AuthenticatedUser,
        r#"
        select users.id as "user_id!", users.display_name
        from sessions
        join users on users.id = sessions.user_id
        where sessions.id = $1
        "#,
        session_id
    )
    .fetch_optional(pool)
    .await
}

/// ログアウト: サーバ側のセッションレコードを削除する(CLAUDE.md セキュリティ必須要件)。
pub async fn delete(pool: &PgPool, session_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query!("delete from sessions where id = $1", session_id)
        .execute(pool)
        .await?;
    Ok(())
}
