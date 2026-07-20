//! F04 プロフィール編集(GET /profile/edit 表示・POST /profile/edit 更新)。
//! P06(`docs/product/designs/ui_design.md`、パスはdecision 0020で確定)。
//!
//! 編集可能なのは表示名のみ(issue 04「その他の情報(ユニークID、パスワードなど)の
//! 変更機能は不要」)。バリデーションは`domain::validation::update_display_name_validation`
//! (C-03/decision 0005: 1文字以上15文字以内)。対応するLean側の不変条件は
//! `formal/Bbs/Invariant.lean`の`updateDisplayName_atomic`(decision 0002: 検査失敗時は
//! 状態を書き換えない)・`updateDisplayName_requires_auth`(C-09)。
//!
//! **AC04-2(過去の投稿の表示名反映)はこの操作の副作用ではない。** decision 0015が
//! 採用したJOIN方式により、`threads`・`comments`は`display_name`列を持たず全クエリが
//! `users.display_name`をJOINして解決する。ここでの更新は`users`の1行を書き換えるのみで、
//! 投稿側の表示は次にそれらを問い合わせたときに自動的に新しい値へ追随する
//! (`formal/Bbs/Invariant.lean`の`displayName_propagates`が保証する性質の実装側の対応)。
//!
//! P06は保存成功後もP06自身に留まる(`ui_design.md`画面遷移図:
//! `P06 -- 保存完了 --> P06`)。decision 0024と同じPRGパターン
//! (POST成功→303 See Other→GET `?updated=1`)で、二重送信の防止と
//! ヘッダー(`current_user.display_name`)の再描画を両立する ―― リダイレクト先の
//! GETは`require_auth`ミドルウェアがセッションから`AuthenticatedUser`を読み直すため、
//! 更新後の表示名が自然にヘッダー・入力欄の初期値へ反映される(追加の配線は不要)。
//!
//! `require_auth`配下に置く(`web/mod.rs`)。

use std::collections::HashMap;

use askama::Template;
use axum::{
    Extension,
    extract::{Query, State},
    http::{HeaderValue, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use sqlx::PgPool;

use crate::db;
use crate::db::sessions::AuthenticatedUser;
use crate::domain::model::ValidationFailure;
use crate::domain::validation::update_display_name_validation;
use crate::web::csrf::{CsrfForm, CsrfToken};
use crate::web::error::AppError;
use crate::web::params::ProfileEditForm;
use crate::web::views::CurrentUser;

#[derive(Template)]
#[template(path = "profile_edit.html")]
struct ProfileEditTemplate {
    current_user: Option<CurrentUser>,
    /// 共通メッセージエリア(common_layout.md §3 / ui-ux-guidelines §2・§4)。
    form_message: Option<String>,
    /// decision 0024と同じ方式の正常系通知(`?updated=1`由来)。
    success_message: Option<String>,
    /// 入力欄の現在値。初期表示・失敗後の再表示のいずれでも埋める
    /// (ui-ux-guidelines §2: 失敗時も入力済みの値を消さない)。
    display_name: String,
    display_name_error: Option<String>,
}

/// `login.rs`の`FormNotice`と同じ設計: 成功通知と失敗通知が同時に構築できないことを
/// 型で保証する(片方だけの`Option`2引数にしない)。
enum FormNotice {
    /// 通知なし(POST失敗後の再表示以外の、`?updated`無しの初期表示)。
    None,
    /// decision 0024と同じ方式の`?updated=1`由来の成功通知。
    Updated,
    /// AC04-3の検証失敗通知。
    Error(ValidationFailure),
}

/// 編集画面(初期表示・失敗後の再表示・成功後のリダイレクト先)を描画する。
fn render_form(current_user: CurrentUser, display_name: &str, notice: FormNotice) -> Response {
    let mut display_name_error = None;
    let (success_message, form_message) = match notice {
        FormNotice::None => (None, None),
        FormNotice::Updated => (Some("表示名を変更しました。".to_string()), None),
        FormNotice::Error(ValidationFailure::DisplayNameEmpty) => {
            display_name_error = Some("表示名を入力してください".to_string());
            (
                None,
                Some("変更できませんでした。入力内容を確認してください。".to_string()),
            )
        }
        FormNotice::Error(ValidationFailure::DisplayNameTooLong) => {
            display_name_error = Some("表示名は15文字以内で入力してください".to_string());
            (
                None,
                Some("変更できませんでした。入力内容を確認してください。".to_string()),
            )
        }
        // update_display_name_validation()が返しうるのはDisplayNameEmpty/
        // DisplayNameTooLongのみ(formal/Bbs/Op.leanのupdateDisplayName参照)なので
        // この腕には到達しない想定。login.rsのFormNotice::Error(_)と同じ理由で
        // Noneに落とさず、失敗したこと自体は必ず伝える。
        FormNotice::Error(_) => (
            None,
            Some("変更できませんでした。入力内容を確認してください。".to_string()),
        ),
    };

    let tmpl = ProfileEditTemplate {
        current_user: Some(current_user),
        form_message,
        success_message,
        display_name: display_name.to_string(),
        display_name_error,
    };
    let mut response = match tmpl.render() {
        Ok(body) => Html(body).into_response(),
        Err(e) => return AppError::from(e).into_response(),
    };
    // 認証必須画面(C-11): ログアウト後のブラウザバックでキャッシュ経由の表示が
    // 起きないようにする。`require_auth`ミドルウェアでも一括付与されるが、
    // このハンドラ自身が返すレスポンスにも明示しておく(thread_create.rsと同じ方針)。
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// GET /profile/edit。`require_auth`配下のルートなので`AuthenticatedUser`が必ず存在する。
/// decision 0024と同じ方式: `?updated=1`(値の中身は問わずキーの有無のみ見る)は
/// `POST /profile/edit`成功直後のリダイレクト由来で、更新完了の成功表示を出す。
pub async fn show(
    Extension(user): Extension<AuthenticatedUser>,
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let notice = if query.contains_key("updated") {
        FormNotice::Updated
    } else {
        FormNotice::None
    };
    let display_name = user.display_name.clone();
    render_form(
        CurrentUser {
            display_name: user.display_name,
            csrf_token,
        },
        &display_name,
        notice,
    )
}

/// POST /profile/edit。AC04-1/AC04-3。
pub async fn submit(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
    CsrfForm(form): CsrfForm<ProfileEditForm>,
) -> Result<Response, AppError> {
    let current_user = CurrentUser {
        display_name: user.display_name.clone(),
        csrf_token,
    };

    let trimmed = match update_display_name_validation(&form.display_name) {
        Ok(name) => name,
        Err(failure) => {
            return Ok(render_form(
                current_user,
                &form.display_name,
                FormNotice::Error(failure),
            ));
        }
    };

    // decision 0002(critical): ハンドラの入口でトランザクションを開始し、Errを
    // 返す経路では必ずロールバックする(foundation-plan.md §3、db::with_transaction)。
    // この操作の書き込みは`update_display_name`一度きり。
    db::with_transaction(&pool, move |mut tx| async move {
        db::users::update_display_name(&mut *tx, user.user_id, &trimmed).await?;
        // P06はP06自身に留まる(ui_design.md画面遷移図)。PRGパターンで
        // `/profile/edit?updated=1`へ303リダイレクトし、二重送信を防ぎつつ
        // ヘッダー・入力欄を更新後の表示名で再描画させる(このファイル冒頭のdocコメント)。
        Ok((Redirect::to("/profile/edit?updated=1").into_response(), tx))
    })
    .await
}
