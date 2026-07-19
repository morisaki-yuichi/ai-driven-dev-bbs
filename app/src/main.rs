mod domain;
mod web;

use axum::{Router, routing::get};
use sqlx::PgPool;

use crate::domain::model::Error as DomainError;
use crate::web::error::AppError;

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

    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .fallback(fallback);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind listener");
    axum::serve(listener, app).await.expect("server error");
}

// C-10: 存在しないURLへのアクセスは一律404相当。
async fn fallback() -> AppError {
    DomainError::NotFound.into()
}
