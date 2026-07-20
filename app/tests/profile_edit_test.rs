//! F04プロフィール編集の結合テスト(Router全体 = ミドルウェア込みでAction層まで通す)。
//! AC04-1〜AC04-3、C-09(未ログイン不可)、decision 0002(1リクエスト=1トランザクション)、
//! decision 0021(CSRF)。formal/Bbs/Invariant.leanの`updateDisplayName_requires_auth`
//! (未認証では状態を書き換えず失敗する)・`updateDisplayName_atomic`(decision 0002)を
//! オラクルとして、対応する結合テストを置く。
//!
//! AC04-2(過去の投稿の表示名反映)は、実際にスレッド・コメントを作成したユーザーの
//! 表示名を変更し、一覧・詳細の表示が追随することを実データで確認する
//! (`displayName_propagates`の実装側の裏付け)。

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;
use common::{HOST, insert_test_user, origin_header, urlencoding_stub};

/// thread_create_test.rsと同じ形: ユーザーを登録済みの状態から実際に`POST /login`を
/// 通し、(`session_id=...; csrf_token=...`のCookieヘッダ)を返す。
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
    let session_pair = set_cookies
        .iter()
        .find(|c| c.starts_with("session_id="))
        .expect("login should set a session_id cookie")
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let csrf_pair = set_cookies
        .iter()
        .find(|c| c.starts_with("csrf_token="))
        .expect("login should rotate the csrf_token cookie")
        .split(';')
        .next()
        .unwrap()
        .to_string();

    format!("{session_pair}; {csrf_pair}")
}

fn csrf_token_from_cookie_header(cookie_header: &str) -> String {
    cookie_header
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("csrf_token="))
        .expect("cookie header should carry csrf_token")
        .to_string()
}

fn get_profile_edit_request(cookie_header: &str, query: &str) -> Request<Body> {
    let uri = if query.is_empty() {
        "/profile/edit".to_string()
    } else {
        format!("/profile/edit?{query}")
    };
    Request::builder()
        .uri(uri)
        .header(header::HOST, HOST)
        .header(header::COOKIE, cookie_header)
        .body(Body::empty())
        .unwrap()
}

fn post_profile_edit_request(
    cookie_header: &str,
    csrf_token: &str,
    display_name: &str,
) -> Request<Body> {
    let form = format!(
        "display_name={}&csrf_token={}",
        urlencoding_stub(display_name),
        urlencoding_stub(csrf_token),
    );
    Request::builder()
        .method("POST")
        .uri("/profile/edit")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, origin_header())
        .header(header::COOKIE, cookie_header)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .unwrap()
}

async fn get_body_text(response: axum::response::Response) -> String {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(body.to_vec()).unwrap()
}

async fn saved_display_name(pool: &PgPool, unique_id: &str) -> String {
    let row: (String,) = sqlx::query_as("select display_name from users where unique_id = $1")
        .bind(unique_id)
        .fetch_one(pool)
        .await
        .unwrap();
    row.0
}

/// AC04-1の前提: ログイン状態でGET /profile/editを開くと、現在の表示名が
/// 入力欄の初期値として描画される。
#[sqlx::test]
async fn get_profile_edit_renders_form_with_current_display_name_and_csrf(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(get_profile_edit_request(&cookie_header, ""))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    assert!(html.contains("プロフィール編集"));
    assert!(html.contains(r#"action="/profile/edit""#));
    assert!(html.contains(r#"name="csrf_token""#));
    assert!(html.contains(r#"name="display_name""#));
    // insert_test_userの初期表示名(common/mod.rs)。
    assert!(html.contains(r#"value="テストユーザー01""#));
    // 初期表示ではメッセージ領域は空。
    assert!(!html.contains("表示名を変更しました"));
    assert!(!html.contains("変更できませんでした"));
}

/// issue 04: 「その他の情報(ユニークID、パスワードなど)の変更機能は不要」。
/// フォームに表示名以外の入力欄が無いことを確認する。
#[sqlx::test]
async fn profile_edit_form_has_no_unique_id_or_password_fields(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(get_profile_edit_request(&cookie_header, ""))
        .await
        .unwrap();
    let html = get_body_text(response).await;
    assert!(!html.contains(r#"name="unique_id""#));
    assert!(!html.contains(r#"name="password""#));
}

/// C-09相当: 未ログインでGET /profile/editへアクセスするとログイン画面へ
/// リダイレクトされる(formal/Bbs/Invariant.leanの`updateDisplayName_requires_auth`と
/// 同じガード)。
#[sqlx::test]
async fn get_profile_edit_without_login_redirects_to_login(pool: PgPool) {
    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/profile/edit")
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");
}

/// 未ログインでのPOST /profile/editもログイン画面へリダイレクトされ、
/// 表示名は書き換わらない(`updateDisplayName_requires_auth`: 状態を書き換えず失敗する)。
#[sqlx::test]
async fn post_profile_edit_without_login_redirects_and_does_not_update(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/profile/edit")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, origin_header())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(
                    "display_name=新しい名前&csrf_token=guessed-token",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");

    assert_eq!(
        saved_display_name(&pool, "testuser_01").await,
        "テストユーザー01"
    );
}

/// AC04-1: ログイン中に新しい表示名を入力して保存すると、P06自身へリダイレクトされ
/// (ui_design.md画面遷移図: P06 -- 保存完了 --> P06、decision 0024と同じPRGパターン)、
/// DBに永続化される。リダイレクト先(?updated=1)では成功メッセージが表示され、
/// 入力欄の初期値も新しい表示名に更新されている(ヘッダーも含め、セッションの
/// 再読み込みによって自動的に反映される)。
#[sqlx::test]
async fn post_profile_edit_with_valid_name_redirects_persists_and_shows_success(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_profile_edit_request(
            &cookie_header,
            &csrf_token,
            "新しい表示名",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        "/profile/edit?updated=1"
    );

    assert_eq!(
        saved_display_name(&pool, "testuser_01").await,
        "新しい表示名"
    );

    let app2 = bbs::web::build_router(pool.clone());
    let follow_up = app2
        .oneshot(get_profile_edit_request(&cookie_header, "updated=1"))
        .await
        .unwrap();
    assert_eq!(follow_up.status(), StatusCode::OK);
    let html = get_body_text(follow_up).await;
    assert!(html.contains("表示名を変更しました"));
    assert!(html.contains(r#"value="新しい表示名""#));
    // ヘッダーも新しい表示名で描画される(AuthenticatedUserをセッションから
    // 再読み込みするため、追加の配線なしに反映される)。
    assert!(html.contains("新しい表示名 さん"));
    assert!(html.contains(r#"aria-live="polite""#));
}

/// レビュー指摘: `db/users.rs`のUPDATEを`where id = $2 or true`(全ユーザーを
/// 書き換える)に壊す変異テストで、結合スイート9件がすべて素通りしDB単体テスト
/// 1本だけが検出した ―― 結合層に「ユーザーAが改名してもユーザーBの表示名が
/// 変わらない」ことを検証するケースが無かったため。Router全体(結合層)を通した
/// 状態で、他ユーザーの行が巻き添えにならないことを確認する。
#[sqlx::test]
async fn post_profile_edit_does_not_change_other_users_display_name(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    insert_test_user(&pool, "testuser_02", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_profile_edit_request(
            &cookie_header,
            &csrf_token,
            "改名後のユーザーA",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);

    assert_eq!(
        saved_display_name(&pool, "testuser_01").await,
        "改名後のユーザーA"
    );
    // ユーザーBの行は書き換えられていない(insert_test_userの初期値のまま)。
    // `where id = $2`を`where id = $2 or true`のように壊す変異があれば、
    // ここが「改名後のユーザーA」に化けて検出される。
    assert_eq!(
        saved_display_name(&pool, "testuser_02").await,
        "テストユーザー01"
    );
}

/// AC04-3: 15文字を超える表示名を入力すると、バリデーションエラーが表示され
/// 保存されない(旧い表示名のまま)。
#[sqlx::test]
async fn post_profile_edit_16_chars_name_is_rejected_and_keeps_old_value(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let too_long = "あいうえおかきくけこさしすせそた"; // 16コードポイント
    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_profile_edit_request(
            &cookie_header,
            &csrf_token,
            too_long,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    assert!(html.contains("表示名は15文字以内で入力してください"));
    // 失敗後も入力済みの値は消えない(ui-ux-guidelines §2)。
    assert!(html.contains(&format!(r#"value="{too_long}""#)));
    assert!(html.contains(r#"aria-live="assertive""#));
    assert!(html.contains("変更できませんでした。入力内容を確認してください。"));

    assert_eq!(
        saved_display_name(&pool, "testuser_01").await,
        "テストユーザー01"
    );
}

/// AC04-3相当: 全角スペースのみ(decision 0004の「空」の定義)の表示名も拒否される。
#[sqlx::test]
async fn post_profile_edit_blank_name_is_rejected_and_keeps_old_value(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_profile_edit_request(
            &cookie_header,
            &csrf_token,
            "　　",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    assert!(html.contains("表示名を入力してください"));

    assert_eq!(
        saved_display_name(&pool, "testuser_01").await,
        "テストユーザー01"
    );
}

/// decision 0021: POST /profile/editもCSRF二重送信トークン検証の対象(例外なし)。
#[sqlx::test]
async fn post_profile_edit_with_mismatched_csrf_token_is_rejected_with_403(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_profile_edit_request(
            &cookie_header,
            "totally-different-token",
            "新しい表示名",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    assert_eq!(
        saved_display_name(&pool, "testuser_01").await,
        "テストユーザー01"
    );
}

/// AC04-2: 表示名を変更すると、変更前に投稿していたスレッド・コメントの
/// 表示名も新しい名前に反映される。decision 0015のJOIN方式(users.display_nameを
/// 都度JOINする)の実データでの確認 ―― formal/Bbs/Invariant.leanの
/// `displayName_propagates`が保証する性質の実装側の裏付け。
#[sqlx::test]
async fn ac04_2_updated_display_name_propagates_to_existing_thread_and_comment(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    // 変更前にスレッドとコメントを作成しておく。
    let app = bbs::web::build_router(pool.clone());
    let create_thread_form = format!(
        "title={}&body={}&csrf_token={}",
        urlencoding_stub("AC04-2確認用スレッド"),
        urlencoding_stub("本文です"),
        urlencoding_stub(&csrf_token),
    );
    let create_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/threads/new")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, origin_header())
                .header(header::COOKIE, cookie_header.clone())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(create_thread_form))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::SEE_OTHER);

    let thread_id: i64 = sqlx::query_scalar("select id from threads where title = $1")
        .bind("AC04-2確認用スレッド")
        .fetch_one(&pool)
        .await
        .unwrap();

    let app2 = bbs::web::build_router(pool.clone());
    let create_comment_form = format!(
        "body={}&csrf_token={}",
        urlencoding_stub("コメント本文です"),
        urlencoding_stub(&csrf_token),
    );
    let comment_response = app2
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/threads/{thread_id}/comments"))
                .header(header::HOST, HOST)
                .header(header::ORIGIN, origin_header())
                .header(header::COOKIE, cookie_header.clone())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(create_comment_form))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(comment_response.status(), StatusCode::SEE_OTHER);

    // 表示名を変更する。
    let app3 = bbs::web::build_router(pool.clone());
    let update_response = app3
        .oneshot(post_profile_edit_request(
            &cookie_header,
            &csrf_token,
            "改名後の表示名",
        ))
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::SEE_OTHER);

    // スレッド一覧: 作成者名が新しい表示名に変わっている。
    let app4 = bbs::web::build_router(pool.clone());
    let list_response = app4
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::HOST, HOST)
                .header(header::COOKIE, cookie_header.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let list_html = get_body_text(list_response).await;
    assert!(list_html.contains("改名後の表示名"));
    assert!(!list_html.contains("テストユーザー01"));

    // スレッド詳細: スレッド作成者・コメント作成者の両方が新しい表示名。
    let app5 = bbs::web::build_router(pool.clone());
    let detail_response = app5
        .oneshot(
            Request::builder()
                .uri(format!("/threads/{thread_id}"))
                .header(header::HOST, HOST)
                .header(header::COOKIE, cookie_header.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let detail_html = get_body_text(detail_response).await;
    assert_eq!(
        detail_html.matches("改名後の表示名").count(),
        // ヘッダー(現在ログイン中の利用者名) + スレッド作成者 + コメント作成者。
        3,
        "ヘッダー・スレッド作成者・コメント作成者の3箇所すべてが新しい表示名であるはず: {detail_html}"
    );
    assert!(!detail_html.contains("テストユーザー01"));
}
