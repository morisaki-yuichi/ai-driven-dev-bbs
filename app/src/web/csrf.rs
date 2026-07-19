//! decision 0021: 二重送信トークン + Origin検証によるCSRF対策。
//! 副作用(Cookie発行・Origin検証・フォーム検証)をここに隔離する。
//! 純粋な判定ロジックは `domain::csrf` を参照する。
//!
//! ユーザーが明示的に確認・承認した2点を実装コメントとして明記する(decision 0021 影響節):
//! - トークンは**ワンタイムにしない**(Cookie生存期間中は同じ値を使い回す)。
//! - `POST /register` / `POST /login` を含む**未ログインPOSTも保護対象に含める**。

use axum::{
    Form,
    extract::{FromRequest, Request},
    http::{HeaderValue, Method, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::domain::csrf::{is_same_origin, tokens_match};
use crate::web::cookies::{CSRF_COOKIE_NAME, build_csrf_cookie};
use crate::web::error::AppError;

/// リクエスト拡張(`Extension`)経由でハンドラに渡すCSRFトークンの値。
/// GET応答でフォームのhidden inputへ描画するために使う。
#[derive(Clone)]
pub struct CsrfToken(pub String);

/// フォーム構造体(`web/params.rs`)がCSRFトークンフィールドを持つことを保証する。
/// `CsrfForm<T>`がこれを要求することで、使い忘れが型検査で見える(decision 0021)。
pub trait HasCsrfToken {
    fn csrf_token(&self) -> &str;
}

/// `Form<T>`を包み、Cookie値とフォーム値のトークン一致を検証したうえで中身を取り出す
/// エクストラクタ。検証に失敗すると403(`AppError::Csrf`)を返す。
/// ハンドラの引数型として使うことで、CSRF検証がハンドラ本体より前(DB書き込みより前)
/// に必ず完了する(decision 0021 決定6: 拒否経路ではトランザクションを開かない)。
pub struct CsrfForm<T>(pub T);

impl<S, T> FromRequest<S> for CsrfForm<T>
where
    T: DeserializeOwned + HasCsrfToken,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let cookie_token = CookieJar::from_headers(req.headers())
            .get(CSRF_COOKIE_NAME)
            .map(|c| c.value().to_string())
            .unwrap_or_default();

        let Form(value) = Form::<T>::from_request(req, state)
            .await
            .map_err(IntoResponse::into_response)?;

        if !tokens_match(&cookie_token, value.csrf_token()) {
            tracing::warn!("csrf: double-submit token mismatch, rejecting form submission");
            return Err(AppError::Csrf.into_response());
        }
        Ok(CsrfForm(value))
    }
}

/// ルータ全体に適用するミドルウェア。`csrf_token`Cookieが無ければCSPRNGで発行し、
/// リクエスト拡張に載せる(GETでの画面描画・POSTでの検証のどちらにも使えるように)。
/// 発行は`Set-Cookie`として応答に反映する(セッションが無い`/register`・`/login`の
/// GETでもトークンが手に入る。これがセッション同期トークン方式との決定的な差)。
pub async fn csrf_token_middleware(jar: CookieJar, mut req: Request, next: Next) -> Response {
    let (token, is_new) = match jar.get(CSRF_COOKIE_NAME) {
        Some(cookie) => (cookie.value().to_string(), false),
        None => (Uuid::new_v4().to_string(), true),
    };
    req.extensions_mut().insert(CsrfToken(token.clone()));

    let mut response = next.run(req).await;
    if is_new {
        let cookie = build_csrf_cookie(token);
        if let Ok(value) = HeaderValue::from_str(&cookie.to_string()) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

/// ルータ全体に適用するミドルウェア。状態変更メソッドについて、`Origin`
/// (無ければ`Referer`)のオリジンがリクエストの`Host`と一致することを要求する。
///
/// 対象は**安全なメソッド(GET / HEAD / OPTIONS)以外のすべて**。
/// Why-not: `== Method::POST` と書くと、将来 PUT / PATCH / DELETE を足したときに
/// 黙って無防備なメソッドが増える。許可側を列挙する形にして、既定を「保護する」に倒す
/// (現状のルータはPOSTしか公開していないので、実挙動は今のところ同じ)。
pub async fn same_origin_guard(req: Request, next: Next) -> Response {
    let is_safe_method = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if !is_safe_method {
        let headers = req.headers();
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
        let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
        let referer = headers.get(header::REFERER).and_then(|v| v.to_str().ok());

        let allowed = match host {
            Some(host) => is_same_origin(origin, referer, host),
            None => false,
        };
        if !allowed {
            tracing::warn!(
                method = %req.method(),
                origin = ?origin,
                referer = ?referer,
                host = ?host,
                "csrf: rejected state-changing request with mismatched Origin/Referer"
            );
            return AppError::Csrf.into_response();
        }
    }
    next.run(req).await
}
