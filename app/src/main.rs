use axum::{Router, middleware::from_fn_with_state, routing::get};
use sqlx::PgPool;

use bbs::domain::model::Error as DomainError;
use bbs::web::error::AppError;
use bbs::web::middleware;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPool::connect(&database_url)
        .await
        .expect("failed to connect to database");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("failed to run migrations");

    // "/" はP03(スレッド一覧画面)。AC09-1によりログイン必須。
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .route_layer(from_fn_with_state(pool.clone(), middleware::require_auth))
        .fallback(fallback)
        .with_state(pool);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind listener");
    axum::serve(listener, app).await.expect("server error");
}

// C-10: 存在しないURLへのアクセスは一律404相当。
async fn fallback() -> AppError {
    DomainError::NotFound.into()
}
