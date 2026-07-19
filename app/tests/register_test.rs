//! F01ユーザー登録の結合テスト(Router全体 = ミドルウェア込みでAction層まで通す)。
//! decision 0021のCSRF機構(同一オリジン検証・二重送信トークン)も、実際のHTTP
//! ヘッダ・Cookieを通して検証する。

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;
use common::{HOST, origin_header, urlencoding_stub};

/// GET /register を叩き、(本文, Set-Cookieのcsrf_token値)を返す。
async fn get_register_page(pool: &PgPool) -> (String, String) {
    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/register")
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

fn post_register_request(
    csrf_token: &str,
    cookie_header: &str,
    unique_id: &str,
    password: &str,
    display_name: &str,
) -> Request<Body> {
    let form = format!(
        "unique_id={}&password={}&display_name={}&csrf_token={}",
        urlencoding_stub(unique_id),
        urlencoding_stub(password),
        urlencoding_stub(display_name),
        urlencoding_stub(csrf_token),
    );
    Request::builder()
        .method("POST")
        .uri("/register")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, origin_header())
        .header(header::COOKIE, cookie_header)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .unwrap()
}

#[sqlx::test]
async fn get_register_renders_form_with_csrf_hidden_input(pool: PgPool) {
    let (html, csrf_token) = get_register_page(&pool).await;
    assert!(html.contains("新規登録"));
    assert!(html.contains(&format!(r#"value="{csrf_token}""#)));
    assert!(html.contains(r#"name="csrf_token""#));
    // 初期表示ではメッセージ領域は空だが、要素自体はDOMに存在する
    // (aria-liveが変化を検知できるようにするため)。
    assert!(html.contains(r#"aria-live="assertive""#));
    assert!(!html.contains("登録できませんでした"));
}

#[sqlx::test]
async fn post_register_blank_unique_id_is_rejected(pool: PgPool) {
    let (_, csrf_token) = get_register_page(&pool).await;
    let cookie = format!("csrf_token={csrf_token}");
    let app = bbs::web::build_router(pool.clone());
    // 全角スペースのみのID。decision 0004の「空」の定義をユニークIDにも適用する。
    let response = app
        .oneshot(post_register_request(
            &csrf_token,
            &cookie,
            "　　",
            "TestPassword123!",
            "テストユーザー06",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("ユニークIDを入力してください"));

    let count: (i64,) = sqlx::query_as("select count(*) from users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

#[sqlx::test]
async fn post_register_with_valid_data_redirects_to_login_and_persists_user(pool: PgPool) {
    let (_, csrf_token) = get_register_page(&pool).await;
    let cookie = format!("csrf_token={csrf_token}");

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_register_request(
            &csrf_token,
            &cookie,
            "testuser_01",
            "TestPassword123!",
            "テストユーザー01",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    // decision 0024: 登録成功をH-12で自然言語観測可能にするため、ログイン画面での
    // 成功表示をクエリパラメータで駆動する。
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        "/login?registered=1"
    );

    // AC01-6: 永続化されている。
    let saved: (String,) = sqlx::query_as("select display_name from users where unique_id = $1")
        .bind("testuser_01")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(saved.0, "テストユーザー01");
}

#[sqlx::test]
async fn post_register_weak_password_rerenders_with_field_errors_and_keeps_input(pool: PgPool) {
    let (_, csrf_token) = get_register_page(&pool).await;
    let cookie = format!("csrf_token={csrf_token}");

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_register_request(
            &csrf_token,
            &cookie,
            "testuser_01",
            "password",
            "テストユーザー01",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    // シナリオ01-1-5: 複数のパスワード違反理由が出る(decision 0006)。
    assert!(html.contains("12文字以上で入力してください"));
    assert!(html.contains("数字を含めてください"));
    assert!(html.contains("記号を含めてください"));
    // 失敗後も入力済みの値(ID)は消えない(ui-ux-guidelines §2)。
    assert!(html.contains(r#"value="testuser_01""#));
    // 共通メッセージエリアに失敗の要約が出る(ui-ux-guidelines §2)。領域はaria-live付き(同 §4)。
    assert!(html.contains(r#"aria-live="assertive""#));
    assert!(html.contains("登録できませんでした。入力内容を確認してください。"));

    // バリデーション失敗時はDBに書き込まれていない。
    let count: (i64,) = sqlx::query_as("select count(*) from users where unique_id = $1")
        .bind("testuser_01")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

#[sqlx::test]
async fn post_register_duplicate_unique_id_rerenders_with_duplicate_error(pool: PgPool) {
    let (_, csrf_token1) = get_register_page(&pool).await;
    let cookie1 = format!("csrf_token={csrf_token1}");
    let app1 = bbs::web::build_router(pool.clone());
    let first = app1
        .oneshot(post_register_request(
            &csrf_token1,
            &cookie1,
            "testuser_01",
            "TestPassword123!",
            "テストユーザー01",
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::SEE_OTHER);

    let (_, csrf_token2) = get_register_page(&pool).await;
    let cookie2 = format!("csrf_token={csrf_token2}");
    let app2 = bbs::web::build_router(pool.clone());
    let second = app2
        .oneshot(post_register_request(
            &csrf_token2,
            &cookie2,
            "testuser_01",
            "AnotherPass123!",
            "べつの表示名",
        ))
        .await
        .unwrap();

    assert_eq!(second.status(), StatusCode::OK);
    let body = axum::body::to_bytes(second.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("このIDは既に使用されています"));
}

#[sqlx::test]
async fn post_register_display_name_too_long_is_rejected(pool: PgPool) {
    let (_, csrf_token) = get_register_page(&pool).await;
    let cookie = format!("csrf_token={csrf_token}");
    let app = bbs::web::build_router(pool.clone());
    let too_long = "あいうえおかきくけこさしすせそた"; // 16コードポイント
    let response = app
        .oneshot(post_register_request(
            &csrf_token,
            &cookie,
            "testuser_02",
            "TestPassword123!",
            too_long,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("表示名は15文字以内で入力してください"));
}

#[sqlx::test]
async fn post_register_without_csrf_cookie_is_rejected_with_403(pool: PgPool) {
    let app = bbs::web::build_router(pool.clone());
    // Cookieヘッダ自体を付けない(csrf_tokenが空 = tokens_matchは常に不一致)。
    let request = Request::builder()
        .method("POST")
        .uri("/register")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, origin_header())
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "unique_id=testuser_03&password=TestPassword123!&display_name=%E3%83%86%E3%82%B9%E3%83%88&csrf_token=guessed-token",
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn post_register_with_mismatched_csrf_token_is_rejected_with_403(pool: PgPool) {
    let (_, csrf_token) = get_register_page(&pool).await;
    let cookie = format!("csrf_token={csrf_token}");
    let app = bbs::web::build_router(pool.clone());
    // フォームのcsrf_tokenをCookieの値と違うものにする。
    let response = app
        .oneshot(post_register_request(
            "totally-different-token",
            &cookie,
            "testuser_04",
            "TestPassword123!",
            "テストユーザー04",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn post_register_with_cross_origin_request_is_rejected_with_403(pool: PgPool) {
    let (_, csrf_token) = get_register_page(&pool).await;
    let cookie = format!("csrf_token={csrf_token}");
    let app = bbs::web::build_router(pool.clone());
    let mut request = post_register_request(
        &csrf_token,
        &cookie,
        "testuser_05",
        "TestPassword123!",
        "テストユーザー05",
    );
    request
        .headers_mut()
        .insert(header::ORIGIN, "http://evil.example".parse().unwrap());
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// GET /login自体の描画(CSRF hidden input・フォーム構造)はF02のスコープ。
// tests/login_test.rsの `get_login_page_renders_form_with_csrf_hidden_input` を参照。
