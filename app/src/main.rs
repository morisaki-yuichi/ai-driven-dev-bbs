use axum::{Router, middleware::from_fn_with_state, routing::get};
use sqlx::PgPool;
use tower_http::services::ServeDir;

use bbs::domain::model::Error as DomainError;
use bbs::web::error::AppError;
use bbs::web::middleware;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    // Why: 購読者(subscriber)を1つも登録しないと`tracing::warn!`/`error!`は
    // どこにも出力されず、丸ごと捨てられる。decision 0021 決定5が要求する
    // 「CSRF検証失敗時にサーバ側へ`tracing::warn!`を1行残す」は、この初期化が
    // 無いと呼び出しが存在しても実運用上は満たされない。DBエラー(web/error.rs)の
    // 診断出力も同様にここに依存する。
    // Why-not: `EnvFilter`は使わない——`tracing-subscriber`の`env-filter`featureは
    // 既定で無効であり、有効化するとdecision 0016で固定した依存構成に手を入れることになる。
    // 併せて、環境変数で挙動が変わるとH-13(clone直後に起動)の再現性を下げる。
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

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
        // `layout.html`が参照する/static/app.cssの配信。相対パス"static"は
        // ホストの`cargo run`(cwd=app/)・コンテナ(WORKDIR /app、Dockerfileが
        // `COPY static ./static`)のどちらでも実行時cwdからの相対で解決する。
        .nest_service("/static", ServeDir::new("static"))
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
