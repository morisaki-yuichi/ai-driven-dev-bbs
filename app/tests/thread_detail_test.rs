//! F10スレッド詳細表示(P04、issues/10)の結合テスト。
//!
//! **範囲は表示のみ**(ユーザー承認済みのスコープ)。issue 10 のACのうち
//! 「自分のスレッドに削除ボタン」「自分のコメントに削除ボタン」はF06・F08の範囲で
//! ありここでは扱わない。formal/Bbs/Invariant.leanの`deleted_comment_renders_fixed_text`
//! (C-01/AC08-2)・`deleted_comment_keeps_metadata`(AC10-3)をオラクルとする。
//!
//! F07(コメント作成)が未実装のため、コメントは`comments`テーブルへ直接INSERTする
//! (`db/threads.rs`・`tests/thread_list_test.rs`と同じ扱い)。
//!
//! スレッド削除は物理削除(decision 0014)なので、「存在しない」「削除済み」は
//! DB上区別が無い ―― F06未実装の現段階では、存在しないIDへのアクセスが
//! 両方のACを兼ねて検証する。

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;
use sqlx::types::time::OffsetDateTime;
use tower::ServiceExt;

mod common;
use common::{HOST, insert_test_user};

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

async fn get_detail(
    pool: &PgPool,
    cookie_header: Option<&str>,
    id: &str,
) -> axum::response::Response {
    let app = bbs::web::build_router(pool.clone());
    let mut builder = Request::builder()
        .uri(format!("/threads/{id}"))
        .header(header::HOST, HOST);
    if let Some(cookie_header) = cookie_header {
        builder = builder.header(header::COOKIE, cookie_header);
    }
    app.oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// 2099-01-01T00:00:00Z。実行環境の実時計がいつであっても確実に未来。
fn far_future() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(4_070_908_800).unwrap()
}

async fn insert_comment(
    pool: &PgPool,
    thread_id: i64,
    author_id: i64,
    body: &str,
    created_at: OffsetDateTime,
    deleted: bool,
) {
    if deleted {
        sqlx::query!(
            "insert into comments (thread_id, author_id, body, created_at, deleted_at) \
             values ($1, $2, $3, $4, $4)",
            thread_id,
            author_id,
            body,
            created_at,
        )
        .execute(pool)
        .await
        .unwrap();
    } else {
        sqlx::query!(
            "insert into comments (thread_id, author_id, body, created_at) values ($1, $2, $3, $4)",
            thread_id,
            author_id,
            body,
            created_at,
        )
        .execute(pool)
        .await
        .unwrap();
    }
}

/// AC10-2: スレッド一覧から選択すると、全本文・作成者・作成日時が表示される。
#[sqlx::test]
async fn thread_detail_shows_title_body_author_and_created_at(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "詳細確認用スレッド", "本文の中身です")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_detail(&pool, Some(&cookie_header), &tid.to_string()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    assert!(html.contains("詳細確認用スレッド"));
    assert!(html.contains("本文の中身です"));
    assert!(html.contains("テストユーザー01 さん"));
}

/// F07未実装のため、コメントが無いスレッドはその旨が分かる表示になる
/// (ui-ux-guidelines §1: 空データを空白画面にしない)。
#[sqlx::test]
async fn thread_detail_shows_empty_state_without_comments(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_detail(&pool, Some(&cookie_header), &tid.to_string()).await;
    let html = get_body_text(response).await;
    assert!(html.contains("コメントはまだありません"));
}

/// 全コメントが作成日時の昇順(会話の文脈順)で表示される。
#[sqlx::test]
async fn thread_detail_shows_comments_in_chronological_order(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let t0 = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
    let t1 = OffsetDateTime::from_unix_timestamp(1_700_000_100).unwrap();
    insert_comment(&pool, tid, uid, "2番目のコメント", t1, false).await;
    insert_comment(&pool, tid, uid, "1番目のコメント", t0, false).await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_detail(&pool, Some(&cookie_header), &tid.to_string()).await;
    let html = get_body_text(response).await;
    let pos1 = html.find("1番目のコメント").expect("1番目のコメントが無い");
    let pos2 = html.find("2番目のコメント").expect("2番目のコメントが無い");
    assert!(pos1 < pos2, "作成日時の昇順で並んでいない");
}

/// C-01/AC08-2/AC10-3: 削除済みコメントは固定文言に差し替わるが、作成者・
/// 作成日時は維持される。formal `deleted_comment_renders_fixed_text` /
/// `deleted_comment_keeps_metadata` をオラクルとする。
#[sqlx::test]
async fn thread_detail_deleted_comment_shows_fixed_text_but_keeps_metadata(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    insert_comment(&pool, tid, uid, "削除される前の本文", far_future(), true).await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_detail(&pool, Some(&cookie_header), &tid.to_string()).await;
    let html = get_body_text(response).await;
    // C-01: 厳密一致の固定文言(全角山括弧)。
    assert!(html.contains("＜このコメントは削除されました＞"));
    // 元本文は表示に出ない。
    assert!(!html.contains("削除される前の本文"));
    // AC10-3: 作成者・日時(未来日付、JSTでも同じ日付になる)は維持される。
    assert!(html.contains("テストユーザー01 さん"));
    assert!(html.contains("2099-01-01"));
}

/// C-09: 未ログインでアクセスするとログイン画面へリダイレクトされる。
#[sqlx::test]
async fn thread_detail_without_login_redirects_to_login(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();

    let response = get_detail(&pool, None, &tid.to_string()).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(location, "/login");
}

/// C-10: 存在しないスレッドIDへの直接アクセスは404相当になる。
/// decision 0014によりスレッド削除は物理削除なので、このケースは
/// 「存在しない」「削除済み」の両方を兼ねる(F06未実装のためDB上区別できない)。
#[sqlx::test]
async fn thread_detail_nonexistent_id_returns_404(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_detail(&pool, Some(&cookie_header), "999999").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let html = get_body_text(response).await;
    assert!(html.contains("404"));
    // ui-ux-guidelines §7: エラー画面には「トップページへ戻る」導線を必ず置く。
    assert!(html.contains(r#"href="/""#));
}

/// 数字以外のIDも同じくC-10の「存在しない」として404にする。
#[sqlx::test]
async fn thread_detail_non_numeric_id_returns_404(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_detail(&pool, Some(&cookie_header), "abc").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// 回帰(このセッションで修正): ログイン中に404を踏んでも、未ログイン用ヘッダー
/// (ログイン/新規登録リンク)ではなく、ログイン中ユーザーのヘッダー(表示名・
/// ログアウトボタン)が出ること。`web/error.rs`の`not_found()`が常に
/// `current_user: None`で描画していたのが原因で、`AppError::with_current_user`
/// (新設)で修正した。
#[sqlx::test]
async fn thread_detail_404_while_logged_in_shows_authenticated_header(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_detail(&pool, Some(&cookie_header), "999999").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let html = get_body_text(response).await;
    assert!(
        html.contains("テストユーザー01 さん"),
        "ログイン中ユーザーの表示名がヘッダーに出ていない: {html}"
    );
    assert!(
        html.contains(r#"<button type="submit">ログアウト</button>"#),
        "ログイン中はログアウトボタンが出るはず: {html}"
    );
    assert!(
        !html.contains(r#"<a href="/login">ログイン</a>"#),
        "ログイン中なのに未ログイン用のログインリンクが出ている: {html}"
    );
}

async fn get_unknown_url(pool: &PgPool, cookie_header: Option<&str>) -> axum::response::Response {
    let app = bbs::web::build_router(pool.clone());
    let mut builder = Request::builder()
        .uri("/nosuchpage")
        .header(header::HOST, HOST);
    if let Some(cookie_header) = cookie_header {
        builder = builder.header(header::COOKIE, cookie_header);
    }
    app.oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// 未知URL(`web/mod.rs`の`fallback`)の404でも、ログイン中なら認証済みヘッダーが出る。
/// `fallback`は`require_auth`の外にあるため、ハンドラ側で認証情報を付ける方式では
/// 構造的に取りこぼしていた経路。`middleware::reflect_auth_on_error_page`の回帰テスト。
#[sqlx::test]
async fn unknown_url_404_while_logged_in_shows_authenticated_header(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_unknown_url(&pool, Some(&cookie_header)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let html = get_body_text(response).await;
    assert!(
        html.contains("テストユーザー01 さん"),
        "ログイン中ユーザーの表示名がヘッダーに出ていない: {html}"
    );
    assert!(
        html.contains(r#"<button type="submit">ログアウト</button>"#),
        "ログイン中はログアウトボタンが出るはず: {html}"
    );
    assert!(
        !html.contains(r#"<a href="/login">ログイン</a>"#),
        "ログイン中なのに未ログイン用のログインリンクが出ている: {html}"
    );
}

/// 回帰(F10最終レビューで修正): 未知URLをログイン中に踏んだ404応答は、本文が
/// 認証済みヘッダーで描き直される以上、`Cache-Control: no-store`を持つこと
/// (C-11/AC03-2、decision 0008)。改修前は`reflect_auth_on_error_page`が本文を
/// 差し替えるだけでこのヘッダーを付けておらず、`fallback`(未知URL)は
/// `require_auth`の外にあるためどこからも付与されていなかった。
#[sqlx::test]
async fn unknown_url_404_while_logged_in_has_no_store(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_unknown_url(&pool, Some(&cookie_header)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL),
        Some(&header::HeaderValue::from_static("no-store")),
        "ログイン中に未知URLを踏んだ404にno-storeが付いていない"
    );
    // 二重付与されていないことも確認する(`insert`であって`append`でないことの保証)。
    assert_eq!(
        response
            .headers()
            .get_all(header::CACHE_CONTROL)
            .iter()
            .count(),
        1
    );
}

/// 未知URLは未ログインでも404を返す(`fallback`は認証必須にできない)。
/// このとき認証情報は解決できないので、未ログイン用ヘッダーのままであること。
#[sqlx::test]
async fn unknown_url_404_without_login_shows_guest_header(pool: PgPool) {
    let response = get_unknown_url(&pool, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let html = get_body_text(response).await;
    assert!(
        html.contains(r#"<a href="/login">ログイン</a>"#),
        "未ログインならログインリンクが出るはず: {html}"
    );
    assert!(
        !html.contains(r#"<button type="submit">ログアウト</button>"#),
        "未ログインなのにログアウトボタンが出ている: {html}"
    );
}

/// 持ち越し修正の回帰: 未ログインで未知URLを踏んだ404にも`no-store`が付くこと
/// (C-11)。`middleware::reflect_auth_on_error_page`は以前、セッション未解決
/// (`let (Some(session_id), Some(csrf_token)) = ... else { return response }`)の
/// 早期returnが`no-store`の付与より前にあり、**未ログイン**でこの経路を通ると
/// ヘッダーが一切付かなかった(ログイン中の`unknown_url_404_while_logged_in_has_no_store`
/// は既存で緑だったため、この非対称は見落とされていた)。
#[sqlx::test]
async fn unknown_url_404_without_login_has_no_store(pool: PgPool) {
    let response = get_unknown_url(&pool, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL),
        Some(&header::HeaderValue::from_static("no-store")),
        "未ログインで未知URLを踏んだ404にno-storeが付いていない"
    );
    assert_eq!(
        response
            .headers()
            .get_all(header::CACHE_CONTROL)
            .iter()
            .count(),
        1,
        "二重付与されていないこと"
    );
}

/// 持ち越しレビュー指摘: `require_auth`配下(`/threads/{id}`)で起きる404にも
/// `Cache-Control: no-store`が付き、かつちょうど1個であること。既存の
/// `unknown_url_404_*_has_no_store`はいずれも`fallback`(未知URL、`require_auth`の
/// **外**)を叩いており、`require_auth`(付与1)と`reflect_auth_on_error_page`
/// (`AuthAwareErrorPage`マーカーを見て付与2、ただし`insert`で上書き)の両方を
/// 通る経路は未検証だった(`middleware.rs`モジュールdoc参照)。
#[sqlx::test]
async fn nonexistent_thread_id_under_require_auth_404_has_no_store_exactly_once(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_detail(&pool, Some(&cookie_header), "999999").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL),
        Some(&header::HeaderValue::from_static("no-store")),
        "require_auth配下の404にno-storeが付いていない"
    );
    assert_eq!(
        response
            .headers()
            .get_all(header::CACHE_CONTROL)
            .iter()
            .count(),
        1,
        "require_authとreflect_auth_on_error_pageの二重付与が起きていないこと"
    );
}

/// スレッド一覧のカードから詳細への導線が実際に機能する(F09→F10の配線確認)。
#[sqlx::test]
async fn thread_list_card_link_navigates_to_the_correct_detail_page(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "配線確認用スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let app = bbs::web::build_router(pool.clone());
    let list_response = app
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
    let expected_href = format!(r#"href="/threads/{tid}""#);
    assert!(
        list_html.contains(&expected_href),
        "一覧のカードに{expected_href}が無い: {list_html}"
    );

    let response = get_detail(&pool, Some(&cookie_header), &tid.to_string()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    assert!(html.contains("配線確認用スレッド"));
}
