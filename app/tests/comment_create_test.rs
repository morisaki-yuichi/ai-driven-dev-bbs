//! F07コメント作成の結合テスト(Router全体 = ミドルウェア込みでAction層まで通す)。
//! AC07-1〜AC07-2、C-09(未ログイン不可)、C-05(作成後は編集不可)、
//! decision 0002(1リクエスト=1トランザクション)、decision 0021(CSRF)。
//! formal/Bbs/Invariant.leanの`createComment_atomic`(decision 0002)・
//! `createComment_does_not_modify_existing_comments`(C-05/AC07-4)をオラクルとして、
//! 対応する結合テストを置く。`thread_create_test.rs`と同じ形。

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;
use common::{HOST, insert_test_user, origin_header, urlencoding_stub};

/// ユーザーを登録済みの状態から実際に`POST /login`を通し、
/// (`session_id=...; csrf_token=...`のCookieヘッダ)を返す。thread_create_test.rsと同じ形。
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

fn post_create_comment_request(
    thread_id: i64,
    cookie_header: &str,
    csrf_token: &str,
    body: &str,
) -> Request<Body> {
    let form = format!(
        "body={}&csrf_token={}",
        urlencoding_stub(body),
        urlencoding_stub(csrf_token),
    );
    Request::builder()
        .method("POST")
        .uri(format!("/threads/{thread_id}/comments"))
        .header(header::HOST, HOST)
        .header(header::ORIGIN, origin_header())
        .header(header::COOKIE, cookie_header)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .unwrap()
}

async fn get_detail(
    pool: &PgPool,
    cookie_header: &str,
    thread_id: i64,
) -> axum::response::Response {
    let app = bbs::web::build_router(pool.clone());
    app.oneshot(
        Request::builder()
            .uri(format!("/threads/{thread_id}"))
            .header(header::HOST, HOST)
            .header(header::COOKIE, cookie_header)
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn get_body_text(response: axum::response::Response) -> String {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(body.to_vec()).unwrap()
}

/// AC07-1: スレッド詳細画面から、ログイン状態で本文を入力し、コメントを投稿できること。
#[sqlx::test]
async fn post_comment_with_valid_body_redirects_to_detail_and_persists_comment(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_create_comment_request(
            tid,
            &cookie_header,
            &csrf_token,
            "コメント本文です",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        &format!("/threads/{tid}")
    );

    let saved: (String,) = sqlx::query_as("select body from comments where thread_id = $1")
        .bind(tid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(saved.0, "コメント本文です");
}

/// 受け入れ基準「投稿後、即座に詳細表示のコメント一覧に反映されること」(D06の解釈:
/// POST後のP04再読み込みで満たす)。
#[sqlx::test]
async fn posted_comment_appears_on_thread_detail_immediately(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let create_response = app
        .oneshot(post_create_comment_request(
            tid,
            &cookie_header,
            &csrf_token,
            "反映確認用コメント",
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::SEE_OTHER);

    let detail_response = get_detail(&pool, &cookie_header, tid).await;
    assert_eq!(detail_response.status(), StatusCode::OK);
    let html = get_body_text(detail_response).await;
    assert!(html.contains("反映確認用コメント"));
    assert!(html.contains("テストユーザー01"));
}

/// AC07-2: 本文が空の状態で投稿しようとした場合、エラーが表示されること。
#[sqlx::test]
async fn post_comment_blank_body_is_rejected_and_creates_no_comment(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    // 全角スペースのみの本文。decision 0004の「空」の定義を適用する。
    let response = app
        .oneshot(post_create_comment_request(
            tid,
            &cookie_header,
            &csrf_token,
            "　　",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    assert!(html.contains("本文を入力してください"));
    // 共通メッセージエリアに失敗の要約が出る(ui-ux-guidelines §2)。
    assert!(html.contains(r#"aria-live="assertive""#));
    assert!(html.contains("コメントを投稿できませんでした。入力内容を確認してください。"));
    // スレッド本体は引き続き表示される(ページ全体がエラーにならない)。
    assert!(html.contains("スレッド"));

    let count: (i64,) = sqlx::query_as("select count(*) from comments where thread_id = $1")
        .bind(tid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

/// 失敗後も入力済みの本文は消えない(ui-ux-guidelines §2)。
#[sqlx::test]
async fn post_comment_failure_preserves_entered_body_when_meaningfully_nonempty(pool: PgPool) {
    // 空チェック以外で失敗させる直接の手段がまだ無いため(AC07-2のみが検査対象)、
    // ここではCSRF不一致時の挙動(値保持は不要、403で完結)ではなく、
    // 空本文エラー時に「元のフォームがそのまま再描画される」こと自体を検証する
    // (再描画されたtextareaが存在し、フィールドエラーIDに紐づいていること)。
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_create_comment_request(
            tid,
            &cookie_header,
            &csrf_token,
            "",
        ))
        .await
        .unwrap();
    let html = get_body_text(response).await;
    assert!(html.contains(r#"aria-describedby="comment_body_error""#));
    assert!(html.contains(r#"id="comment_body_error""#));
}

/// C-09: 未ログインでのPOSTはログイン画面へリダイレクトされ、コメントは作られない。
#[sqlx::test]
async fn post_comment_without_login_redirects_and_creates_no_comment(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/threads/{tid}/comments"))
                .header(header::HOST, HOST)
                .header(header::ORIGIN, origin_header())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("body=b&csrf_token=guessed-token"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");

    let count: (i64,) = sqlx::query_as("select count(*) from comments")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

/// C-10: 存在しないスレッドへのコメント投稿は404になり、コメントは作られない。
#[sqlx::test]
async fn post_comment_to_nonexistent_thread_returns_404(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_create_comment_request(
            999_999,
            &cookie_header,
            &csrf_token,
            "本文",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let count: (i64,) = sqlx::query_as("select count(*) from comments")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

/// decision 0021: POST /threads/{id}/commentsもCSRF二重送信トークン検証の対象(例外なし)。
#[sqlx::test]
async fn post_comment_with_mismatched_csrf_token_is_rejected_with_403(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_create_comment_request(
            tid,
            &cookie_header,
            "totally-different-token",
            "本文",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let count: (i64,) = sqlx::query_as("select count(*) from comments where thread_id = $1")
        .bind(tid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

/// C-05/AC07-4: 投稿したコメントに、作成後の編集を行うUIが存在しない
/// (formal/Bbs/Op.leanにコメント更新操作が無いことに対応)。
#[sqlx::test]
async fn thread_detail_has_no_edit_ui_for_posted_comment(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let title = "編集できないことの確認用コメント";
    let create_response = app
        .oneshot(post_create_comment_request(
            tid,
            &cookie_header,
            &csrf_token,
            title,
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::SEE_OTHER);

    let detail_response = get_detail(&pool, &cookie_header, tid).await;
    let html = get_body_text(detail_response).await;

    // コメント編集用のエンドポイント自体が存在しない(ルータに生えていない)。
    for (method, uri) in [
        ("GET", format!("/threads/{tid}/comments/1/edit")),
        ("POST", format!("/threads/{tid}/comments/1/edit")),
        ("POST", format!("/threads/{tid}/comments/1")),
    ] {
        let app = bbs::web::build_router(pool.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri.clone())
                    .header(header::HOST, HOST)
                    .header(header::ORIGIN, origin_header())
                    .header(header::COOKIE, cookie_header.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(
            response.status(),
            StatusCode::OK,
            "{method} {uri} は存在してはならない"
        );
    }

    // コメント一覧領域(ヘッダーのログアウトフォーム・投稿フォームは範囲外)には、
    // 状態変更の手段(decision 0021により全てPOST)が無い。
    let comments_section_start = html
        .find(r#"aria-label="コメント一覧""#)
        .expect("comments section marker not found");
    // 区切りは投稿フォーム側のaria-label属性で取る。見出しの本文リテラル
    // ("コメントを投稿")だと、同じ文字列を含むコメントが投稿された場合に
    // 切り出し位置がずれる。属性値はAskamaのHTMLエスケープを経ないと出現しない
    // (コメント本文由来なら`"`は`&quot;`にエスケープされるため、この文字列と
    // 衝突しない)。
    let comments_section_end = html
        .find(r#"aria-label="コメント投稿""#)
        .unwrap_or(html.len());
    let comments_section = &html[comments_section_start..comments_section_end];
    for control in ["<form", "<button", "<input"] {
        assert!(
            !comments_section.contains(control),
            "コメント一覧に状態変更の手段 {control} があってはならない"
        );
    }
}
