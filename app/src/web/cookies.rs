//! セッションCookie・CSRFトークンCookieの属性を1箇所に集約する
//! (foundation-plan.md §1.6、decision 0021)。両方とも`HttpOnly` / `SameSite=Lax` /
//! `Path=/`。`Secure`はHTTP評価環境のため付けない。有効期限は設けない
//! (decision 0007/0021: セッションと同じ属性に揃える)。
//!
//! `build_session_cookie`/`removal_cookie`の呼び出し元(F02ログイン・F03ログアウトの
//! ハンドラ)はfoundation-plan.md §5の範囲外のため、それまでの間`dead_code`を抑止する。
//! `SESSION_COOKIE_NAME`はweb/middleware.rsの認証ガードが使う。

#![allow(dead_code)]

use axum_extra::extract::cookie::{Cookie, SameSite};

pub const SESSION_COOKIE_NAME: &str = "session_id";

/// decision 0021: セッションから独立したCSRFトークンCookie名。
pub const CSRF_COOKIE_NAME: &str = "csrf_token";

fn base_cookie(name: &'static str, value: String) -> Cookie<'static> {
    Cookie::build((name, value))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .build()
}

pub fn build_session_cookie(token: String) -> Cookie<'static> {
    base_cookie(SESSION_COOKIE_NAME, token)
}

/// ログアウト時にクライアント側のCookieも失効させるための削除用エントリ。
/// パス属性は発行時と一致させる必要がある(cookie仕様上、パスが違うと別Cookie扱いになる)。
pub fn removal_cookie() -> Cookie<'static> {
    Cookie::build(SESSION_COOKIE_NAME).path("/").build()
}

/// decision 0021: CSRFトークンCookieを発行する。ワンタイムにせず、Cookieの
/// 生存期間中は同じ値を使い回す(決定4。複数タブ・ブラウザバック時の誤検知を避ける)。
pub fn build_csrf_cookie(token: String) -> Cookie<'static> {
    base_cookie(CSRF_COOKIE_NAME, token)
}
