//! GET / (P03スレッド一覧画面)の最小スタブ。
//!
//! 一覧の中身(検索・ソート・ページネーション、F09の範囲)はまだ実装しない。
//! ここで満たしたいのは「`layout.html`経由で描画されること」そのもの ――
//! 従来はプレーンテキスト"ok"を返しており、ヘッダー(表示名・ログアウトボタン)が
//! 描画されないため、シナリオ01の「ログイン後に可視のログアウトボタンを押す」を
//! agent-browserで通しで実演できなかった(F03着手の前提)。

use askama::Template;
use axum::{
    extract::Extension,
    response::{Html, IntoResponse, Response},
};

use crate::db::sessions::AuthenticatedUser;
use crate::web::csrf::CsrfToken;
use crate::web::error::AppError;
use crate::web::views::CurrentUser;

#[derive(Template)]
#[template(path = "thread_list.html")]
struct ThreadListTemplate {
    current_user: Option<CurrentUser>,
}

/// GET /。`require_auth`ミドルウェア配下のルートなので、ここに到達した時点で
/// `AuthenticatedUser`がリクエスト拡張に必ず存在する(C-09、AC09-1)。
/// `Cache-Control: no-store`は`require_auth`側で一括付与される(C-11)。
pub async fn show(
    Extension(user): Extension<AuthenticatedUser>,
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
) -> Response {
    let tmpl = ThreadListTemplate {
        current_user: Some(CurrentUser {
            display_name: user.display_name,
            csrf_token,
        }),
    };
    match tmpl.render() {
        Ok(body) => Html(body).into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}
