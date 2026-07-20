//! F11検索(P03、issues/11_search_function.md)の結合テスト。
//!
//! シナリオ04-1(`docs/evaluation/scenarios/04_search_and_sort.md`)の手順を
//! そのまま追う: 本文にキーワードを含むスレッドA、コメントにキーワードを含む
//! スレッドBを用意し、検索結果に両方が表示されること・スレッドBの詳細へ遷移した際に
//! ヒットしたコメントへスクロールできる(フラグメント識別子が付く)ことを検証する。
//!
//! decision 0011(大文字小文字・全角半角を区別、空クエリは全件表示)・
//! decision 0012(タイトル対象外、削除済みコメント除外)・decision 0032
//! (LIKEワイルドカードのエスケープ)の受け入れ確認も、DB層(`db::threads`)の
//! 単体テストとは別に、HTTP経路まで通しで確認する。

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;
use common::{HOST, insert_test_user, urlencoding_stub};

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

    let form =
        format!("unique_id={unique_id}&password={plain_password}&csrf_token={initial_csrf_token}");
    let app = bbs::web::build_router(pool.clone());
    let login_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, format!("http://{HOST}"))
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
    set_cookies
        .iter()
        .find(|c| c.starts_with("session_id="))
        .expect("login should set a session_id cookie")
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

async fn get_body_text(response: axum::response::Response) -> String {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(body.to_vec()).unwrap()
}

async fn get(pool: &PgPool, cookie_header: &str, path: &str) -> axum::response::Response {
    let app = bbs::web::build_router(pool.clone());
    app.oneshot(
        Request::builder()
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::COOKIE, cookie_header)
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
}

/// AC11-1/C-09: 未ログインで検索窓付きの一覧(`/?q=...`)へアクセスするとログイン画面へ
/// リダイレクトされる(通常の一覧と同じ`require_auth`配下)。
#[sqlx::test]
async fn search_without_login_redirects_to_login(pool: PgPool) {
    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/?q=Rust")
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");
}

/// シナリオ04-1(手順1〜3): 本文ヒット(スレッドA)・コメントヒット(スレッドB)の
/// 両方が検索結果一覧に表示される。
#[sqlx::test]
async fn search_finds_threads_by_body_and_comment(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    bbs::db::threads::insert(&pool, uid, "スレッドA", "プログラミング言語Rustの特徴")
        .await
        .unwrap();
    let tid_b = bbs::db::threads::insert(&pool, uid, "スレッドB", "関係ない本文")
        .await
        .unwrap();
    bbs::db::comments::insert(&pool, tid_b, uid, "メモリ安全性が高いのがRustの魅力です")
        .await
        .unwrap();

    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let response = get(&pool, &cookie_header, "/?q=Rust").await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;

    assert!(html.contains("スレッドA"), "本文ヒットのスレッドAが無い");
    assert!(
        html.contains("スレッドB"),
        "コメントヒットのスレッドBが無い"
    );
    // 検索窓に入力値が保持されている(ui-ux-guidelines §1 初期表示/状態保持)。
    assert!(
        html.contains(r#"value="Rust""#),
        "検索窓に入力値が残っていない"
    );
    assert!(
        html.contains("「Rust」の検索結果"),
        "検索結果である旨の表示が無い"
    );
}

/// シナリオ04-1(手順4〜5)/AC11-3/D19: スレッドBの検索結果リンクには、
/// ヒットしたコメントへスクロールするためのフラグメント識別子が付き、
/// 詳細画面の該当コメントには同じidのアンカー要素が実在する。
#[sqlx::test]
async fn search_result_links_to_the_hit_comment_anchor(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid_b = bbs::db::threads::insert(&pool, uid, "スレッドB", "関係ない本文")
        .await
        .unwrap();
    let cid = bbs::db::comments::insert(&pool, tid_b, uid, "メモリ安全性が高いのがRustの魅力です")
        .await
        .unwrap();

    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let response = get(&pool, &cookie_header, "/?q=Rust").await;
    let html = get_body_text(response).await;
    let expected_href = format!(r#"href="/threads/{tid_b}#comment-{cid}""#);
    assert!(
        html.contains(&expected_href),
        "スレッドBへのリンクに#comment-{cid}フラグメントが無い: {html}"
    );

    let detail = get(&pool, &cookie_header, &format!("/threads/{tid_b}")).await;
    let detail_html = get_body_text(detail).await;
    let expected_anchor = format!(r#"id="comment-{cid}""#);
    assert!(
        detail_html.contains(&expected_anchor),
        "詳細画面にスクロール先のアンカー要素が無い"
    );
}

/// 本文ヒットのスレッドAは、コメントを検索対象にする必要が無いので
/// フラグメント無しの通常のリンクになる(本文は詳細画面の最上部にあるため)。
#[sqlx::test]
async fn search_result_body_hit_has_no_fragment(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid_a = bbs::db::threads::insert(&pool, uid, "スレッドA", "プログラミング言語Rustの特徴")
        .await
        .unwrap();

    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let response = get(&pool, &cookie_header, "/?q=Rust").await;
    let html = get_body_text(response).await;
    assert!(html.contains(&format!(r#"href="/threads/{tid_a}""#)));
    assert!(!html.contains(&format!(r#"href="/threads/{tid_a}#"#)));
}

/// decision 0011: 大文字小文字を区別する(HTTP経路でも)。
#[sqlx::test]
async fn search_is_case_sensitive_over_http(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    bbs::db::threads::insert(&pool, uid, "スレッドA", "プログラミング言語Rustの特徴")
        .await
        .unwrap();

    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let response = get(&pool, &cookie_header, "/?q=rust").await;
    let html = get_body_text(response).await;
    assert!(!html.contains("スレッドA"));
    assert!(
        html.contains("「rust」に一致するスレッドがありません"),
        "検索専用の空状態文言が出ていない"
    );
}

/// AC11-4/decision 0012: 削除済みコメントの元本文では検索してもヒットしない
/// (固定文言に差し替わる前のオリジナルの本文で検索した場合)。
#[sqlx::test]
async fn search_does_not_find_a_deleted_comments_original_body_over_http(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cid = bbs::db::comments::insert(&pool, tid, uid, "テストコメント1")
        .await
        .unwrap();
    bbs::db::comments::delete(&pool, cid).await.unwrap();

    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let response = get(&pool, &cookie_header, "/?q=テストコメント1").await;
    let html = get_body_text(response).await;
    assert!(
        !html.contains("class=\"thread-card\""),
        "削除済みコメントの元本文はヒットしないはず"
    );
}

/// decision 0011: 空クエリは全件表示(検索前の一覧と同じ挙動)。
#[sqlx::test]
async fn search_with_empty_query_shows_all_threads(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    bbs::db::threads::insert(&pool, uid, "スレッドA", "本文A")
        .await
        .unwrap();
    bbs::db::threads::insert(&pool, uid, "スレッドB", "本文B")
        .await
        .unwrap();

    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let response = get(&pool, &cookie_header, "/?q=").await;
    let html = get_body_text(response).await;
    assert!(html.contains("スレッドA"));
    assert!(html.contains("スレッドB"));
}

/// decision 0032の回帰(HTTP経路): `%`はワイルドカードとしてではなくリテラルに扱われる。
#[sqlx::test]
async fn search_escapes_percent_as_literal_over_http(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    bbs::db::threads::insert(&pool, uid, "セール", "本日は50%引きです")
        .await
        .unwrap();
    bbs::db::threads::insert(&pool, uid, "無関係", "500円のセールです")
        .await
        .unwrap();

    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let query = urlencoding_stub("50%");
    let response = get(&pool, &cookie_header, &format!("/?q={query}")).await;
    let html = get_body_text(response).await;
    assert!(html.contains("セール"));
    assert!(!html.contains("無関係"));
}

/// C-13: 検索中のページ送りリンクは`q`を保持する(URLエンコード済み)。
#[sqlx::test]
async fn pagination_links_preserve_the_search_query(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    for i in 1..=11 {
        // decision 0012: タイトルは検索対象外なので、本文に`Rust`を入れる。
        bbs::db::threads::insert(&pool, uid, &format!("スレッド{i:02}"), "Rustの話題です")
            .await
            .unwrap();
    }

    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let response = get(&pool, &cookie_header, "/?q=Rust").await;
    let html = get_body_text(response).await;
    assert!(
        html.contains(r#"href="/?q=Rust&sort=created_desc&page=2""#),
        "検索中のページ送りリンクに`q=Rust`が保持されていない: {html}"
    );
}
