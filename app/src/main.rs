use sqlx::PgPool;

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

    let app = bbs::web::build_router(pool);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind listener");
    axum::serve(listener, app).await.expect("server error");
}
