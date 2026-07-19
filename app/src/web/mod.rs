pub mod cookies;
pub mod csrf;
pub mod error;
pub mod login;
pub mod logout;
pub mod middleware;
pub mod params;
pub mod register;
pub mod views;

use axum::{
    Router,
    middleware::{from_fn, from_fn_with_state},
    routing::{get, post},
};
use sqlx::PgPool;
use tower_http::services::ServeDir;

use crate::domain::model::Error as DomainError;
use error::AppError;

/// アプリ全体のルータを構築する。`main.rs`と統合テスト(`tests/`)の両方が
/// 同じ配線を共有するための入口。
pub fn build_router(pool: PgPool) -> Router {
    // ログインのタイミング差を埋めるダミーハッシュ(web/login.rs)を起動時に用意する。
    // 遅延生成のままだと、最初の1回だけハッシュ生成のコストが上乗せされ、
    // ならすはずのタイミング差をその1回に作ってしまう。
    let _ = login::dummy_password_hash();

    Router::new()
        // "/" はP03(スレッド一覧画面)。AC09-1によりログイン必須。
        .route("/", get(|| async { "ok" }))
        // F03: formal/Bbs/Op.leanの`logout`が`requireAuth`を先に呼ぶ定義に
        // 合わせ、"/logout"もここ(require_authより前に登録されたルート)に置く。
        .route("/logout", post(logout::submit))
        .route_layer(from_fn_with_state(pool.clone(), middleware::require_auth))
        // P01(ログイン)・P02(登録)は未ログインで到達できる(F01/F02)。
        .route("/register", get(register::show).post(register::submit))
        .route("/login", get(login::show).post(login::submit))
        // `layout.html`が参照する/static/app.cssの配信。相対パス"static"は
        // ホストの`cargo run`(cwd=app/)・コンテナ(WORKDIR /app、Dockerfileが
        // `COPY static ./static`)のどちらでも実行時cwdからの相対で解決する。
        .nest_service("/static", ServeDir::new("static"))
        .fallback(fallback)
        .with_state(pool)
        // decision 0021: CSRF対策(トークン発行 + 同一オリジン検証)はルータ全体に適用する。
        .layer(from_fn(csrf::csrf_token_middleware))
        .layer(from_fn(csrf::same_origin_guard))
}

// C-10: 存在しないURLへのアクセスは一律404相当。
async fn fallback() -> AppError {
    DomainError::NotFound.into()
}
