//! GET /threads/{id} (P04スレッド詳細画面、F10・issues/10)・
//! POST /threads/{id}/comments (F07コメント作成、issues/07)。
//!
//! **表示範囲(F10)は表示のみ**(ユーザー承認済みのスコープ)。issue 10 のACのうち
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
//!   オラクルとする。
//! - `id`は`Path<i64>`ではなく`Path<String>`で受け、パース失敗もNotFoundに倒す。
//!   `Path<i64>`の既定の失敗経路(axumの400)だとAppErrorを経由せず、この画面が
//!   直す対象のヘッダー不整合(下記)の修正が効かない別経路になってしまう。
//!
//! **F07(コメント作成)**: バリデーション順序は`domain::validation::create_comment_validation`
//! (本文の空チェックのみ、`formal/Bbs/Op.lean`の`createComment`と同じ順序)。
//! `formal/Bbs/Invariant.lean`の`createComment_atomic`(decision 0002: 検査失敗時は
//! 状態を書き換えない)・`createComment_does_not_modify_existing_comments`
//! (C-05/AC07-4: 作成は既存コメントを一切書き換えない)がこの操作の対応する不変条件。
//!
//! **C-05 / AC07-4**: コメント作成後の本文編集は一切不可。編集用のハンドラ・
//! フォーム・リンクはこのファイル・`thread_detail.html`のいずれにも作らない
//! (`formal/Bbs/Op.lean`にコメント更新操作が存在しないことと対応させる)。
//!
//! POSTは`require_auth`配下に置く(`web/mod.rs`)。ログイン中のユーザーのみが
//! コメント作成可能(詳細要件)。二重送信抑止は実装しない(F01・F05の裁定を踏襲。
//! decision 0008のSSR/MPA前提に例外を作らないことを優先——session-logs参照)。
//!
//! **ログイン中に404を踏んだときのヘッダー**: このハンドラは何もしない。
//! 404ページを認証済みヘッダーで描き直すのは`web/middleware.rs`の
//! `reflect_auth_on_error_page`の役目で、`?`でNotFoundを返すだけで正しく出る
//! (F06/F08が後からForbidden/NotFoundを返すようになっても同じ)。

use askama::Template;
use axum::{
    Extension,
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use sqlx::PgPool;

use crate::db;
use crate::db::sessions::AuthenticatedUser;
use crate::db::threads::ThreadDetailRow;
use crate::domain::model::Error as DomainError;
use crate::domain::model::ValidationFailure;
use crate::domain::query::render_comment_body;
use crate::domain::validation::create_comment_validation;
use crate::web::csrf::{CsrfForm, CsrfToken};
use crate::web::error::AppError;
use crate::web::format::format_created_at;
use crate::web::params::CreateCommentForm;
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
    thread_id: i64,
    title: String,
    body: String,
    author_display_name: String,
    created_at: String,
    comments: Vec<CommentItem>,
    /// 共通メッセージエリア(common_layout.md §3 異常系通知)に出す要約(F07)。
    comment_form_message: Option<String>,
    /// 投稿フォームの入力保持(F07、失敗時も入力済みの値を消さない、ui-ux-guidelines §2)。
    comment_body: String,
    comment_error: Option<String>,
}

/// 既に取得済みのスレッド行・コメント一覧から詳細画面を描画する。`show`(初期表示)・
/// `submit`(F07投稿失敗時の再表示)の両方から呼ばれる共通の描画経路。
async fn render_detail(
    pool: &PgPool,
    id: i64,
    thread: ThreadDetailRow,
    current_user: CurrentUser,
    comment_body: String,
    comment_error: Option<ValidationFailure>,
) -> Result<Response, AppError> {
    let comment_rows = db::comments::list_by_thread(pool, id).await?;
    let comments = comment_rows
        .into_iter()
        .map(|c| CommentItem {
            author_display_name: c.author_display_name,
            body: render_comment_body(&c.body, c.deleted).to_string(),
            created_at: format_created_at(c.created_at),
        })
        .collect();

    let mut comment_error_msg = None;
    let error_present = comment_error.is_some();
    match comment_error {
        None => {}
        Some(ValidationFailure::BodyEmpty) => {
            comment_error_msg = Some("本文を入力してください".to_string());
        }
        // create_comment_validation()が返しうるのはBodyEmptyのみ
        // (formal/Bbs/Op.leanのcreateComment参照)なので他の腕には到達しない想定だが、
        // `Some(_)`という捨てワイルドカードにはしない。ワイルドカードだと将来
        // ValidationFailureに新しいvariantが増えたとき、ここが無言でコンパイルを
        // 通り続け、共通メッセージ(comment_form_message)だけが出てフィールド
        // エラー(comment_error_msg)が消えたままになる。列挙し切ることで、
        // variant追加時に「ここを見て判断する」強制を効かせる。
        Some(
            ValidationFailure::UniqueIdInvalid
            | ValidationFailure::PasswordWeak(_)
            | ValidationFailure::DisplayNameTooLong
            | ValidationFailure::DisplayNameEmpty
            | ValidationFailure::TitleEmpty,
        ) => {}
    }
    let comment_form_message = error_present
        .then(|| "コメントを投稿できませんでした。入力内容を確認してください。".to_string());

    let tmpl = ThreadDetailTemplate {
        current_user: Some(current_user),
        thread_id: id,
        title: thread.title,
        body: thread.body,
        author_display_name: thread.author_display_name,
        created_at: format_created_at(thread.created_at),
        comments,
        comment_form_message,
        comment_body,
        comment_error: comment_error_msg,
    };
    Ok(Html(tmpl.render()?).into_response())
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

    render_detail(&pool, id, thread, current_user, String::new(), None).await
}

/// POST /threads/{id}/comments。AC07-1〜AC07-2。
pub async fn create_comment(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
    Path(raw_id): Path<String>,
    CsrfForm(form): CsrfForm<CreateCommentForm>,
) -> Result<Response, AppError> {
    let current_user = CurrentUser {
        display_name: user.display_name.clone(),
        csrf_token,
    };

    let id: i64 = raw_id.parse().map_err(|_| DomainError::NotFound)?;

    // レビュー指摘: スレッド存在確認をINSERTと同じトランザクション内に移した。
    // 以前は`find_by_id`をトランザクション開始前に実行し、INSERTだけを
    // `with_transaction`で包んでいたが、F06(スレッド削除)導入後は確認とINSERTの
    // 間に削除が割り込みうる。その場合「存在しないスレッドへの投稿は404」で
    // あるべき経路が、INSERTのFK違反による500に化けてしまう。decision 0002
    // (critical: 1リクエスト=1トランザクション)の趣旨どおり、読み取りを含めて
    // ハンドラの副作用はこの1トランザクションに収める。
    //
    // render_detail(バリデーション失敗時の再描画)は`&PgPool`を要求するため、
    // トランザクションとは別にpoolのクローンを渡す(PgPoolのクローンは内部Arcの
    // 複製で安価)。再描画はSELECTのみでこのトランザクションへの書き込みは
    // 発生しない。
    let render_pool = pool.clone();
    db::with_transaction(&pool, move |mut tx| async move {
        // `formal/Bbs/Op.lean`の`createComment`と同じ順序: requireAuth(ミドルウェア) →
        // findThread(スレッド存在検査、無ければ404) → 本文空検査。
        let thread = db::threads::find_by_id(&mut *tx, id)
            .await?
            .ok_or(DomainError::NotFound)?;

        let body = match create_comment_validation(&form.body) {
            Ok(b) => b,
            Err(failure) => {
                let response = render_detail(
                    &render_pool,
                    id,
                    thread,
                    current_user,
                    form.body,
                    Some(failure),
                )
                .await?;
                return Ok((response, tx));
            }
        };

        let _id = db::comments::insert(&mut *tx, id, user.user_id, &body).await?;
        // 受け入れ基準「投稿後、即座に詳細表示のコメント一覧に反映される」は
        // D06の解釈によりPOST後のP04再読み込みで満たす(WebSocketは使わない、decision 0008)。
        Ok((Redirect::to(&format!("/threads/{id}")).into_response(), tx))
    })
    .await
}
