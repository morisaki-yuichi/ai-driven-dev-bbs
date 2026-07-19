//! セッションCookie・CSRFトークンCookieの属性を1箇所に集約する
//! (foundation-plan.md §1.6、decision 0021)。両方とも`HttpOnly` / `SameSite=Lax` /
//! `Path=/`。`Secure`はHTTP評価環境のため付けない。有効期限は設けない
//! (decision 0007/0021: セッションと同じ属性に揃える)。
//!
//! `build_session_cookie`はF02ログイン(web/login.rs)が使う。`removal_cookie`は
//! F03ログアウト(web/logout.rs)が使う。`SESSION_COOKIE_NAME`はweb/middleware.rsの
//! 認証ガードが使う。

use axum::{
    http::{HeaderValue, header},
    response::Response,
};
use axum_extra::extract::cookie::{Cookie, SameSite};

use crate::web::error::AppError;

pub const SESSION_COOKIE_NAME: &str = "session_id";

/// decision 0021: セッションから独立したCSRFトークンCookie名。
pub const CSRF_COOKIE_NAME: &str = "csrf_token";

/// `cookie`を`Set-Cookie`ヘッダとして`response`へ追加する。
///
/// Why-not: 変換失敗を`if let Ok(..)`で読み飛ばさない。たとえばセッションCookieを
/// 載せられなかったのに「/へリダイレクト」だけ返すと、DBにはセッションが残っている
/// のにクライアントは未ログインのまま、という「失敗したのに成功したことになる」
/// 応答になる。UUID由来の値なので`HeaderValue`への変換は実際には失敗しないが、
/// 到達不能であることと握り潰してよいことは別なので、エラーとして伝播させ操作全体を
/// 失敗させる(呼び出し元がトランザクション内で使えば、そのままロールバックされる)。
///
/// Why-not: `web/csrf.rs`の2箇所(`csrf_token_middleware` / `rotate_csrf_cookie`)は
/// この関数へ寄せていない。どちらも`Response`を返す形でエラーを伝播できない位置に
/// あり、CSRFトークンCookieの付与失敗は上記のような成功偽装にもならないため、
/// 意図的に「読み飛ばす」ままにしてある。
pub fn append_cookie(response: &mut Response, cookie: Cookie<'_>) -> Result<(), AppError> {
    let name = cookie.name().to_string();
    let value = HeaderValue::from_str(&cookie.to_string()).map_err(|e| {
        AppError::Internal(format!("cookie {name} is not a valid header value: {e}"))
    })?;
    response.headers_mut().append(header::SET_COOKIE, value);
    Ok(())
}

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
