//! P01(ログイン画面)。GET /login 表示・POST /login 認証(F02)。AC02-1〜AC02-3。
//! ID不存在とパスワード誤りは同一のエラー(`DomainError::InvalidCredentials`)に
//! 潰す(列挙攻撃を避けるため。formal/Bbs/Op.lean の `login` と同じ判断で、
//! `formal/Bbs/Invariant.lean` の `login_atomic` がこの操作の原子性
//! ―― 認証に失敗した経路ではセッションを書き込まないこと ―― を証明している)。

use std::collections::HashMap;
use std::sync::OnceLock;

use askama::Template;
use axum::{
    extract::{Extension, Query, State},
    http::{HeaderValue, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use sqlx::PgPool;

use crate::db;
use crate::domain::model::Error as DomainError;
use crate::web::cookies::{append_cookie, build_session_cookie};
use crate::web::csrf::{CsrfForm, CsrfToken, rotate_csrf_cookie};
use crate::web::error::AppError;
use crate::web::params::LoginForm;
use crate::web::views::CurrentUser;

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    current_user: Option<CurrentUser>,
    csrf_token: String,
    /// 共通メッセージエリア(common_layout.md §3 異常系通知)に出す要約。
    /// AC02-3: ID不存在・パスワード誤りのどちらでも同じ文言(ui-ux-guidelines §2の例
    /// そのもの「IDまたはパスワードが正しくありません」)。
    form_message: Option<String>,
    /// decision 0024: `POST /register`成功後のリダイレクト(`/login?registered=1`)を
    /// 受けて出す正常系通知(ui-ux-guidelines §2「成功時は共通メッセージエリアに
    /// 正常系通知を出す」)。H-12でagent-browserが「登録が成功した」ことを
    /// 自然言語から観測できるようにするための表示。
    success_message: Option<String>,
    unique_id: String,
}

/// 共通メッセージエリアに出す通知の種別。
///
/// Why: 以前は`Option<DomainError>`(失敗)と`bool`(成功)の2引数で呼び出し側が
/// 指定していたが、隣り合う異なる意味の引数は取り違えやすい(コンパイラは
/// `render_form(csrf, id, None, true)`と`render_form(csrf, id, Some(e), false)`の
/// どちらも同じ型として受理してしまう)。「成功通知と失敗通知は同時に出ない」
/// という不変条件を、呼び出し側のコメントではなく列挙型のバリアントとして表現し、
/// 両方を同時に持つ状態(`Some(_)`かつ`true`)がそもそも構築できないようにする。
enum FormNotice {
    /// 通知なし(POST /login失敗後の再表示以外の、初期表示で`registered`も無い場合)。
    None,
    /// AC02-3の認証失敗通知。
    Error(DomainError),
    /// decision 0024: `GET /login?registered=1`由来の登録成功通知。
    Registered,
}

/// ログイン画面(初期表示・失敗後の再表示)を描画する。失敗時も入力済みの
/// ユニークIDは消さない(ui-ux-guidelines §2)。パスワードは再表示しない
/// (decision 0022と同じ方針: 機微情報をレスポンスへ不要に載せない)。
///
/// `FormNotice::Registered`はGET /login?registered=1(decision 0024)からのみ渡される。
/// POST /loginの失敗再表示(`submit`内の呼び出し)は常に`FormNotice::Error(..)`を渡す。
fn render_form(csrf_token: String, unique_id: &str, notice: FormNotice) -> Response {
    let (success_message, form_message) = match notice {
        FormNotice::None => (None, None),
        FormNotice::Registered => (
            Some("登録が完了しました。ログインしてください。".to_string()),
            None,
        ),
        FormNotice::Error(DomainError::InvalidCredentials) => (
            None,
            Some("IDまたはパスワードが正しくありません".to_string()),
        ),
        // login()が返しうるエラーはInvalidCredentialsのみ(formal/Bbs/Op.lean参照)なので
        // この腕には到達しない想定。それでもNoneに落とさないのは、落とすと
        // 「エラーを渡したのに画面には何も出ていない200応答」になり、
        // ui-ux-guidelines §2 の失敗フィードバック要件を静かに破るため。
        // 種別を特定できない失敗でも、失敗したこと自体は必ず伝える(register.rsが
        // error_present.then(...)で要約を必ず出すのと同じ非対称を作らない)。
        FormNotice::Error(_) => (
            None,
            Some("ログインできませんでした。入力内容を確認してください。".to_string()),
        ),
    };

    let tmpl = LoginTemplate {
        current_user: None,
        csrf_token,
        form_message,
        success_message,
        unique_id: unique_id.to_string(),
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

/// ユニークIDが存在しなかった経路で「捨てるために」検証する、固定のダミーハッシュ。
///
/// Why: AC02-3はID不存在とパスワード誤りを同一の文言に潰して列挙を防いでいるが、
/// 文言だけを揃えても、ID不存在の経路がargon2の検証(意図的に数十〜数百ms)を
/// 飛ばして即座に返るなら、**応答時間**が「そのIDは存在するか」を答えてしまう
/// (タイミングオラクル)。両経路で必ず1回verifyを回し、この差を埋める。
///
/// 生成はプロセスで一度だけ(`OnceLock`)。毎回ハッシュし直すと、hashはverifyより
/// 重いうえソルトも変わるため、消したい差を別の形で作り直すことになる。
pub fn dummy_password_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| {
        // 値そのものに意味はない。どのパスワードとも一致しないことだけが要件で、
        // 一致しても呼び出し側は結果を捨てるため安全性には影響しない。
        db::password::hash("dummy password for login timing equalization")
            .expect("argon2 hashing of a fixed dummy password must not fail")
    })
}

/// GET /login。
/// decision 0024: `?registered=1`(値の中身は問わずキーの有無のみ見る)は
/// `POST /register`成功直後のリダイレクト由来で、登録完了の成功表示を出す。
pub async fn show(
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let notice = if query.contains_key("registered") {
        FormNotice::Registered
    } else {
        FormNotice::None
    };
    render_form(csrf_token, "", notice)
}

/// POST /login。AC02-1〜AC02-3。
///
/// decision 0002(critical): 状態を変える書き込みは`db::with_transaction`越しに行い、
/// Errを返す経路では必ずロールバックする(foundation-plan.md §3)。
///
/// Why-not: 認証情報の照合(読み取り)とargon2検証をトランザクションの内側に入れない。
/// argon2は意図的に数十〜数百msのCPUを使うため、その間トランザクションを開いたままに
/// すると、プールの接続とPostgres側のバックエンドを`idle in transaction`のまま占有する。
/// `POST /login`は未認証で叩けるので、同時ログイン試行がプールを飽和させ、
/// 同じプールを共有するアプリ全体が接続待ちで止まりうる。この操作の書き込みは
/// セッション作成の1回だけなので、トランザクションを検証通過後に絞っても
/// 「失敗時に部分書き込みが残らない」(formal/Bbs/Invariant.leanの`login_atomic`
/// = NoWriteOnError)は変わらない。
pub async fn submit(
    State(pool): State<PgPool>,
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
    CsrfForm(form): CsrfForm<LoginForm>,
) -> Result<Response, AppError> {
    let user = db::users::find_by_unique_id(&pool, &form.unique_id).await?;

    // Why: ユニークIDが見つからなくてもここで早期returnしない。ID不存在の経路だけ
    // argon2の検証を飛ばすと、応答時間の差が「そのIDは存在するか」を漏らし、
    // 文言を揃えて列挙を防いでいるAC02-3の意図がタイミングチャネルから破られる。
    // 見つからなかった場合は固定のダミーハッシュ(dummy_password_hash)を検証し、
    // 結果は捨てて同じInvalidCredentialsを返す。
    let password_hash = match &user {
        Some(u) => u.password_hash.clone(),
        None => dummy_password_hash().to_string(),
    };

    // argon2は意図的に数十〜数百msのCPUを使う(register.rsのハッシュ化と同じ理由で
    // blockingプールへ逃がす)。ダミー側も同じ経路を通す ―― spawn_blocking越しの
    // 実行なので、結果を捨てても検証そのものが最適化で消えることはない。
    let password = form.password.clone();
    let verified =
        tokio::task::spawn_blocking(move || db::password::verify(&password, &password_hash))
            .await
            .map_err(|e| {
                AppError::internal(format!("password verification task panicked: {e}"))
            })??;

    // ID不存在とパスワード不一致は同一のInvalidCredentialsに潰す(AC02-3)。
    let Some(user) = user else {
        return Ok(render_form(
            csrf_token,
            &form.unique_id,
            FormNotice::Error(DomainError::InvalidCredentials),
        ));
    };

    if !verified {
        return Ok(render_form(
            csrf_token,
            &form.unique_id,
            FormNotice::Error(DomainError::InvalidCredentials),
        ));
    }

    db::with_transaction(&pool, move |mut tx| async move {
        // decision 0007: 多重セッションを許可する(既存セッションを破棄しない)。
        let session_id = db::sessions::create(&mut *tx, user.id).await?;

        let mut response = Redirect::to("/").into_response();
        // 変換失敗を握り潰さない理由は`cookies::append_cookie`のWhy-notに書いてある
        // (ここでエラーになればトランザクションはcommitされずロールバックされる)。
        append_cookie(&mut response, build_session_cookie(session_id))?;
        // decision 0021 決定5: ログイン成功時にCSRFトークンをローテーションする
        // (F01から持ち越されていた実装。F02のスコープ)。
        rotate_csrf_cookie(&mut response);
        Ok((response, tx))
    })
    .await
}
