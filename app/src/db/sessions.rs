//! セッションのDB永続化(decision 0007: 多重セッション許可・DB永続化・有効期限なし)。
//!
//! `create`はF02ログイン(web/login.rs)が使う。`delete`はF03ログアウト(web/logout.rs)が
//! 使う。`find_user`はweb/middleware.rsの認証ガードが使う。
//!
//! いずれの関数も`executor`をジェネリックにしてあり、`&PgPool`(単発クエリ、
//! middleware.rsの読み取りなど)にも`&mut Transaction`(decision 0002:
//! login.rs・logout.rsが`db::with_transaction`で開いたトランザクション)にも使える。

use sqlx::PgExecutor;
use uuid::Uuid;

#[derive(Clone)]
pub struct AuthenticatedUser {
    pub user_id: i64,
    pub display_name: String,
}

/// 新しいセッションを作成し、CSPRNG生成のセッションIDを返す(foundation-plan.md §1.6)。
pub async fn create<'e, E>(executor: E, user_id: i64) -> Result<String, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    let token = Uuid::new_v4().to_string();
    sqlx::query!(
        "insert into sessions (id, user_id) values ($1, $2)",
        token,
        user_id
    )
    .execute(executor)
    .await?;
    Ok(token)
}

/// セッションIDからユーザーを解決する。存在しない/失効済みなら`None`。
pub async fn find_user<'e, E>(
    executor: E,
    session_id: &str,
) -> Result<Option<AuthenticatedUser>, sqlx::Error>
where
    E: PgExecutor<'e>,
{
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
    .fetch_optional(executor)
    .await
}

/// ログアウト: サーバ側のセッションレコードを削除する(CLAUDE.md セキュリティ必須要件)。
/// formal/Bbs/Invariant.leanの`logout_removes_only_target_session`が示すとおり、
/// 対象の1行だけを消し他のセッション(同一利用者の別セッションを含む)には触れない。
pub async fn delete<'e, E>(executor: E, session_id: &str) -> Result<(), sqlx::Error>
where
    E: PgExecutor<'e>,
{
    sqlx::query!("delete from sessions where id = $1", session_id)
        .execute(executor)
        .await?;
    Ok(())
}
