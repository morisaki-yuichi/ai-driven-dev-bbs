//! F02ログインの結合テスト(Router全体 = ミドルウェア込みでAction層まで通す)。
//! AC02-1〜AC02-3、C-09、decision 0021(CSRF)。formal/Bbs/Invariant.leanの
//! `login_atomic`(認証失敗時にセッションを書き込まない)・`requireAuth_fails_without_session`
//! (未ログインでは保護されたルートに入れない)をオラクルとして、対応する結合テストを置く。

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use bbs::db::password;
use sqlx::PgPool;
use tower::ServiceExt;

const HOST: &str = "example.test";

fn origin_header() -> String {
    format!("http://{HOST}")
}

async fn insert_test_user(pool: &PgPool, unique_id: &str, plain_password: &str) -> i64 {
    let hash = password::hash(plain_password).unwrap();
    sqlx::query_scalar!(
        "insert into users (unique_id, password_hash, display_name) values ($1, $2, $3) returning id",
        unique_id,
        hash,
        "テストユーザー01"
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// GET /login を叩き、(本文, Set-Cookieのcsrf_token値)を返す。register_test.rsの
/// `get_register_page`と同じ形。
async fn get_login_page(pool: &PgPool) -> (String, String) {
    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/login")
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .expect("csrf cookie must be issued on GET")
        .to_str()
        .unwrap()
        .to_string();
    let csrf_token = set_cookie
        .split(';')
        .next()
        .unwrap()
        .strip_prefix("csrf_token=")
        .expect("Set-Cookie should be csrf_token=...")
        .to_string();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    (html, csrf_token)
}

fn urlencoding_stub(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn post_login_request(
    csrf_token: &str,
    cookie_header: &str,
    unique_id: &str,
    password: &str,
) -> Request<Body> {
    let form = format!(
        "unique_id={}&password={}&csrf_token={}",
        urlencoding_stub(unique_id),
        urlencoding_stub(password),
        urlencoding_stub(csrf_token),
    );
    Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, origin_header())
        .header(header::COOKIE, cookie_header)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .unwrap()
}

#[sqlx::test]
async fn get_login_page_renders_form_with_csrf_hidden_input(pool: PgPool) {
    let (html, csrf_token) = get_login_page(&pool).await;
    assert!(html.contains("ログイン"));
    assert!(html.contains(&format!(r#"value="{csrf_token}""#)));
    assert!(html.contains(r#"name="csrf_token""#));
    assert!(html.contains(r#"aria-live="assertive""#));
    // AC02-1: 未ログインでもログイン画面自体は見える(初期表示ではエラーなし)。
    assert!(!html.contains("IDまたはパスワードが正しくありません"));
}

/// AC02-1: 保護されたルート("/"、formal/Bbs/Invariant.leanの
/// `requireAuth_fails_without_session`がモデル上の対応物)は未ログインでは
/// 入れず、ログイン画面へリダイレクトされる。
#[sqlx::test]
async fn accessing_protected_route_without_login_redirects_to_login(pool: PgPool) {
    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");
}

#[sqlx::test]
async fn post_login_with_correct_credentials_redirects_and_sets_session_cookie(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let (_, csrf_token) = get_login_page(&pool).await;
    let cookie = format!("csrf_token={csrf_token}");

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_login_request(
            &csrf_token,
            &cookie,
            "testuser_01",
            "TestPassword123!",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/");

    let set_cookies: Vec<String> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    assert!(
        set_cookies.iter().any(|c| c.starts_with("session_id=")),
        "expected a session_id cookie, got {set_cookies:?}"
    );
    // decision 0021 決定5: ログイン成功時にCSRFトークンをローテーションする。
    let rotated = set_cookies
        .iter()
        .find(|c| c.starts_with("csrf_token="))
        .expect("expected a rotated csrf_token cookie");
    assert!(
        !rotated.starts_with(&format!("csrf_token={csrf_token};")),
        "csrf token should have been rotated to a new value, got {rotated}"
    );

    // AC02-2: セッションが1件永続化されている。
    let count: (i64,) = sqlx::query_as("select count(*) from sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1);
}

#[sqlx::test]
async fn post_login_with_wrong_password_shows_error_and_creates_no_session(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let (_, csrf_token) = get_login_page(&pool).await;
    let cookie = format!("csrf_token={csrf_token}");

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_login_request(
            &csrf_token,
            &cookie,
            "testuser_01",
            "WrongPassword!",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    // シナリオ01-2-2の文言そのもの。
    assert!(html.contains("IDまたはパスワードが正しくありません"));
    // 失敗後も入力済みのユニークIDは消えない(ui-ux-guidelines §2)。
    assert!(html.contains(r#"value="testuser_01""#));

    // login_atomic(formal/Bbs/Invariant.lean): 失敗経路ではセッションを書き込まない。
    let count: (i64,) = sqlx::query_as("select count(*) from sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

#[sqlx::test]
async fn post_login_with_unknown_unique_id_shows_same_error_and_creates_no_session(pool: PgPool) {
    let (_, csrf_token) = get_login_page(&pool).await;
    let cookie = format!("csrf_token={csrf_token}");

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_login_request(
            &csrf_token,
            &cookie,
            "no_such_user",
            "TestPassword123!",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    // AC02-3: 存在しないIDでも、誤ったパスワードと同じ文言(列挙攻撃を避ける)。
    assert!(html.contains("IDまたはパスワードが正しくありません"));

    let count: (i64,) = sqlx::query_as("select count(*) from sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

/// AC02-4/AC02-5相当: ログイン後、発行されたセッションCookieで後続リクエストが
/// 認証済みとして扱われ、再度ログインを求められない(保護されたルートに入れる)。
#[sqlx::test]
async fn login_then_access_protected_route_succeeds_with_session_cookie(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let (_, csrf_token) = get_login_page(&pool).await;
    let cookie = format!("csrf_token={csrf_token}");

    let app = bbs::web::build_router(pool.clone());
    let login_response = app
        .oneshot(post_login_request(
            &csrf_token,
            &cookie,
            "testuser_01",
            "TestPassword123!",
        ))
        .await
        .unwrap();
    assert_eq!(login_response.status(), StatusCode::SEE_OTHER);

    let session_cookie = login_response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .find(|c| c.starts_with("session_id="))
        .expect("expected session_id cookie");
    let session_pair = session_cookie.split(';').next().unwrap().to_string();

    let app2 = bbs::web::build_router(pool.clone());
    let protected_response = app2
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::HOST, HOST)
                .header(header::COOKIE, session_pair)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(protected_response.status(), StatusCode::OK);
}

#[sqlx::test]
async fn post_login_without_csrf_cookie_is_rejected_with_403(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let app = bbs::web::build_router(pool.clone());
    let request = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, origin_header())
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "unique_id=testuser_01&password=TestPassword123!&csrf_token=guessed-token",
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // CSRF拒否経路ではトランザクションを開始しない(decision 0021 決定6)ので
    // セッションも作られない。
    let count: (i64,) = sqlx::query_as("select count(*) from sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

#[sqlx::test]
async fn post_login_with_mismatched_csrf_token_is_rejected_with_403(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let (_, csrf_token) = get_login_page(&pool).await;
    let cookie = format!("csrf_token={csrf_token}");
    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_login_request(
            "totally-different-token",
            &cookie,
            "testuser_01",
            "TestPassword123!",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn post_login_with_cross_origin_request_is_rejected_with_403(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let (_, csrf_token) = get_login_page(&pool).await;
    let cookie = format!("csrf_token={csrf_token}");
    let app = bbs::web::build_router(pool.clone());
    let mut request = post_login_request(&csrf_token, &cookie, "testuser_01", "TestPassword123!");
    request
        .headers_mut()
        .insert(header::ORIGIN, "http://evil.example".parse().unwrap());
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
