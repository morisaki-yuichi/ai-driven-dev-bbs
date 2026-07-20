//! GET /threads/{id} (P04スレッド詳細画面、F10・issues/10)・
//! POST /threads/{id}/comments (F07コメント作成、issues/07)・
//! POST /threads/{thread_id}/comments/{comment_id}/delete (F08コメント削除、issues/08)。
//!
//! **表示範囲(F10)は表示のみ**(ユーザー承認済みのスコープ)。issue 10 のACのうち
//! 「自分のスレッドに削除ボタン(コメント0件の場合のみ)」はF06の範囲であり、
//! ここでは実装しない(F06は本セッション時点でRust未実装のまま)。「自分の
//! コメントに削除ボタン」はF08としてこのファイルで実装する。
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
//! **F08(コメント削除)**: `formal/Bbs/Op.lean`の`deleteComment`と同じ順序 ――
//! `requireAuth`(ミドルウェア) → `findComment`(存在検査、無ければ404) →
//! 作成者検査(他人のコメントは`forbidden`) → 未削除検査(`alreadyDeleted`) →
//! 論理削除、で判定する。`deleteComment_atomic`(decision 0002)・
//! `deletion_irreversible`(C-07/C-08)がこの操作の対応する不変条件。
//! **D18: 確認なしで即削除する**(`window.confirm`は使わない。decision 0008の
//! JSなし前提とH-02のagent-browser操作性を優先。decision 0030参照)。
//! Forbidden/AlreadyDeletedは`web/error.rs`の一律400フォールバックに流さず、
//! このハンドラが明示的に捕捉して`/threads/{id}`への再リダイレクト+
//! クエリパラメータ経由のフラッシュ通知(decision 0024と同じ方式)で
//! 画面上に観測可能なフィードバックを返す(ui-ux-guidelines §1・§2、H-12)。
//!
//! POSTは`require_auth`配下に置く(`web/mod.rs`)。ログイン中のユーザーのみが
//! コメント作成・削除可能(詳細要件)。二重送信抑止は実装しない(F01・F05の裁定を
//! 踏襲。decision 0008のSSR/MPA前提に例外を作らないことを優先——session-logs参照)。
//!
//! **ログイン中に404を踏んだときのヘッダー**: このハンドラは何もしない。
//! 404ページを認証済みヘッダーで描き直すのは`web/middleware.rs`の
//! `reflect_auth_on_error_page`の役目で、`?`でNotFoundを返すだけで正しく出る。

use std::collections::HashMap;

use askama::Template;
use axum::{
    Extension,
    extract::{Path, Query, State},
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
use crate::web::params::{CreateCommentForm, DeleteCommentForm};
use crate::web::views::CurrentUser;

/// コメント一覧に描画する1件ぶんの行。本文は表示時点で固定文言への差し替えを
/// 済ませてある(`render_comment_body`)。`id`は削除フォームのURL生成に使う。
/// `can_delete`は「ログイン中ユーザー自身の・未削除のコメントか」(AC08-3/AC08-4:
/// 他人のコメント・削除済みコメントには削除ボタンを出さない、
/// ui-ux-guidelines §1の「操作不可は非表示にする」要件)。
struct CommentItem {
    id: i64,
    author_display_name: String,
    body: String,
    created_at: String,
    can_delete: bool,
}

/// F08: `GET /threads/{id}`が受けるコメント削除結果のフラッシュ通知(decision 0024と
/// 同じクエリパラメータ方式)。成功・失敗(forbidden/alreadyDeleted)の3値+通知なし。
enum CommentDeleteNotice {
    None,
    Deleted,
    Forbidden,
    AlreadyDeleted,
}

impl CommentDeleteNotice {
    /// クエリパラメータから読み取る。`?comment_deleted=1`(値は問わずキーの有無のみ、
    /// decision 0024と同じ)を優先し、無ければ`?comment_delete_error=forbidden|already_deleted`
    /// を見る。両方同時に来ることは無い(`delete_comment`はどちらか一方のみを付与する)。
    fn from_query(query: &HashMap<String, String>) -> Self {
        if query.contains_key("comment_deleted") {
            return Self::Deleted;
        }
        match query.get("comment_delete_error").map(String::as_str) {
            Some("forbidden") => Self::Forbidden,
            Some("already_deleted") => Self::AlreadyDeleted,
            _ => Self::None,
        }
    }

    /// (成功メッセージ, 失敗メッセージ)。同時に`Some`にはならない
    /// (`FormNotice`とdecision 0024のaria-live使い分けと同じ設計)。
    fn messages(&self) -> (Option<String>, Option<String>) {
        match self {
            Self::None => (None, None),
            Self::Deleted => (Some("コメントを削除しました。".to_string()), None),
            Self::Forbidden => (
                None,
                Some("このコメントを削除する権限がありません。".to_string()),
            ),
            Self::AlreadyDeleted => (
                None,
                Some("このコメントは既に削除されています。".to_string()),
            ),
        }
    }
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
    /// F08: コメント削除結果のフラッシュ通知(`CommentDeleteNotice::messages`)。
    delete_success_message: Option<String>,
    delete_error_message: Option<String>,
}

/// F07投稿フォームの再描画に要る状態。`comment_body`(入力保持)・`error`(検証結果)を
/// 1組で扱う ―― `render_detail`の引数を減らす(clippy::too_many_arguments)ためだけの
/// まとめであり、ドメインの意味的なまとまりを表すものではない。
#[derive(Default)]
struct CommentFormState {
    body: String,
    error: Option<ValidationFailure>,
}

/// 既に取得済みのスレッド行・コメント一覧から詳細画面を描画する。`show`(初期表示)・
/// `submit`(F07投稿失敗時の再表示)の両方から呼ばれる共通の描画経路。
/// `current_user_id`はF08の削除ボタン表示可否(`CommentItem::can_delete`)の判定に使う。
async fn render_detail(
    pool: &PgPool,
    id: i64,
    thread: ThreadDetailRow,
    current_user: CurrentUser,
    current_user_id: i64,
    comment_form: CommentFormState,
    delete_notice: CommentDeleteNotice,
) -> Result<Response, AppError> {
    let comment_rows = db::comments::list_by_thread(pool, id).await?;
    let comments = comment_rows
        .into_iter()
        .map(|c| CommentItem {
            id: c.id,
            author_display_name: c.author_display_name,
            body: render_comment_body(&c.body, c.deleted).to_string(),
            created_at: format_created_at(c.created_at),
            can_delete: c.author_id == current_user_id && !c.deleted,
        })
        .collect();

    let CommentFormState {
        body: comment_body,
        error: comment_error,
    } = comment_form;
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
    let (delete_success_message, delete_error_message) = delete_notice.messages();

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
        delete_success_message,
        delete_error_message,
    };
    Ok(Html(tmpl.render()?).into_response())
}

/// GET /threads/{id}。`require_auth`配下(`web/mod.rs`)なので`AuthenticatedUser`が
/// 必ず存在する(C-09: 未ログインは一律`/login`へリダイレクト)。
/// `query`はF08(コメント削除)の結果フラッシュ(`?comment_deleted=1`等、decision 0024と
/// 同じ方式)を`delete_comment`のリダイレクト先として読み取る。
pub async fn show(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
    Path(raw_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
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

    let delete_notice = CommentDeleteNotice::from_query(&query);
    render_detail(
        &pool,
        id,
        thread,
        current_user,
        user.user_id,
        CommentFormState::default(),
        delete_notice,
    )
    .await
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
                    user.user_id,
                    CommentFormState {
                        body: form.body,
                        error: Some(failure),
                    },
                    CommentDeleteNotice::None,
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

/// POST /threads/{thread_id}/comments/{comment_id}/delete。AC08-1〜AC08-4。
///
/// `formal/Bbs/Op.lean`の`deleteComment`と同じ順序で判定する:
/// `requireAuth`(ミドルウェア) → `findComment`(存在検査、無ければ404) →
/// 作成者検査(他人のコメントは`forbidden`) → 未削除検査(`alreadyDeleted`) →
/// 論理削除。Forbidden/AlreadyDeletedは`?`で`AppError`へ伝播させず、この関数が
/// 直接`/threads/{thread_id}`へのフラッシュ付きリダイレクトを返す ――
/// `web/error.rs`の一律400フォールバック(ハンドラの実装漏れの安全網)に頼らず、
/// 画面上で観測できるフィードバックを返すため(ユーザー承認済みのスコープ)。
///
/// URLの`thread_id`セグメントは、コメントが実際に属するスレッドと一致するかを
/// 検査する(不一致はネスト構造が壊れているとみなしC-10の404)。リダイレクト先には
/// URLからパース済みの`thread_id`をそのまま埋める ―― 直上の等値ガードを通った
/// 時点で`find_ownership`が返した`thread_id`と一致しているため、どちらを使っても
/// 同じURLになる。
///
/// **AC08-4(再削除)の判定は`db::comments::delete`の戻り値で行う**。
/// `find_ownership`の`deleted`による事前検査は早期リターンのための最適化であり、
/// 判定の根拠ではない ―― `find_ownership`と`delete`の間には行ロックが無く、
/// 同時削除が双方ともこの検査を通過しうるため(F08レビュー指摘のTOCTOU)。
/// 実際に未削除→削除済みへ遷移させたトランザクションだけが成功文言を出し、
/// 競り負けた側は`AlreadyDeleted`のフィードバックを受ける。
pub async fn delete_comment(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((raw_thread_id, raw_comment_id)): Path<(String, String)>,
    CsrfForm(_form): CsrfForm<DeleteCommentForm>,
) -> Result<Response, AppError> {
    let thread_id: i64 = raw_thread_id.parse().map_err(|_| DomainError::NotFound)?;
    let comment_id: i64 = raw_comment_id.parse().map_err(|_| DomainError::NotFound)?;

    db::with_transaction(&pool, move |mut tx| async move {
        let ownership = db::comments::find_ownership(&mut *tx, comment_id)
            .await?
            .ok_or(DomainError::NotFound)?;
        if ownership.thread_id != thread_id {
            return Err(DomainError::NotFound.into());
        }

        if ownership.author_id != user.user_id {
            let response = Redirect::to(&format!(
                "/threads/{thread_id}?comment_delete_error=forbidden"
            ))
            .into_response();
            return Ok((response, tx));
        }
        // 早期リターン(最適化)。真の判定は下の`delete`の戻り値が行う ―― 上記docコメント参照。
        if ownership.deleted {
            let response = Redirect::to(&format!(
                "/threads/{thread_id}?comment_delete_error=already_deleted"
            ))
            .into_response();
            return Ok((response, tx));
        }

        let deleted_now = db::comments::delete(&mut *tx, comment_id).await?;
        let response = if deleted_now {
            Redirect::to(&format!("/threads/{thread_id}?comment_deleted=1")).into_response()
        } else {
            // 事前検査は通ったが`update`が0行 ＝ この検査の後に別トランザクションが
            // 先に削除を確定させた(AC08-4)。二重に「削除しました」を出さない。
            Redirect::to(&format!(
                "/threads/{thread_id}?comment_delete_error=already_deleted"
            ))
            .into_response()
        };
        Ok((response, tx))
    })
    .await
}
