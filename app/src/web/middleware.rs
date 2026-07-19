//! 認証ガード(C-09)と`Cache-Control: no-store`(C-11/AC03-2)を1箇所に集約する。
//! decision 0008: ブラウザバックはSSR/MPAの範囲でHTTPレイヤの責務として扱う。

use axum::{
    extract::{Request, State},
    http::{HeaderValue, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use sqlx::PgPool;

use crate::db;
use crate::domain::model::Error as DomainError;
use crate::web::cookies::SESSION_COOKIE_NAME;
use crate::web::error::AppError;

/// 認証必須画面向けミドルウェア。未ログイン/無効なセッションは一律ログイン画面へ
/// リダイレクトする(C-09)。成功時はレスポンスに`Cache-Control: no-store`を付与し、
/// ログアウト後のブラウザバックでキャッシュ経由の表示が起きないようにする(C-11)。
pub async fn require_auth(
    State(pool): State<PgPool>,
    jar: CookieJar,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(cookie) = jar.get(SESSION_COOKIE_NAME) else {
        return AppError::from(DomainError::NotAuthenticated).into_response();
    };

    let user = match db::sessions::find_user(&pool, cookie.value()).await {
        Ok(Some(user)) => user,
        Ok(None) => return AppError::from(DomainError::NotAuthenticated).into_response(),
        Err(e) => return AppError::from(e).into_response(),
    };

    req.extensions_mut().insert(user);
    let mut response = next.run(req).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
