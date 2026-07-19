//! F01 ユーザー登録(GET /register 表示・POST /register 登録)。P02(ui_design.md)。
//! バリデーション順序は`domain::validation::register_validation`(decision 0006)。
//! 対応するLean側の不変条件・場合分けは formal/Bbs/Invariant.lean の
//! `register_atomic`(decision 0002: 検査失敗時は状態を書き換えない)。

use askama::Template;
use axum::{
    extract::{Extension, State},
    http::{HeaderValue, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use sqlx::PgPool;

use crate::db;
use crate::domain::model::{Error as DomainError, PasswordWeakness, ValidationFailure};
use crate::domain::validation::register_validation;
use crate::web::csrf::{CsrfForm, CsrfToken};
use crate::web::error::AppError;
use crate::web::params::RegisterForm;
use crate::web::views::CurrentUser;

#[derive(Template)]
#[template(path = "register.html")]
struct RegisterTemplate {
    current_user: Option<CurrentUser>,
    csrf_token: String,
    /// 共通メッセージエリア(common_layout.md §3 異常系通知)に出す要約。
    /// 個別の項目エラーとは別に、「送信は失敗した」という結果そのものを1箇所で伝える
    /// (ui-ux-guidelines §2)。`aria-live`付きの領域に描画される(同 §4)。
    form_message: Option<String>,
    unique_id: String,
    display_name: String,
    unique_id_error: Option<String>,
    password_errors: Vec<String>,
    display_name_error: Option<String>,
}

/// decision 0006の対応表。文言そのものはUI層の関心(domain/model.rsのコメント参照)
/// なのでここに置く。原典は文言の完全一致を要求していない。
fn password_weakness_text(weakness: PasswordWeakness) -> &'static str {
    match weakness {
        PasswordWeakness::TooShort => "12文字以上で入力してください",
        PasswordWeakness::NoAlpha => "英字を含めてください",
        PasswordWeakness::NoDigit => "数字を含めてください",
        PasswordWeakness::NoSymbol => "記号を含めてください",
        PasswordWeakness::DisallowedChar => {
            "使用できない文字が含まれています(使えるのは英数字と記号 !@#$%^&*()_+-=[]{}|;':\",./<>? です)"
        }
    }
}

/// 登録画面(初期表示・失敗後の再表示)を描画する。
/// `error`が`None`のときは初期表示、`Some`のときは該当フィールドにエラーを出す
/// (decision 0006: 項目ごとに該当フィールド付近へ表示)。
/// 失敗時も入力済みの値(ユニークID・表示名)を消さない(ui-ux-guidelines §2)。
/// パスワードだけは再表示時に値を保持しない(decision 0022: 機微情報をレスポンスへ
/// 不要に載せないことを優先する)。
fn render_form(
    csrf_token: String,
    unique_id: &str,
    display_name: &str,
    error: Option<DomainError>,
) -> Response {
    let mut unique_id_error = None;
    let mut password_errors = Vec::new();
    let mut display_name_error = None;
    let error_present = error.is_some();

    match error {
        None => {}
        Some(DomainError::DuplicateUniqueId) => {
            unique_id_error = Some("このIDは既に使用されています".to_string());
        }
        Some(DomainError::Validation(ValidationFailure::UniqueIdInvalid)) => {
            unique_id_error = Some("ユニークIDを入力してください".to_string());
        }
        Some(DomainError::Validation(ValidationFailure::PasswordWeak(weaknesses))) => {
            password_errors = weaknesses
                .into_iter()
                .map(|w| password_weakness_text(w).to_string())
                .collect();
        }
        Some(DomainError::Validation(ValidationFailure::DisplayNameEmpty)) => {
            display_name_error = Some("表示名を入力してください".to_string());
        }
        Some(DomainError::Validation(ValidationFailure::DisplayNameTooLong)) => {
            display_name_error = Some("表示名は15文字以内で入力してください".to_string());
        }
        // register()が返しうるエラーはここまで(Forbidden等は生じない)。
        Some(_) => {}
    }

    // 失敗したことは、項目ごとのエラーとは別に要約としても伝える(ui-ux-guidelines §2:
    // 送信中/成功/失敗の3状態を区別する)。文言だけで判別でき、色に依存しない(同 §4)。
    let form_message =
        error_present.then(|| "登録できませんでした。入力内容を確認してください。".to_string());

    let tmpl = RegisterTemplate {
        current_user: None,
        csrf_token,
        form_message,
        unique_id: unique_id.to_string(),
        display_name: display_name.to_string(),
        unique_id_error,
        password_errors,
        display_name_error,
    };
    let mut response = match tmpl.render() {
        Ok(body) => Html(body).into_response(),
        Err(e) => return AppError::from(e).into_response(),
    };
    // 未認証画面だがCSRFトークンを含むHTMLを返すため、キャッシュされないようにする
    // (decision 0021)。
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// GET /register。
pub async fn show(Extension(CsrfToken(csrf_token)): Extension<CsrfToken>) -> Response {
    render_form(csrf_token, "", "", None)
}

/// POST /register。AC01-1〜AC01-6。
pub async fn submit(
    State(pool): State<PgPool>,
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
    CsrfForm(form): CsrfForm<RegisterForm>,
) -> Result<Response, AppError> {
    let trimmed_display_name =
        match register_validation(&form.unique_id, &form.password, &form.display_name) {
            Ok(name) => name,
            Err(failure) => {
                return Ok(render_form(
                    csrf_token,
                    &form.unique_id,
                    &form.display_name,
                    Some(DomainError::Validation(failure)),
                ));
            }
        };

    // argon2は意図的に数十〜数百msのCPUを使う。asyncハンドラ内で直接呼ぶと
    // その間tokioのワーカースレッドを占有し、他のリクエストの進行を止めるため
    // blockingプールへ逃がす。
    // Why-not: ハッシュ化と重複判定の**順序は変えない**。ユニークIDの重複は
    // 事前SELECTではなくINSERTの23505検知に委ねてTOCTOUを避けており(下の
    // is_unique_violation分岐)、「重複を先に見てからハッシュ化する」ように
    // 組み替えるとその構造が崩れる。
    let password = form.password.clone();
    let password_hash = tokio::task::spawn_blocking(move || db::password::hash(&password))
        .await
        .map_err(|e| AppError::Internal(format!("password hashing task panicked: {e}")))??;

    // decision 0002(critical): ハンドラの入口でトランザクションを開始し、Errを
    // 返す経路では必ずロールバックする規律を`db::with_transaction`に集約している
    // (foundation-plan.md §3)。この操作は書き込みが`insert`一度きりなので
    // 実質的な効果は従来と変わらないが、書き方をlogin.rs・logout.rsと揃える。
    let unique_id = form.unique_id.clone();
    let result = db::with_transaction(&pool, move |mut tx| async move {
        let _id = db::users::insert(&mut *tx, &unique_id, &password_hash, &trimmed_display_name)
            .await
            // Why: 重複を検出した経路は`Err`で抜ける。ここで再描画したレスポンスを
            // `Ok`として返すと`with_transaction`が`commit`を呼ぶが、23505を起こした
            // トランザクションはPostgres側で既にアボート状態にあり、commitは
            // 暗黙のROLLBACKになる(意図した終わり方と実際の終わり方が食い違う)。
            // `Err`で抜ければ`tx`はcommitされずにdropされ、sqlxが明示的に
            // ROLLBACKを送る —— decision 0002のNoWriteOnErrorが、
            // 「たまたまそうなる」ではなく構造として保証される。
            .map_err(|e| match db::users::is_unique_violation(&e) {
                true => AppError::Domain(DomainError::DuplicateUniqueId),
                false => AppError::from(e),
            })?;
        // C-18: 登録はセッションを作らない。AC01-5: ログイン画面へリダイレクトする。
        // decision 0024: `registered=1` はログイン画面側で登録完了の成功表示を
        // 出すためのフラッシュ用クエリパラメータ(H-12の自然言語観測性)。
        Ok((Redirect::to("/login?registered=1").into_response(), tx))
    })
    .await;

    match result {
        Ok(response) => Ok(response),
        // AC01-4: 重複は500やエラーページではなく、登録画面のユニークID欄への
        // インライン表示として返す(ui-ux-guidelinesの二重バリデーション要件)。
        // `AppError`の既定の写像(error.rs)は400を返すので、ここで捕まえる。
        Err(AppError::Domain(DomainError::DuplicateUniqueId)) => Ok(render_form(
            csrf_token,
            &form.unique_id,
            &form.display_name,
            Some(DomainError::DuplicateUniqueId),
        )),
        Err(e) => Err(e),
    }
}
