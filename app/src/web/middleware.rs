//! 認証ガード(C-09)を扱う。`Cache-Control: no-store`(C-11/AC03-2)はここに
//! 集約されているわけではない ―― 本モジュールの`require_auth`と
//! `reflect_auth_on_error_page`の2箇所に加え、`register.rs`・`login.rs`・
//! `profile.rs`・`thread_create.rs`の各ハンドラでも個別に付与しており、実態は
//! 計5箇所。`profile.rs`・`thread_create.rs`は`require_auth`配下のルートなので
//! `require_auth`が付与した後にハンドラ側でも付与することになり設計上は冗長だが、
//! 両者とも`insert`(上書き)であるため実害(二重ヘッダー化)は起きない
//! (回帰テストは`unknown_url_404_*_has_no_store`(`require_auth`の外)・
//! `nonexistent_thread_id_under_require_auth_404_has_no_store_exactly_once`
//! (`require_auth`配下、いずれも`tests/thread_detail_test.rs`)を参照)。
//! decision 0008: ブラウザバックはSSR/MPAの範囲でHTTPレイヤの責務として扱う。

use axum::{
    body::Body,
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
use crate::web::csrf::CsrfToken;
use crate::web::error::{AppError, AuthAwareErrorPage, render_not_found_body};
use crate::web::views::CurrentUser;

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

/// decision 0028: `AuthAwareErrorPage`マーカーの付いたエラーページ(404)を、**ログイン中なら
/// 認証済みヘッダーで描き直す**。ルータ全体に適用する(`web/mod.rs`)。
///
/// Why(`require_auth`の中ではなくルータ全体に置く理由): `fallback`(未知URL)は
/// 認証を必須にできない ―― 未ログインでも404を返す必要がある ―― ので
/// `require_auth`の外にある。`require_auth`の後処理として書くと、この経路だけ
/// 取りこぼす(改修前に実際に起きていた不具合)。ここでは「セッションがあれば解決し、
/// 無ければ未ログイン扱い」という緩い解決にすることで、認証必須画面と`fallback`の
/// どちらも同じ1箇所で面倒を見る。
///
/// Why(先にレスポンスを待ってからDBを引く理由): マーカーが無いレスポンス
/// (通常の画面・静的ファイル)では`db::sessions::find_user`を呼ばない。
/// 常時解決する方式だと全リクエストにセッション参照が1回増える。
///
/// **CSRF検証失敗はここを通っても書き換わらない。** マーカーが付かないため。
/// 加えて`web/mod.rs`ではCSRFの2ミドルウェアをこれより外側に置いてあり、
/// `same_origin_guard`が撥ねた応答はそもそもここへ来ない(二重の防御)。
pub async fn reflect_auth_on_error_page(
    State(pool): State<PgPool>,
    jar: CookieJar,
    req: Request,
    next: Next,
) -> Response {
    // リクエスト側の情報は`next.run`でreqを手放す前に取り出しておく。
    let session_id = jar.get(SESSION_COOKIE_NAME).map(|c| c.value().to_string());
    let csrf_token = req.extensions().get::<CsrfToken>().map(|t| t.0.clone());

    let mut response = next.run(req).await;
    if response.extensions().get::<AuthAwareErrorPage>().is_none() {
        return response;
    }

    // マーカーを確認した時点で一律に付与する(持ち越し修正: 以前はこの付与が
    // セッション未解決時の早期returnより後にあり、**未ログインで未知URLを踏んだ404**
    // にだけ`no-store`が欠落していた ―― `fallback`(未知URL)は`require_auth`の外に
    // あるため、他のどこからも付与されない経路だった)。`insert`は上書きなので、
    // `require_auth`配下で既に付与済みの場合(この後の分岐で本文が描き直される経路)
    // でも二重付与にはならない。
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));

    // ログアウトフォームにはCSRFトークンが要る(views::CurrentUserのdocコメント)。
    // どちらか欠けるなら未ログイン表示のまま返す(no-storeは既に付与済み)。
    let (Some(session_id), Some(csrf_token)) = (session_id, csrf_token) else {
        return response;
    };
    let Ok(Some(user)) = db::sessions::find_user(&pool, &session_id).await else {
        return response;
    };
    let Ok(body) = render_not_found_body(Some(CurrentUser {
        display_name: user.display_name,
        csrf_token,
    })) else {
        return response;
    };

    // ステータス・ヘッダー(`Set-Cookie`・`Cache-Control`等)は元の応答のものを保つ
    // (`Cache-Control`は上で付与済み)。差し替えるのは本文だけ。
    let (mut parts, _) = response.into_parts();
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(body))
}
