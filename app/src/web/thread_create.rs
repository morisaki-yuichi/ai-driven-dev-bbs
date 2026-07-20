//! F05 スレッド作成(GET /threads/new 表示・POST /threads/new 作成)。P05(ui_design.md)。
//! バリデーション順序は`domain::validation::create_thread_validation`
//! (formal/Bbs/Op.lean の `createThread` と同じ順序: タイトル→本文。
//! `formal/Bbs/Invariant.lean` の `createThread_atomic`(decision 0002: 検査失敗時は
//! 状態を書き換えない)・`createThread_does_not_modify_existing_threads`(C-05/AC05-4:
//! 作成は既存スレッドを一切書き換えない)がこの操作の対応する不変条件)。
//!
//! **C-05 / AC05-4**: 作成後のタイトル・本文編集は一切不可。編集用のハンドラ・
//! フォーム・リンクはこのファイル・`thread_create.html`・`thread_list.html`のいずれにも
//! 作らない(`formal/Bbs/Op.lean`にスレッド更新操作が存在しないことと対応させる)。
//!
//! `require_auth`配下に置く(`web/mod.rs`)。ログイン中のユーザーのみが作成可能
//! (詳細要件)。二重送信抑止は実装しない(F01の裁定を踏襲。ユーザー裁定により
//! decision 0008のSSR/MPA前提に例外を作らないことを優先——session-logs参照)。

use askama::Template;
use axum::{
    Extension,
    extract::State,
    http::{HeaderValue, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use sqlx::PgPool;

use crate::db;
use crate::db::sessions::AuthenticatedUser;
use crate::domain::model::ValidationFailure;
use crate::domain::validation::create_thread_validation;
use crate::web::csrf::{CsrfForm, CsrfToken};
use crate::web::error::AppError;
use crate::web::params::CreateThreadForm;
use crate::web::views::CurrentUser;

#[derive(Template)]
#[template(path = "thread_create.html")]
struct ThreadCreateTemplate {
    current_user: Option<CurrentUser>,
    /// 共通メッセージエリア(common_layout.md §3 異常系通知)に出す要約。
    form_message: Option<String>,
    title: String,
    body: String,
    title_error: Option<String>,
    body_error: Option<String>,
}

/// 作成画面(初期表示・失敗後の再表示)を描画する。失敗時も入力済みの値
/// (タイトル・本文)は消さない(ui-ux-guidelines §2)。
fn render_form(
    current_user: CurrentUser,
    title: &str,
    body: &str,
    error: Option<ValidationFailure>,
) -> Response {
    let mut title_error = None;
    let mut body_error = None;
    let error_present = error.is_some();

    match error {
        None => {}
        Some(ValidationFailure::TitleEmpty) => {
            title_error = Some("タイトルを入力してください".to_string());
        }
        Some(ValidationFailure::BodyEmpty) => {
            body_error = Some("本文を入力してください".to_string());
        }
        // create_thread_validation()が返しうるのはTitleEmpty/BodyEmptyのみ
        // (formal/Bbs/Op.leanのcreateThread参照)なのでこの腕には到達しない想定。
        Some(_) => {}
    }

    let form_message = error_present
        .then(|| "スレッドを作成できませんでした。入力内容を確認してください。".to_string());

    let tmpl = ThreadCreateTemplate {
        current_user: Some(current_user),
        form_message,
        title: title.to_string(),
        body: body.to_string(),
        title_error,
        body_error,
    };
    let mut response = match tmpl.render() {
        Ok(body) => Html(body).into_response(),
        Err(e) => return AppError::from(e).into_response(),
    };
    // 認証必須画面(C-11): ログアウト後のブラウザバックでキャッシュ経由の表示が
    // 起きないようにする。`require_auth`ミドルウェアでも一括付与されるが、
    // このハンドラ自身が返すバリデーション再表示レスポンスにも明示しておく。
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// GET /threads/new。`require_auth`配下のルートなので`AuthenticatedUser`が必ず存在する。
pub async fn show(
    Extension(user): Extension<AuthenticatedUser>,
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
) -> Response {
    render_form(
        CurrentUser {
            display_name: user.display_name,
            csrf_token,
        },
        "",
        "",
        None,
    )
}

/// POST /threads/new。AC05-1〜AC05-3。
pub async fn submit(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
    CsrfForm(form): CsrfForm<CreateThreadForm>,
) -> Result<Response, AppError> {
    let current_user = CurrentUser {
        display_name: user.display_name.clone(),
        csrf_token,
    };

    let (title, body) = match create_thread_validation(&form.title, &form.body) {
        Ok(v) => v,
        Err(failure) => {
            return Ok(render_form(
                current_user,
                &form.title,
                &form.body,
                Some(failure),
            ));
        }
    };

    // decision 0002(critical): ハンドラの入口でトランザクションを開始し、Errを
    // 返す経路では必ずロールバックする(foundation-plan.md §3、db::with_transaction)。
    db::with_transaction(&pool, move |mut tx| async move {
        let _id = db::threads::insert(&mut *tx, user.user_id, &title, &body).await?;
        // AC05-3: 作成後はスレッド一覧画面(P03)へ遷移する。
        Ok((Redirect::to("/").into_response(), tx))
    })
    .await
}
