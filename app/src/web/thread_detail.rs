//! GET /threads/{id} (P04スレッド詳細画面、F10・issues/10)・
//! POST /threads/{id}/comments (F07コメント作成、issues/07)・
//! POST /threads/{thread_id}/comments/{comment_id}/delete (F08コメント削除、issues/08)・
//! POST /threads/{id}/delete (F06スレッド削除、issues/06)。
//!
//! **表示範囲(F10)は表示のみ**(ユーザー承認済みのスコープ)。issue 10 のACのうち
//! 「自分のスレッドに削除ボタン(コメント0件の場合のみ)」はF06としてこのファイルで
//! 実装する。「自分のコメントに削除ボタン」はF08としてこのファイルで実装する。
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
//! **F06(スレッド削除)**: `formal/Bbs/Op.lean`の`deleteThread`と同じ順序 ――
//! `requireAuth`(ミドルウェア) → `findThread`(存在検査、無ければ404) →
//! 作成者検査(他人のスレッドは`forbidden`) → `commentsOf`/空検査
//! (`threadHasComments`。削除済みコメントも件数に数える、AC06-2) → 物理削除、で
//! 判定する。`deleteThread_atomic`(decision 0002)・`deleteThread_needs_owner`・
//! `deleteThread_blocked_by_any_comment`/`deleteThread_blocked_by_deleted_comment`
//! (C-06/AC06-1〜AC06-2)がこの操作の対応する不変条件。
//! **確認ダイアログは設けない**(コメント削除のD18/decision 0030に倣う。
//! ただしスレッド削除は物理削除でコメント削除より取り返しがつかない ――
//! それでも確認を挟まないのは、`window.confirm`がH-02(agent-browser操作性)を
//! 害しdecision 0008(JSなし)とも非整合という理由がコメント削除の場合と
//! 同様に成り立つため。issue 06のACも確認を必須にしていない)。
//! 削除成功後はスレッド自体が404になるため`/threads/{id}`には戻れず、
//! `/`(一覧、`web/thread_list.rs`)へ`?thread_deleted=1`付きでリダイレクトする
//! (シナリオ02: 「一覧画面へリダイレクトされることを確認する」)。
//! Forbidden/ThreadHasCommentsはスレッドがまだ存在するので`/threads/{id}`へ
//! フラッシュ付きで戻す(F08と同じ観測可能性の方針、H-12)。
//! TOCTOU対策は`db/threads.rs::delete`の条件付き1文+`rows_affected`判定に
//! 委ねる ―― **所有者チェックはその1文に含めない**(所有権は作成後変わらず
//! レース対象ではない。`db/threads.rs`冒頭のdocコメント参照)。
//!
//! POSTは`require_auth`配下に置く(`web/mod.rs`)。ログイン中のユーザーのみが
//! コメント作成・削除・スレッド削除可能(詳細要件)。二重送信抑止は実装しない
//! (F01・F05の裁定を踏襲。decision 0008のSSR/MPA前提に例外を作らないことを
//! 優先——session-logs参照)。
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
use crate::web::params::{CreateCommentForm, DeleteCommentForm, DeleteThreadForm};
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
#[derive(Default)]
enum CommentDeleteNotice {
    #[default]
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

/// F06: `GET /threads/{id}`が受けるスレッド削除失敗結果のフラッシュ通知
/// (`CommentDeleteNotice`と同じクエリパラメータ方式)。成功時はスレッド自体が
/// 消えてこの画面に戻れないため、ここには失敗の2値+通知なししか無い
/// (`CommentDeleteNotice::Deleted`に相当するものは無い)。
#[derive(Default)]
enum ThreadDeleteNotice {
    #[default]
    None,
    Forbidden,
    HasComments,
}

impl ThreadDeleteNotice {
    /// `?thread_delete_error=forbidden|has_comments`を見る。`delete_thread`は
    /// どちらか一方のみを付与する。
    fn from_query(query: &HashMap<String, String>) -> Self {
        match query.get("thread_delete_error").map(String::as_str) {
            Some("forbidden") => Self::Forbidden,
            Some("has_comments") => Self::HasComments,
            _ => Self::None,
        }
    }

    fn message(&self) -> Option<String> {
        match self {
            Self::None => None,
            Self::Forbidden => Some("このスレッドを削除する権限がありません。".to_string()),
            Self::HasComments => {
                Some("コメントが1件以上あるスレッドは削除できません。".to_string())
            }
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
    /// F06/AC06-1〜AC06-3/ui-ux-guidelines §1: ログイン中ユーザー自身の・
    /// コメント0件(削除済み込み)のスレッドかどうか。削除ボタンの表示可否。
    can_delete_thread: bool,
    /// F06: スレッド削除失敗結果のフラッシュ通知(`ThreadDeleteNotice::message`)。
    /// 成功時はこの画面自体に戻らない(`/`へリダイレクトするため)ので成功文言は無い。
    thread_delete_error_message: Option<String>,
}

/// F07投稿フォームの再描画に要る状態。`comment_body`(入力保持)・`error`(検証結果)を
/// 1組で扱う ―― `render_detail`の引数を減らす(clippy::too_many_arguments)ためだけの
/// まとめであり、ドメインの意味的なまとまりを表すものではない。
#[derive(Default)]
struct CommentFormState {
    body: String,
    error: Option<ValidationFailure>,
}

/// F08(コメント削除)・F06(スレッド削除)それぞれの結果フラッシュ通知を1組で扱う。
/// `CommentFormState`と同じ理由(`render_detail`の引数を減らす、
/// clippy::too_many_arguments)だけのまとめで、ドメインの意味的なまとまりではない。
#[derive(Default)]
struct DeleteNotices {
    comment: CommentDeleteNotice,
    thread: ThreadDeleteNotice,
}

/// 既に取得済みのスレッド行・コメント一覧から詳細画面を描画する。`show`(初期表示)・
/// `submit`(F07投稿失敗時の再表示)の両方から呼ばれる共通の描画経路。
/// `current_user_id`はF08の削除ボタン表示可否(`CommentItem::can_delete`)・
/// F06の削除ボタン表示可否(`can_delete_thread`)の判定に使う。
async fn render_detail(
    pool: &PgPool,
    id: i64,
    thread: ThreadDetailRow,
    current_user: CurrentUser,
    current_user_id: i64,
    comment_form: CommentFormState,
    delete_notices: DeleteNotices,
) -> Result<Response, AppError> {
    let comment_rows = db::comments::list_by_thread(pool, id).await?;
    // F06/AC06-1〜AC06-2: 削除ボタンを出せるのは「自分のスレッド」かつ「コメント
    // 0件(削除済みも数える、C-06)」のときだけ。`list_by_thread`は削除済みコメントも
    // 含めて返す(`db/comments.rs`のdocコメント参照)ので、この時点での件数が
    // そのままAC06-2が要求する判定になる ―― `deleteThread`(`formal/Bbs/Op.lean`)の
    // `commentsOf`(削除済みも含めてfilter)と同じ基準。
    let can_delete_thread = thread.author_id == current_user_id && comment_rows.is_empty();
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
    let (delete_success_message, delete_error_message) = delete_notices.comment.messages();
    let thread_delete_error_message = delete_notices.thread.message();

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
        can_delete_thread,
        thread_delete_error_message,
    };
    Ok(Html(tmpl.render()?).into_response())
}

/// GET /threads/{id}。`require_auth`配下(`web/mod.rs`)なので`AuthenticatedUser`が
/// 必ず存在する(C-09: 未ログインは一律`/login`へリダイレクト)。
/// `query`はF08(コメント削除)の結果フラッシュ(`?comment_deleted=1`等)・
/// F06(スレッド削除)の失敗結果フラッシュ(`?thread_delete_error=...`)を
/// (いずれもdecision 0024と同じ方式で)`delete_comment`/`delete_thread`の
/// リダイレクト先として読み取る。
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

    let delete_notices = DeleteNotices {
        comment: CommentDeleteNotice::from_query(&query),
        thread: ThreadDeleteNotice::from_query(&query),
    };
    render_detail(
        &pool,
        id,
        thread,
        current_user,
        user.user_id,
        CommentFormState::default(),
        delete_notices,
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
    // **ただしトランザクションに入れるだけでは閉じない**(F06レビューで実測):
    // 素の`select`は行をロックしないので、「在る」と読んだ後に削除が確定し、
    // `insert`のFK検査が23503を投げる経路が残る。`for share`版で読み、削除側の
    // `for update`と直列化させる(`db/threads.rs::find_by_id_for_share`参照)。
    //
    // render_detail(バリデーション失敗時の再描画)は`&PgPool`を要求するため、
    // トランザクションとは別にpoolのクローンを渡す(PgPoolのクローンは内部Arcの
    // 複製で安価)。再描画はSELECTのみでこのトランザクションへの書き込みは
    // 発生しない。
    let render_pool = pool.clone();
    db::with_transaction(&pool, move |mut tx| async move {
        // `formal/Bbs/Op.lean`の`createComment`と同じ順序: requireAuth(ミドルウェア) →
        // findThread(スレッド存在検査、無ければ404) → 本文空検査。
        let thread = db::threads::find_by_id_for_share(&mut *tx, id)
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
                    DeleteNotices::default(),
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

/// POST /threads/{id}/delete。AC06-1〜AC06-4。
///
/// `formal/Bbs/Op.lean`の`deleteThread`と同じ順序で判定する:
/// `requireAuth`(ミドルウェア) → `findThread`(存在検査、無ければ404) →
/// 作成者検査(他人のスレッドは`forbidden`) → コメント有無検査
/// (`threadHasComments`、削除済みも数える) → 物理削除。Forbidden/
/// ThreadHasCommentsは`?`で`AppError`へ伝播させず、この関数が直接
/// `/threads/{id}`へのフラッシュ付きリダイレクトを返す(F08の`delete_comment`と
/// 同じ方針、`web/error.rs`の一律400フォールバックに頼らない)。
///
/// **所有者チェックとコメント有無チェックの非対称**(このファイル冒頭の
/// docコメント・`db/threads.rs`冒頭のdocコメント参照): 所有者チェックはここ
/// (web層、`find_by_id`が返した`author_id`との比較)で行い、`db::threads::delete`の
/// 原子文には含めない ―― スレッドの所有権は作成後変わらない(譲渡機能なし)ので
/// レース対象ではない。**コメント有無だけ**が他トランザクションとの競合対象なので、
/// `db::threads::delete`の`not exists`サブクエリで原子的に判定する
/// (TOCTOU対策、`db/comments.rs::delete`と同じ形)。
///
/// **確認ダイアログは設けない**(D18/decision 0030と同じ裁定をスレッド削除にも
/// 適用する。このファイル冒頭のdocコメント参照)。
///
/// 削除成功後はスレッド自体が消えて`/threads/{id}`が404になるため、
/// `/`(一覧)へ`?thread_deleted=1`付きでリダイレクトする(シナリオ02)。
pub async fn delete_thread(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(raw_id): Path<String>,
    CsrfForm(_form): CsrfForm<DeleteThreadForm>,
) -> Result<Response, AppError> {
    let id: i64 = raw_id.parse().map_err(|_| DomainError::NotFound)?;

    db::with_transaction(&pool, move |mut tx| async move {
        // レビュー指摘: 素の`find_by_id`ではなく`for update`版でスレッド行をロックする。
        // ロックしないと`db::threads::delete`の`not exists`が同時挿入を取りこぼし、
        // FK違反(23503)で500になる(実測済み。`db/threads.rs::find_by_id_for_update`の
        // docコメント参照)。所有者検査のためにどのみちこの行を読むので、追加コストは
        // ロックのみ。
        let thread = db::threads::find_by_id_for_update(&mut *tx, id)
            .await?
            .ok_or(DomainError::NotFound)?;

        if thread.author_id != user.user_id {
            let response = Redirect::to(&format!("/threads/{id}?thread_delete_error=forbidden"))
                .into_response();
            return Ok((response, tx));
        }

        let deleted = db::threads::delete(&mut *tx, id).await?;
        let response = if deleted {
            Redirect::to("/?thread_deleted=1").into_response()
        } else {
            // 所有者チェックは通ったが`delete`が0行 ＝ この検査の後(あるいは
            // そもそもの時点で)コメントが存在した(AC06-2、TOCTOU対策)。
            Redirect::to(&format!("/threads/{id}?thread_delete_error=has_comments")).into_response()
        };
        Ok((response, tx))
    })
    .await
}
