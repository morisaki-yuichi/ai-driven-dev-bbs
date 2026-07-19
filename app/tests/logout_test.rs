//! F03ログアウトの結合テスト(Router全体 = ミドルウェア込みでAction層まで通す)。
//! AC03-1〜AC03-3、C-09、C-11、decision 0002(1リクエスト=1トランザクション)、
//! decision 0007(多重セッション許可)、decision 0021(CSRF)。
//! formal/Bbs/Invariant.leanの`logout_requires_auth`(未ログインでは`sessions`に
//! 一切触れず失敗する)・`logout_removes_only_target_session`(対象セッションだけを
//! 消し、同一利用者の別セッションには影響しない)をオラクルとして、対応する
//! 結合テストを置く。

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;
use common::{HOST, insert_test_user, origin_header, urlencoding_stub};

/// Set-Cookieの並びから`name=`で始まる1件の"name=value"部分(属性を含まない)を拾う。
fn find_cookie_pair<'a>(set_cookies: &'a [String], name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=");
    set_cookies
        .iter()
        .find(|c| c.starts_with(&prefix))
        .map(|c| c.split(';').next().unwrap())
}

/// ユーザーを登録済みの状態から実際に`POST /login`を通し、
/// (`session_id=...; csrf_token=...`のCookieヘッダ, レスポンス全体)を返す。
/// login_test.rsの各ヘルパと同じ形。
async fn login(pool: &PgPool, unique_id: &str, plain_password: &str) -> String {
    let app = bbs::web::build_router(pool.clone());
    let get_response = app
        .oneshot(
            Request::builder()
                .uri("/login")
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let set_cookie = get_response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let initial_csrf_token = set_cookie
        .split(';')
        .next()
        .unwrap()
        .strip_prefix("csrf_token=")
        .unwrap()
        .to_string();

    let form = format!(
        "unique_id={}&password={}&csrf_token={}",
        urlencoding_stub(unique_id),
        urlencoding_stub(plain_password),
        urlencoding_stub(&initial_csrf_token),
    );
    let app = bbs::web::build_router(pool.clone());
    let login_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, origin_header())
                .header(header::COOKIE, format!("csrf_token={initial_csrf_token}"))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login_response.status(), StatusCode::SEE_OTHER);

    let set_cookies: Vec<String> = login_response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    let session_pair = find_cookie_pair(&set_cookies, "session_id")
        .expect("login should set a session_id cookie")
        .to_string();
    let csrf_pair = find_cookie_pair(&set_cookies, "csrf_token")
        .expect("login should rotate the csrf_token cookie")
        .to_string();

    format!("{session_pair}; {csrf_pair}")
}

fn post_logout_request(cookie_header: &str, csrf_token: &str) -> Request<Body> {
    let form = format!("csrf_token={}", urlencoding_stub(csrf_token));
    Request::builder()
        .method("POST")
        .uri("/logout")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, origin_header())
        .header(header::COOKIE, cookie_header)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .unwrap()
}

fn csrf_token_from_cookie_header(cookie_header: &str) -> String {
    cookie_header
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("csrf_token="))
        .expect("cookie header should carry csrf_token")
        .to_string()
}

/// AC09-1/シナリオ01前提: "/"がlayout.html経由で描画され、ログイン中の
/// 表示名とログアウトボタン(POSTフォーム)がHTMLに現れる。従来の"ok"という
/// プレーンテキスト応答では、agent-browserがログアウトボタンを操作できず
/// シナリオ01の通し実行ができなかった(F03着手の前提)。
#[sqlx::test]
async fn get_root_renders_layout_with_visible_logout_button(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::HOST, HOST)
                .header(header::COOKIE, cookie_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    // C-01固定文言(F14)とAC09-1の表示名。
    assert!(html.contains("テストユーザー01 さん"));
    assert!(html.contains(r#"action="/logout""#));
    assert!(html.contains(r#"name="csrf_token""#));
}

/// AC03-1: ログアウトボタン(POST /logout)で即座にログイン画面へリダイレクトされる。
/// formal/Bbs/Invariant.leanの`logout_removes_only_target_session`の効果本体
/// (対象セッションがsessionsテーブルから消える)をDBに問い合わせて確認する。
#[sqlx::test]
async fn post_logout_redirects_to_login_and_deletes_the_session(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let count_before: (i64,) = sqlx::query_as("select count(*) from sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_before.0, 1);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_logout_request(&cookie_header, &csrf_token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");

    let count_after: (i64,) = sqlx::query_as("select count(*) from sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_after.0, 0);
}

/// ログアウト応答はセッションCookieを失効させる(Max-Age=0)。
/// C-11: サーバ側のセッション破棄と揃え、クライアント側にも残さない。
#[sqlx::test]
async fn post_logout_expires_the_session_cookie_on_the_client(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_logout_request(&cookie_header, &csrf_token))
        .await
        .unwrap();

    let set_cookies: Vec<String> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    let removal = set_cookies
        .iter()
        .find(|c| c.starts_with("session_id="))
        .expect("expected a session_id removal cookie");
    assert!(
        removal.contains("Max-Age=0"),
        "expected an expiring session_id cookie, got {removal}"
    );

    // decision 0021 決定5: ログアウト時にもCSRFトークンをローテーションする。
    let rotated_csrf = set_cookies
        .iter()
        .find(|c| c.starts_with("csrf_token="))
        .expect("expected a rotated csrf_token cookie");
    assert!(
        !rotated_csrf.starts_with(&format!("csrf_token={csrf_token};"))
            && rotated_csrf != &format!("csrf_token={csrf_token}"),
        "csrf token should have been rotated to a new value, got {rotated_csrf}"
    );
}

/// AC03-3相当: ログアウト後は認証必須URLへ再びアクセスできない
/// (formal/Bbs/Invariant.leanの`viewThreadList_requires_auth`と同じ形)。
#[sqlx::test]
async fn after_logout_protected_route_redirects_to_login_again(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let _ = app
        .oneshot(post_logout_request(&cookie_header, &csrf_token))
        .await
        .unwrap();

    // ログアウト前と同じ(失効済みの)Cookieでも保護ルートへは入れない。
    let app2 = bbs::web::build_router(pool.clone());
    let response = app2
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::HOST, HOST)
                .header(header::COOKIE, cookie_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");
}

/// AC03-3の前提(formal/Bbs/Invariant.leanの`logout_requires_auth`): 未ログインでの
/// POST /logoutはログイン画面へリダイレクトされ、sessionsテーブルには一切触れない。
#[sqlx::test]
async fn post_logout_without_login_redirects_to_login_and_touches_no_session(pool: PgPool) {
    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/logout")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, origin_header())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("csrf_token=guessed-token"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");

    let count: (i64,) = sqlx::query_as("select count(*) from sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

/// decision 0007(多重セッション許可): 同じ利用者の別セッションはログアウトの
/// 影響を受けない。formal/Bbs/Invariant.leanの`logout_removes_only_target_session`
/// (対象セッションだけを消し、他のセッションには触れない)の実装側対応。
#[sqlx::test]
async fn logout_does_not_invalidate_other_sessions_of_the_same_user(pool: PgPool) {
    let user_id = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    // 2つ目のセッションを直接作る(同一ユーザーの多重ログインを模する)。
    let other_session = bbs::db::sessions::create(&pool, user_id).await.unwrap();

    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let count_before: (i64,) = sqlx::query_as("select count(*) from sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_before.0, 2);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_logout_request(&cookie_header, &csrf_token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);

    // ログアウトしたセッションだけが消え、他方は残る。
    let remaining = bbs::db::sessions::find_user(&pool, &other_session)
        .await
        .unwrap();
    assert!(
        remaining.is_some(),
        "the other session of the same user must survive logout"
    );
    let count_after: (i64,) = sqlx::query_as("select count(*) from sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_after.0, 1);
}

/// decision 0021: POST /logoutもCSRF二重送信トークン検証の対象(例外なし)。
#[sqlx::test]
async fn post_logout_with_mismatched_csrf_token_is_rejected_and_keeps_the_session(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_logout_request(
            &cookie_header,
            "totally-different-token",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let count: (i64,) = sqlx::query_as("select count(*) from sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1, "rejected logout must not delete the session");
}

/// decision 0021: 同一オリジン検証もPOST /logoutに課される。
#[sqlx::test]
async fn post_logout_with_cross_origin_request_is_rejected_with_403(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let mut request = post_logout_request(&cookie_header, &csrf_token);
    request
        .headers_mut()
        .insert(header::ORIGIN, "http://evil.example".parse().unwrap());
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let count: (i64,) = sqlx::query_as("select count(*) from sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1);
}
