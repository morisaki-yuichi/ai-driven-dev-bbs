pub mod cookies;
pub mod csrf;
pub mod error;
pub mod format;
pub mod login;
pub mod logout;
pub mod middleware;
pub mod params;
pub mod profile;
pub mod register;
pub mod thread_create;
pub mod thread_detail;
pub mod thread_list;
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
        .route("/", get(thread_list::show))
        // F03: formal/Bbs/Op.leanの`logout`が`requireAuth`を先に呼ぶ定義に
        // 合わせ、"/logout"もここ(require_authより前に登録されたルート)に置く。
        .route("/logout", post(logout::submit))
        // "/threads/new" はP05(スレッド作成画面)。詳細要件によりログイン中の
        // ユーザーのみ作成可能(F05)。
        .route(
            "/threads/new",
            get(thread_create::show).post(thread_create::submit),
        )
        // "/threads/{id}" はP04(スレッド詳細画面)。F10(スレッド詳細表示、issues/10)。
        // 静的セグメント"/threads/new"と動的セグメント"/threads/{id}"はaxum(matchit)が
        // 登録順に関わらず静的側を優先するため、両立できる。
        .route("/threads/{id}", get(thread_detail::show))
        // F07(コメント作成、issues/07)。ログイン中のユーザーのみ作成可能なので
        // 他の"/threads/{id}"系ルートと同じくrequire_auth配下(下の.route_layer)に置く。
        .route(
            "/threads/{id}/comments",
            post(thread_detail::create_comment),
        )
        // F08(コメント削除、issues/08)。作成者本人のみ削除可能(AC08-3)なので
        // 同じくrequire_auth配下に置く。
        .route(
            "/threads/{thread_id}/comments/{comment_id}/delete",
            post(thread_detail::delete_comment),
        )
        // F06(スレッド削除、issues/06)。作成者本人のみ削除可能(AC06-3)なので
        // 同じくrequire_auth配下に置く。静的セグメント"/threads/{id}/comments"等と
        // 動的な"/threads/{id}/delete"は末尾のリテラルが異なるためaxum(matchit)が
        // 曖昧なく解決する。
        .route("/threads/{id}/delete", post(thread_detail::delete_thread))
        // F04(プロフィール編集、issue 04)。P06、パスはdecision 0020で確定。
        // 表示名の変更はログイン中のユーザー自身に限る(C-09)ので、他の認証必須
        // ルートと同じくrequire_auth配下に置く。
        .route("/profile/edit", get(profile::show).post(profile::submit))
        .route_layer(from_fn_with_state(pool.clone(), middleware::require_auth))
        // P01(ログイン)・P02(登録)は未ログインで到達できる(F01/F02)。
        .route("/register", get(register::show).post(register::submit))
        .route("/login", get(login::show).post(login::submit))
        // `layout.html`が参照する/static/app.cssの配信。相対パス"static"は
        // ホストの`cargo run`(cwd=app/)・コンテナ(WORKDIR /app、Dockerfileが
        // `COPY static ./static`)のどちらでも実行時cwdからの相対で解決する。
        .nest_service("/static", ServeDir::new("static"))
        .fallback(fallback)
        .with_state(pool.clone())
        // ログイン中に404を踏んだときのヘッダーを認証済み表示に揃える(F10)。
        // `require_auth`の内側ではなくここに置くのは、`fallback`(未知URL)が
        // `require_auth`の外にあるため ―― 詳細は`middleware::reflect_auth_on_error_page`。
        // CSRFの2ミドルウェアより内側に置くことで、`same_origin_guard`が撥ねた
        // 応答(認証前のエラー)がここへ来ないことも同時に保証する。
        .layer(from_fn_with_state(
            pool,
            middleware::reflect_auth_on_error_page,
        ))
        // decision 0021: CSRF対策(トークン発行 + 同一オリジン検証)はルータ全体に適用する。
        .layer(from_fn(csrf::csrf_token_middleware))
        .layer(from_fn(csrf::same_origin_guard))
}

// C-10: 存在しないURLへのアクセスは一律404相当。
async fn fallback() -> AppError {
    DomainError::NotFound.into()
}
