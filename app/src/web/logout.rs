//! F03 ログアウト(POST /logout)。AC03-1〜AC03-3。P07(`docs/product/issues/03_logout.md`)。
//!
//! `require_auth`配下に置く(`web/mod.rs`のルータ構成)。formal/Bbs/Op.leanの
//! `logout`が`requireAuth`を先に呼ぶ定義になっているのと実装側を一致させるため
//! ―― 未ログインでの`POST /logout`は`sessions`テーブルへ一切触れずログイン画面へ
//! リダイレクトされる(`formal/Bbs/Invariant.lean`の`logout_requires_auth`)。
//!
//! decision 0002(critical): ハンドラの入口でトランザクションを開始し、Errを
//! 返す経路では必ずロールバックする規律を`db::with_transaction`に集約している。
//! decision 0021 決定1: `POST /logout`も例外なくCSRF二重送信トークン検証の対象。

use axum::{
    extract::State,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;
use sqlx::PgPool;

use crate::db;
use crate::web::cookies::{SESSION_COOKIE_NAME, append_cookie, removal_cookie};
use crate::web::csrf::{CsrfForm, rotate_csrf_cookie};
use crate::web::error::AppError;
use crate::web::params::LogoutForm;

/// POST /logout。
pub async fn submit(
    State(pool): State<PgPool>,
    jar: CookieJar,
    CsrfForm(_form): CsrfForm<LogoutForm>,
) -> Result<Response, AppError> {
    // `require_auth`を通過済みなのでセッションCookieの存在自体は保証されているが、
    // 値を握り潰さず明示的に扱う。万一取得できない場合、「何も起きない」ログアウト
    // 成功応答を返すよりは内部エラーとして扱うほうが安全(`cookies::append_cookie`の
    // Why-notと同じ理由: 到達不能であることと握り潰してよいことは別)。
    let session_id = jar.get(SESSION_COOKIE_NAME).map(|c| c.value().to_string());
    let Some(session_id) = session_id else {
        return Err(AppError::Internal(
            "POST /logout reached require_auth without a session cookie".to_string(),
        ));
    };

    db::with_transaction(&pool, move |mut tx| async move {
        db::sessions::delete(&mut *tx, &session_id).await?;
        Ok(((), tx))
    })
    .await?;

    // AC03-1: ログイン画面へ即座にリダイレクトする。
    let mut response = Redirect::to("/login").into_response();
    append_cookie(&mut response, removal_cookie())?;
    // decision 0021 決定5: ログアウト時にもCSRFトークンをローテーションする
    // (セッション固定攻撃と同種のリスクを避ける衛生措置。login.rsと同じ処理)。
    rotate_csrf_cookie(&mut response);
    Ok(response)
}
