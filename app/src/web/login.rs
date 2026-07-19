//! P01(ログイン画面)。GET /login 表示・POST /login 認証(F02)。AC02-1〜AC02-3。
//! ID不存在とパスワード誤りは同一のエラー(`DomainError::InvalidCredentials`)に
//! 潰す(列挙攻撃を避けるため。formal/Bbs/Op.lean の `login` と同じ判断で、
//! `formal/Bbs/Invariant.lean` の `login_atomic` がこの操作の原子性
//! ―― 認証に失敗した経路ではセッションを書き込まないこと ―― を証明している)。

use std::sync::OnceLock;

use askama::Template;
use axum::{
    extract::{Extension, State},
    http::{HeaderValue, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use sqlx::PgPool;

use crate::db;
use crate::domain::model::Error as DomainError;
use crate::web::cookies::build_session_cookie;
use crate::web::csrf::{CsrfForm, CsrfToken};
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
    unique_id: String,
}

/// ログイン画面(初期表示・失敗後の再表示)を描画する。失敗時も入力済みの
/// ユニークIDは消さない(ui-ux-guidelines §2)。パスワードは再表示しない
/// (decision 0022と同じ方針: 機微情報をレスポンスへ不要に載せない)。
fn render_form(csrf_token: String, unique_id: &str, error: Option<DomainError>) -> Response {
    let form_message = match error {
        None => None,
        Some(DomainError::InvalidCredentials) => {
            Some("IDまたはパスワードが正しくありません".to_string())
        }
        // login()が返しうるエラーはInvalidCredentialsのみ(formal/Bbs/Op.lean参照)なので
        // この腕には到達しない想定。それでもNoneに落とさないのは、落とすと
        // 「エラーを渡したのに画面には何も出ていない200応答」になり、
        // ui-ux-guidelines §2 の失敗フィードバック要件を静かに破るため。
        // 種別を特定できない失敗でも、失敗したこと自体は必ず伝える(register.rsが
        // error_present.then(...)で要約を必ず出すのと同じ非対称を作らない)。
        Some(_) => Some("ログインできませんでした。入力内容を確認してください。".to_string()),
    };

    let tmpl = LoginTemplate {
        current_user: None,
        csrf_token,
        form_message,
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
pub async fn show(Extension(CsrfToken(csrf_token)): Extension<CsrfToken>) -> Response {
    render_form(csrf_token, "", None)
}

/// POST /login。AC02-1〜AC02-3。
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
                AppError::Internal(format!("password verification task panicked: {e}"))
            })??;

    // ID不存在とパスワード不一致は同一のInvalidCredentialsに潰す(AC02-3)。
    let Some(user) = user else {
        return Ok(render_form(
            csrf_token,
            &form.unique_id,
            Some(DomainError::InvalidCredentials),
        ));
    };

    if !verified {
        return Ok(render_form(
            csrf_token,
            &form.unique_id,
            Some(DomainError::InvalidCredentials),
        ));
    }

    // decision 0007: 多重セッションを許可する(既存セッションを破棄しない)。
    let session_id = db::sessions::create(&pool, user.id).await?;

    let mut response = Redirect::to("/").into_response();
    let cookie = build_session_cookie(session_id);
    // Why-not: 変換失敗を`if let Ok(..)`で読み飛ばさない。セッションCookieを
    // 載せられなかったのに「/へリダイレクト」だけ返すと、DBにはセッションが
    // 残っているのにクライアントは未ログインのまま、という「失敗したのに成功した
    // ことになる」応答になる。UUID由来の値なのでHeaderValueへの変換は実際には
    // 失敗しないが、到達不能であることと握り潰してよいことは別なので、
    // エラーとして伝播させ、この操作全体を失敗させる。
    let value = HeaderValue::from_str(&cookie.to_string()).map_err(|e| {
        AppError::Internal(format!("session cookie is not a valid header value: {e}"))
    })?;
    response.headers_mut().append(header::SET_COOKIE, value);
    Ok(response)
}
