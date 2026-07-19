//! `AppError` → HTTPレスポンスの写像を1箇所に集約する。
//! C-10(存在しない/削除済みリソースは一律404)をここで保証する。

use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};

use crate::domain::model::Error as DomainError;
use crate::web::views::CurrentUser;

#[derive(Debug)]
pub enum AppError {
    Domain(DomainError),
    Sqlx(sqlx::Error),
    Template(askama::Error),
    /// decision 0021: 二重送信トークン不一致 / Origin不一致。C-10(404)とは別に
    /// 403として扱う(リソースの存否とは無関係な検証失敗のため)。
    Csrf,
}

impl From<DomainError> for AppError {
    fn from(e: DomainError) -> Self {
        AppError::Domain(e)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Sqlx(e)
    }
}

impl From<askama::Error> for AppError {
    fn from(e: askama::Error) -> Self {
        AppError::Template(e)
    }
}

#[derive(Template)]
#[template(path = "error.html")]
struct NotFoundTemplate {
    current_user: Option<CurrentUser>,
}

#[derive(Template)]
#[template(path = "csrf_error.html")]
struct CsrfErrorTemplate {
    current_user: Option<CurrentUser>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            // C-09: 未ログインで認証必須URLへは一律ログイン画面へリダイレクト。
            AppError::Domain(DomainError::NotAuthenticated) => {
                Redirect::to("/login").into_response()
            }
            // C-10: 存在しない/削除済み/認可違反は一律404相当。
            // (forbiddenをnotFoundと同一視するのは decision 0019 の範囲外の実装判断だが、
            //  AC06-3のシナリオが「エラーまたはリダイレクト」を許容しており、
            //  C-10の「一律」の趣旨に沿う。)
            AppError::Domain(DomainError::NotFound | DomainError::Forbidden) => not_found(),
            // 以下はハンドラ側でフォームにインライン表示するのが本来の経路(UI/UXガイドライン
            // の二重バリデーション要件)。ここに到達するのはハンドラの実装漏れの安全網であり、
            // ページ全体を404にはしない(C-10は「存在しない」ケース限定のため)。
            AppError::Domain(
                DomainError::DuplicateUniqueId
                | DomainError::InvalidCredentials
                | DomainError::Validation(_)
                | DomainError::ThreadHasComments
                | DomainError::AlreadyDeleted,
            ) => (StatusCode::BAD_REQUEST, "invalid request").into_response(),
            AppError::Sqlx(e) => {
                tracing::error!(error = %e, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
            }
            AppError::Template(e) => {
                tracing::error!(error = %e, "template render error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
            }
            AppError::Csrf => csrf_error(),
        }
    }
}

fn not_found() -> Response {
    // ここでは認証状態を持たないため、常に未ログイン表示のヘッダーになる。
    let tmpl = NotFoundTemplate { current_user: None };
    match tmpl.render() {
        Ok(body) => (StatusCode::NOT_FOUND, Html(body)).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "failed to render error page");
            (StatusCode::NOT_FOUND, "Not Found").into_response()
        }
    }
}

// decision 0021 決定5: 検証失敗時はHTTP 403 + 専用エラー画面。リダイレクトで
// 握り潰さない(失敗が観測できなくなるため)。
fn csrf_error() -> Response {
    let tmpl = CsrfErrorTemplate { current_user: None };
    match tmpl.render() {
        Ok(body) => (StatusCode::FORBIDDEN, Html(body)).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "failed to render csrf error page");
            (StatusCode::FORBIDDEN, "Forbidden").into_response()
        }
    }
}
