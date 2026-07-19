//! P01(ログイン画面)の暫定スタブ。GET /register からの動線(AC01-5: 登録成功後の
//! リダイレクト先)を実在させるためのプレースホルダで、ログイン処理(F02)は
//! このセッションのスコープ外。F02実装時にフォーム・POSTハンドラを追加する。

use askama::Template;
use axum::response::{Html, IntoResponse, Response};

use crate::web::error::AppError;
use crate::web::views::CurrentUser;

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    current_user: Option<CurrentUser>,
}

/// GET /login。
pub async fn show() -> Response {
    let tmpl = LoginTemplate { current_user: None };
    match tmpl.render() {
        Ok(body) => Html(body).into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}
