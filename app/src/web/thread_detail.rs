//! GET /threads/{id} (P04スレッド詳細画面)。F10(スレッド詳細表示、issues/10)。
//!
//! **範囲は表示のみ**(ユーザー承認済みのスコープ)。issue 10 のACのうち
//! 「自分のスレッドに削除ボタン(コメント0件の場合のみ)」「自分のコメントに削除
//! ボタン」はそれぞれF06・F08の範囲であり、ここでは実装しない
//! (F09がソート切替UIをF12へ切り出したのと同じ扱い)。
//!
//! - スレッド削除は物理削除(decision 0014)なので、`threads`に行が無い ＝
//!   「存在しない」「削除済み」のどちらも一律`DomainError::NotFound`(C-10)。
//! - コメントは論理削除(C-07)なので`deleted_at`の有無で判定し、本文は
//!   `domain::query::render_comment_body`で固定文言(C-01)に差し替える。作成者・
//!   作成日時は削除済みでも維持する(AC10-3)。`formal/Bbs/Invariant.lean`の
//!   `deleted_comment_renders_fixed_text`・`deleted_comment_keeps_metadata`を
//!   オラクルとする(このセッションで証明済み)。
//! - `id`は`Path<i64>`ではなく`Path<String>`で受け、パース失敗もNotFoundに倒す。
//!   `Path<i64>`の既定の失敗経路(axumの400)だとAppErrorを経由せず、この画面が
//!   直す対象のヘッダー不整合(下記)の修正が効かない別経路になってしまう。
//!
//! **ログイン中に404を踏んだときのヘッダー**: このハンドラは何もしない。
//! 404ページを認証済みヘッダーで描き直すのは`web/middleware.rs`の
//! `reflect_auth_on_error_page`の役目で、`?`でNotFoundを返すだけで正しく出る
//! (F06/F08が後からForbidden/NotFoundを返すようになっても同じ)。

use askama::Template;
use axum::{
    Extension,
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
};
use sqlx::PgPool;

use crate::db;
use crate::db::sessions::AuthenticatedUser;
use crate::domain::model::Error as DomainError;
use crate::domain::query::render_comment_body;
use crate::web::csrf::CsrfToken;
use crate::web::error::AppError;
use crate::web::format::format_created_at;
use crate::web::views::CurrentUser;

/// コメント一覧に描画する1件ぶんの行。本文は表示時点で固定文言への差し替えを
/// 済ませてある(`render_comment_body`)。
struct CommentItem {
    author_display_name: String,
    body: String,
    created_at: String,
}

#[derive(Template)]
#[template(path = "thread_detail.html")]
struct ThreadDetailTemplate {
    current_user: Option<CurrentUser>,
    title: String,
    body: String,
    author_display_name: String,
    created_at: String,
    comments: Vec<CommentItem>,
}

/// GET /threads/{id}。`require_auth`配下(`web/mod.rs`)なので`AuthenticatedUser`が
/// 必ず存在する(C-09: 未ログインは一律`/login`へリダイレクト)。
pub async fn show(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
    Path(raw_id): Path<String>,
) -> Result<Response, AppError> {
    let current_user = CurrentUser {
        display_name: user.display_name,
        csrf_token,
    };

    // 数字以外のID(`/threads/abc`)もC-10の「存在しない」と同じ扱いにする。
    let id: i64 = raw_id.parse().map_err(|_| DomainError::NotFound)?;

    let thread = db::threads::find_by_id(&pool, id)
        .await?
        .ok_or(DomainError::NotFound)?;

    let comment_rows = db::comments::list_by_thread(&pool, id).await?;
    let comments = comment_rows
        .into_iter()
        .map(|c| CommentItem {
            author_display_name: c.author_display_name,
            body: render_comment_body(&c.body, c.deleted).to_string(),
            created_at: format_created_at(c.created_at),
        })
        .collect();

    let tmpl = ThreadDetailTemplate {
        current_user: Some(current_user),
        title: thread.title,
        body: thread.body,
        author_display_name: thread.author_display_name,
        created_at: format_created_at(thread.created_at),
        comments,
    };
    Ok(Html(tmpl.render()?).into_response())
}
