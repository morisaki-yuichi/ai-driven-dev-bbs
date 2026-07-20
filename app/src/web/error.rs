//! `AppError` → HTTPレスポンスの写像を1箇所に集約する。
//! C-10(存在しない/削除済みリソースは一律404)をここで保証する。

use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};

use crate::domain::model::Error as DomainError;
use crate::web::views::CurrentUser;

/// エラーの種類。以前は`AppError`本体がこの形の enum だった
/// (`AppError::Domain(...)`のように直接構築・パターンマッチされていた)。
#[derive(Debug)]
enum AppErrorKind {
    Domain(DomainError),
    Sqlx(sqlx::Error),
    Template(askama::Error),
    /// decision 0021: 二重送信トークン不一致 / Origin不一致。C-10(404)とは別に
    /// 403として扱う(リソースの存否とは無関係な検証失敗のため)。
    Csrf,
    /// 通常起こらない内部エラー(パスワードハッシュ化失敗等)。
    Internal(String),
}

/// `AppError` → HTTPレスポンスへの一元化された変換の入口。
///
/// **認証中ユーザーの情報は持たない。** 404ページのヘッダーを認証状態に合わせる
/// 仕組みは`AuthAwareErrorPage`マーカー＋`web/middleware.rs`のレスポンス後処理に
/// 任せる(下記`AuthAwareErrorPage`のdocコメント参照)。
#[derive(Debug)]
pub struct AppError {
    kind: AppErrorKind,
}

impl AppError {
    pub fn domain(e: DomainError) -> Self {
        AppError {
            kind: AppErrorKind::Domain(e),
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        AppError {
            kind: AppErrorKind::Internal(msg.into()),
        }
    }

    pub fn csrf() -> Self {
        AppError {
            kind: AppErrorKind::Csrf,
        }
    }

    /// register.rs: 重複IDエラーをフォームへインライン表示するための判定。
    /// `AppError`が構造体になった後も直接パターンマッチできるよう、
    /// 内部の`kind`を覗く専用の述語として用意する。
    pub fn is_duplicate_unique_id(&self) -> bool {
        matches!(
            self.kind,
            AppErrorKind::Domain(DomainError::DuplicateUniqueId)
        )
    }
}

impl From<DomainError> for AppError {
    fn from(e: DomainError) -> Self {
        AppError::domain(e)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError {
            kind: AppErrorKind::Sqlx(e),
        }
    }
}

impl From<askama::Error> for AppError {
    fn from(e: askama::Error) -> Self {
        AppError {
            kind: AppErrorKind::Template(e),
        }
    }
}

impl From<argon2::password_hash::Error> for AppError {
    fn from(e: argon2::password_hash::Error) -> Self {
        AppError::internal(e.to_string())
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

/// decision 0028: 404ページのレスポンスに載せるマーカー。「このレスポンスは認証状態に応じて
/// ヘッダーを描き直してよいエラーページである」ことだけを表す。
/// 実際の描き直しは`web/middleware.rs`の`reflect_auth_on_error_page`が行う。
///
/// Why(この方式にした理由): 404を返すのは`?`で`DomainError`が`AppError`へ
/// 変換される経路であり、`From<DomainError>`にリクエストコンテキストは渡せない。
/// つまり「エラー値そのものに認証情報を積む」やり方は、どこかで誰かが積み直す
/// 必要がある。マーカー＋ミドルウェア後処理なら、**ハンドラ側は何もしなくてよい**
/// ―― 付け忘れという状態が存在しなくなる。F06/F07/F08がNotFound・Forbiddenを
/// 返すようになっても、追加の配線なしに正しいヘッダーが出る。
///
/// Why-not(採らなかった案):
/// - **ハンドラが`.map_err(|e| e.with_current_user(..))`で後付けする**(改修前の形):
///   opt-inなので付け忘れをコンパイラが検出できない。実際、`web/mod.rs`の
///   `fallback`(未知URL)が`require_auth`の外にあるため付けようがなく、
///   ログイン中に存在しないURLを踏むと未ログイン用ヘッダーが出ていた。
///   さらに成功経路(引数渡し)と失敗経路(`map_err`)で`current_user`を
///   二重に受け渡すことになり、片方だけ直す事故の余地が残る。
/// - **`AppError`に`AuthenticatedUser`を持たせ、エクストラクタで自動注入する**:
///   `From<DomainError>`が使えなくなり(`?`が壊れる)、全ハンドラの戻り値型に
///   波及する。得られるものに対して改変が大きい。
/// - **ミドルウェアで常に認証情報を解決してリクエスト拡張に載せる**: 全リクエスト
///   (静的ファイル含む)にセッション参照のDBクエリが増える。マーカー方式なら
///   エラーページを返すときだけ引けばよい。
///
/// **CSRF検証失敗(`AppErrorKind::Csrf`)にはこのマーカーを付けない。** CSRFエラー
/// ページに認証情報が載らないことは、この「付けない」一点で保証される
/// (`AppError`には認証情報を後から積む手段がもう無い)。理由は`csrf_error()`の
/// コメント参照。
#[derive(Clone, Copy)]
pub struct AuthAwareErrorPage;

/// `web/middleware.rs`がマーカー付きレスポンスの本文を描き直すための入口。
/// テンプレートとフィールドの対応をこのモジュールの外へ漏らさない。
pub(crate) fn render_not_found_body(
    current_user: Option<CurrentUser>,
) -> Result<String, askama::Error> {
    NotFoundTemplate { current_user }.render()
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self.kind {
            // C-09: 未ログインで認証必須URLへは一律ログイン画面へリダイレクト。
            AppErrorKind::Domain(DomainError::NotAuthenticated) => {
                Redirect::to("/login").into_response()
            }
            // C-10: 存在しない/削除済み/認可違反は一律404相当。
            // (forbiddenをnotFoundと同一視するのは decision 0019 の範囲外の実装判断だが、
            //  AC06-3のシナリオが「エラーまたはリダイレクト」を許容しており、
            //  C-10の「一律」の趣旨に沿う。)
            AppErrorKind::Domain(DomainError::NotFound | DomainError::Forbidden) => not_found(),
            // 以下はハンドラ側でフォームにインライン表示するのが本来の経路(UI/UXガイドライン
            // の二重バリデーション要件)。ここに到達するのはハンドラの実装漏れの安全網であり、
            // ページ全体を404にはしない(C-10は「存在しない」ケース限定のため)。
            AppErrorKind::Domain(
                DomainError::DuplicateUniqueId
                | DomainError::InvalidCredentials
                | DomainError::Validation(_)
                | DomainError::ThreadHasComments
                | DomainError::AlreadyDeleted,
            ) => (StatusCode::BAD_REQUEST, "invalid request").into_response(),
            AppErrorKind::Sqlx(e) => {
                tracing::error!(error = %e, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
            }
            AppErrorKind::Template(e) => {
                tracing::error!(error = %e, "template render error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
            }
            AppErrorKind::Csrf => csrf_error(),
            AppErrorKind::Internal(msg) => {
                tracing::error!(error = %msg, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
            }
        }
    }
}

/// ここでは常に未ログイン用ヘッダーで描画する。ログイン中だった場合は
/// `AuthAwareErrorPage`マーカーを見た`web/middleware.rs`が本文を描き直す。
/// 「既定は未ログイン表示、セッションが解決できたときだけ認証済み表示に上書き」
/// という向きにしてあるので、後処理が働かない経路でも表示は壊れない
/// (認証情報を漏らす方向には倒れない)。
fn not_found() -> Response {
    match render_not_found_body(None) {
        Ok(body) => {
            let mut response = (StatusCode::NOT_FOUND, Html(body)).into_response();
            response.extensions_mut().insert(AuthAwareErrorPage);
            response
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to render error page");
            (StatusCode::NOT_FOUND, "Not Found").into_response()
        }
    }
}

// decision 0021 決定5: 検証失敗時はHTTP 403 + 専用エラー画面。リダイレクトで
// 握り潰さない(失敗が観測できなくなるため)。
//
// Why-not(このページに認証情報を載せない理由): `layout.html`のログイン中ヘッダーは
// ログアウトフォームを含み、そのフォームはCSRFトークンを要する。CSRF検証に失敗した
// 文脈で有効なトークンを埋めたフォームを描くのは筋が通らず、空トークンで描けば
// 押した瞬間にまた403になるボタンを見せることになる。よってCSRFエラーページは
// **常に未ログイン用ヘッダー**で描画し、ログアウトフォームを持たない。
// この不変条件は`not_found()`と違って`AuthAwareErrorPage`マーカーを付けないこと、
// および`AppError`が認証情報を後から積む手段を持たないことで構造的に保たれる。
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
