//! セッションCookieの属性を1箇所に集約する(foundation-plan.md §1.6)。
//! `HttpOnly` / `SameSite=Lax` / `Path=/`。`Secure`はHTTP評価環境のため付けない。
//! 有効期限は設けない(decision 0007)。
//!
//! `build_session_cookie`/`removal_cookie`の呼び出し元(F02ログイン・F03ログアウトの
//! ハンドラ)はfoundation-plan.md §5の範囲外のため、それまでの間`dead_code`を抑止する。
//! `SESSION_COOKIE_NAME`はweb/middleware.rsの認証ガードが使う。

#![allow(dead_code)]

use axum_extra::extract::cookie::{Cookie, SameSite};

pub const SESSION_COOKIE_NAME: &str = "session_id";

pub fn build_session_cookie(token: String) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE_NAME, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .build()
}

/// ログアウト時にクライアント側のCookieも失効させるための削除用エントリ。
/// パス属性は発行時と一致させる必要がある(cookie仕様上、パスが違うと別Cookie扱いになる)。
pub fn removal_cookie() -> Cookie<'static> {
    Cookie::build(SESSION_COOKIE_NAME).path("/").build()
}
