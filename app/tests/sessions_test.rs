//! `#[sqlx::test]`のひな形。テストごとに使い捨てDBが用意され、
//! `migrations/`が自動適用される(F、foundation-plan.md §1.8)。

use bbs::db::{password, sessions};
use sqlx::PgPool;

async fn insert_test_user(pool: &PgPool, unique_id: &str) -> i64 {
    let hash = password::hash("Correct1Horse!").unwrap();
    sqlx::query_scalar!(
        "insert into users (unique_id, password_hash, display_name) values ($1, $2, $3) returning id",
        unique_id,
        hash,
        "テストユーザー"
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn create_then_find_user_resolves_display_name(pool: PgPool) {
    let user_id = insert_test_user(&pool, "testuser_01").await;

    let token = sessions::create(&pool, user_id).await.unwrap();
    let found = sessions::find_user(&pool, &token).await.unwrap();

    assert!(found.is_some());
    assert_eq!(found.unwrap().display_name, "テストユーザー");
}

#[sqlx::test]
async fn find_user_returns_none_for_unknown_session(pool: PgPool) {
    let found = sessions::find_user(&pool, "no-such-session").await.unwrap();
    assert!(found.is_none());
}

#[sqlx::test]
async fn delete_invalidates_the_session(pool: PgPool) {
    let user_id = insert_test_user(&pool, "testuser_02").await;
    let token = sessions::create(&pool, user_id).await.unwrap();

    sessions::delete(&pool, &token).await.unwrap();

    let found = sessions::find_user(&pool, &token).await.unwrap();
    assert!(found.is_none());
}

#[sqlx::test]
async fn same_user_can_hold_multiple_sessions(pool: PgPool) {
    // decision 0007: 多重ログインを許可し、既存セッションを破棄しない。
    let user_id = insert_test_user(&pool, "testuser_03").await;

    let token_a = sessions::create(&pool, user_id).await.unwrap();
    let token_b = sessions::create(&pool, user_id).await.unwrap();

    assert!(
        sessions::find_user(&pool, &token_a)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        sessions::find_user(&pool, &token_b)
            .await
            .unwrap()
            .is_some()
    );
}
