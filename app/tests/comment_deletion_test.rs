//! F08コメント削除の結合テスト(Router全体 = ミドルウェア込みでAction層まで通す)。
//! AC08-1〜AC08-4、C-07(論理削除・本文保持)、C-09(未ログイン不可)、C-10(存在しない
//! /他スレッドのコメントは404)、decision 0002(1リクエスト=1トランザクション)、
//! decision 0021(CSRF)、D18(確認なし即削除・decision 0030)。
//! formal/Bbs/Invariant.leanの`deleteComment_atomic`(decision 0002)・
//! `deletion_irreversible`(C-07/C-08)をオラクルとして、対応する結合テストを置く。
//! `comment_create_test.rs`と同じ形。

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;
use common::{HOST, insert_test_user, origin_header, urlencoding_stub};

/// ユーザーを登録済みの状態から実際に`POST /login`を通し、
/// (`session_id=...; csrf_token=...`のCookieヘッダ)を返す。comment_create_test.rsと同じ形。
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

fn post_delete_comment_request(
    thread_id: i64,
    comment_id: i64,
    cookie_header: &str,
    csrf_token: &str,
) -> Request<Body> {
    let form = format!("csrf_token={}", urlencoding_stub(csrf_token));
    Request::builder()
        .method("POST")
        .uri(format!("/threads/{thread_id}/comments/{comment_id}/delete"))
        .header(header::HOST, HOST)
        .header(header::ORIGIN, origin_header())
        .header(header::COOKIE, cookie_header)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .unwrap()
}

async fn get(pool: &PgPool, cookie_header: &str, uri: &str) -> axum::response::Response {
    let app = bbs::web::build_router(pool.clone());
    app.oneshot(
        Request::builder()
            .uri(uri)
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

async fn comment_deleted_at_is_set(pool: &PgPool, comment_id: i64) -> bool {
    let row: (bool,) = sqlx::query_as("select deleted_at is not null from comments where id = $1")
        .bind(comment_id)
        .fetch_one(pool)
        .await
        .unwrap();
    row.0
}

/// AC08-1/AC08-2: 作成者本人が自分のコメントを削除でき、削除後は本文が固定文言に
/// 置き換わり、成功の通知が出ること(ui-ux-guidelines §2)。D18: 確認なしで即削除
/// されること(このテストがwindow.confirm等を経由せず1回のPOSTで完了することが
/// それ自体の証跡)。
#[sqlx::test]
async fn owner_can_delete_own_comment_and_sees_fixed_text_and_success_message(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cid = bbs::db::comments::insert(&pool, tid, uid, "削除される本文です")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_comment_request(
            tid,
            cid,
            &cookie_header,
            &csrf_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        &format!("/threads/{tid}?comment_deleted=1")
    );

    assert!(comment_deleted_at_is_set(&pool, cid).await);

    let detail = get(
        &pool,
        &cookie_header,
        &format!("/threads/{tid}?comment_deleted=1"),
    )
    .await;
    assert_eq!(detail.status(), StatusCode::OK);
    let html = get_body_text(detail).await;
    assert!(html.contains("コメントを削除しました。"));
    assert!(html.contains("＜このコメントは削除されました＞"));
    assert!(
        !html.contains("削除される本文です"),
        "削除済みの元本文がそのまま出てはならない(C-01)"
    );
}

/// C-07: 削除は論理削除であり、行そのもの・元の本文はDB上に残る
/// (`comments::delete`のテストの結合版)。
#[sqlx::test]
async fn delete_keeps_the_row_and_original_body_in_the_database(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cid = bbs::db::comments::insert(&pool, tid, uid, "元の本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    app.oneshot(post_delete_comment_request(
        tid,
        cid,
        &cookie_header,
        &csrf_token,
    ))
    .await
    .unwrap();

    let saved: (String,) = sqlx::query_as("select body from comments where id = $1")
        .bind(cid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(saved.0, "元の本文");
}

/// AC08-3: 自分以外が作成したコメントを直接POSTで削除しようとしても拒否され、
/// 権限がないことが画面上で観測できるフィードバックとして返ること
/// (ハンドラが`Forbidden`を明示的に捕捉する、web/error.rsの一律400フォールバックに
/// 頼らない)。
#[sqlx::test]
async fn deleting_another_users_comment_is_forbidden_and_shows_message(pool: PgPool) {
    let owner = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    insert_test_user(&pool, "testuser_02", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, owner, "スレッド", "本文")
        .await
        .unwrap();
    let cid = bbs::db::comments::insert(&pool, tid, owner, "他人のコメント")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_02", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_comment_request(
            tid,
            cid,
            &cookie_header,
            &csrf_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        &format!("/threads/{tid}?comment_delete_error=forbidden")
    );
    assert!(
        !comment_deleted_at_is_set(&pool, cid).await,
        "他人のコメントは削除されてはならない"
    );

    let detail = get(
        &pool,
        &cookie_header,
        &format!("/threads/{tid}?comment_delete_error=forbidden"),
    )
    .await;
    let html = get_body_text(detail).await;
    assert!(html.contains("このコメントを削除する権限がありません。"));
    assert!(
        html.contains("他人のコメント"),
        "本文はそのまま表示され続ける"
    );
}

/// AC08-4: 既に削除済みのコメントへの再削除は拒否され、そのことが画面上で
/// 観測できること。二重削除で状態が壊れない(不可逆性の実装側の確認)。
#[sqlx::test]
async fn deleting_an_already_deleted_comment_is_rejected_and_shows_message(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cid = bbs::db::comments::insert(&pool, tid, uid, "二重削除確認用")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let first = app
        .oneshot(post_delete_comment_request(
            tid,
            cid,
            &cookie_header,
            &csrf_token,
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::SEE_OTHER);
    assert!(comment_deleted_at_is_set(&pool, cid).await);

    let app = bbs::web::build_router(pool.clone());
    let second = app
        .oneshot(post_delete_comment_request(
            tid,
            cid,
            &cookie_header,
            &csrf_token,
        ))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        second.headers().get(header::LOCATION).unwrap(),
        &format!("/threads/{tid}?comment_delete_error=already_deleted")
    );
    assert!(
        comment_deleted_at_is_set(&pool, cid).await,
        "削除済みのまま"
    );

    let detail = get(
        &pool,
        &cookie_header,
        &format!("/threads/{tid}?comment_delete_error=already_deleted"),
    )
    .await;
    let html = get_body_text(detail).await;
    assert!(html.contains("このコメントは既に削除されています。"));
}

/// C-09: 未ログインでのPOSTはログイン画面へリダイレクトされ、削除は行われない。
#[sqlx::test]
async fn delete_without_login_redirects_and_does_not_delete(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cid = bbs::db::comments::insert(&pool, tid, uid, "本文")
        .await
        .unwrap();

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/threads/{tid}/comments/{cid}/delete"))
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
    assert!(!comment_deleted_at_is_set(&pool, cid).await);
}

/// C-10: 存在しないコメントIDへの削除は404になる。
#[sqlx::test]
async fn delete_nonexistent_comment_returns_404(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_comment_request(
            tid,
            999_999,
            &cookie_header,
            &csrf_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// C-10: URLの`thread_id`セグメントが実際のコメントの所属スレッドと食い違う場合も
/// 404にする(ネスト構造の整合性)。
#[sqlx::test]
async fn delete_with_mismatched_thread_id_in_url_returns_404(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid1 = bbs::db::threads::insert(&pool, uid, "スレッド1", "本文")
        .await
        .unwrap();
    let tid2 = bbs::db::threads::insert(&pool, uid, "スレッド2", "本文")
        .await
        .unwrap();
    let cid = bbs::db::comments::insert(&pool, tid1, uid, "スレッド1のコメント")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_comment_request(
            tid2,
            cid,
            &cookie_header,
            &csrf_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(!comment_deleted_at_is_set(&pool, cid).await);
}

/// decision 0021: POST /threads/{tid}/comments/{cid}/deleteもCSRF二重送信トークン
/// 検証の対象(例外なし)。
#[sqlx::test]
async fn delete_with_mismatched_csrf_token_is_rejected_with_403(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cid = bbs::db::comments::insert(&pool, tid, uid, "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_comment_request(
            tid,
            cid,
            &cookie_header,
            "totally-different-token",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!comment_deleted_at_is_set(&pool, cid).await);
}

/// ui-ux-guidelines §1 / AC08-3: 自分以外が作成したコメントには削除ボタンが
/// 表示されない(操作不可の状態は要素ごと非表示にする)。
#[sqlx::test]
async fn delete_button_is_hidden_for_another_users_comment(pool: PgPool) {
    let owner = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    insert_test_user(&pool, "testuser_02", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, owner, "スレッド", "本文")
        .await
        .unwrap();
    bbs::db::comments::insert(&pool, tid, owner, "所有者のコメント")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_02", "TestPassword123!").await;

    let detail = get(&pool, &cookie_header, &format!("/threads/{tid}")).await;
    let html = get_body_text(detail).await;
    let comments_section_start = html.find(r#"aria-label="コメント一覧""#).unwrap();
    let comments_section_end = html
        .find(r#"aria-label="コメント投稿""#)
        .unwrap_or(html.len());
    let comments_section = &html[comments_section_start..comments_section_end];
    assert!(
        !comments_section.contains("削除"),
        "他人のコメントに削除ボタンが出てはならない(AC08-3)"
    );
}

/// ui-ux-guidelines §1 / AC08-4: 既に削除済みのコメントには削除ボタンが
/// 表示されない(再削除の手段自体を与えない)。
#[sqlx::test]
async fn delete_button_is_hidden_for_an_already_deleted_comment(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cid = bbs::db::comments::insert(&pool, tid, uid, "削除済みにする本文")
        .await
        .unwrap();
    bbs::db::comments::delete(&pool, cid).await.unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let detail = get(&pool, &cookie_header, &format!("/threads/{tid}")).await;
    let html = get_body_text(detail).await;
    let comments_section_start = html.find(r#"aria-label="コメント一覧""#).unwrap();
    let comments_section_end = html
        .find(r#"aria-label="コメント投稿""#)
        .unwrap_or(html.len());
    let comments_section = &html[comments_section_start..comments_section_end];
    // 削除フォームのaction URLの不在で判定する。以前は`削除</button>`という
    // マークアップの完全一致で見ていたが、buttonに属性(class等)や空白が入るだけで
    // assertが無言で空振りする(F08レビュー指摘)。action URLならボタンの見た目や
    // 文言の変更に影響されず、「再削除を実行する手段が存在しない」という
    // 検証したい性質そのものを突く。
    assert!(
        !comments_section.contains(&format!("/threads/{tid}/comments/{cid}/delete")),
        "削除済みコメントに再削除フォームが出てはならない(AC08-4)"
    );
}

/// 持ち越し事項(1): 対象限定を壊す変異(`db/comments.rs::delete`の`where`を`or true`等に
/// 壊す)の検出層。結合層(Router全体)を通した状態で、削除対象と無関係な**同一スレッド内の
/// 別コメント**が巻き添えで削除済みにならないことを見る(F04で結合スイートがこの種の
/// 分離ケースを欠いていたことが判明したのと同じパターン)。
#[sqlx::test]
async fn deleting_a_comment_does_not_affect_another_comment_in_the_same_thread(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let target = bbs::db::comments::insert(&pool, tid, uid, "削除される本文")
        .await
        .unwrap();
    let victim = bbs::db::comments::insert(&pool, tid, uid, "無関係なコメント")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_comment_request(
            tid,
            target,
            &cookie_header,
            &csrf_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(comment_deleted_at_is_set(&pool, target).await);
    assert!(
        !comment_deleted_at_is_set(&pool, victim).await,
        "同一スレッド内の無関係なコメントが巻き添えで削除されてはならない"
    );

    let detail = get(&pool, &cookie_header, &format!("/threads/{tid}")).await;
    let html = get_body_text(detail).await;
    assert!(
        html.contains("無関係なコメント"),
        "無関係なコメントの本文がそのまま表示され続けるはず"
    );
}

/// 持ち越し事項(1)続き: 犠牲オブジェクトを**別スレッド**に置いた版。
#[sqlx::test]
async fn deleting_a_comment_does_not_affect_a_comment_in_another_thread(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid1 = bbs::db::threads::insert(&pool, uid, "スレッド1", "本文")
        .await
        .unwrap();
    let tid2 = bbs::db::threads::insert(&pool, uid, "スレッド2", "本文")
        .await
        .unwrap();
    let target = bbs::db::comments::insert(&pool, tid1, uid, "削除される本文")
        .await
        .unwrap();
    let victim = bbs::db::comments::insert(&pool, tid2, uid, "無関係なコメント")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_comment_request(
            tid1,
            target,
            &cookie_header,
            &csrf_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(
        !comment_deleted_at_is_set(&pool, victim).await,
        "別スレッドの無関係なコメントが巻き添えで削除されてはならない"
    );
}

/// D18: 確認ダイアログ(`window.confirm`等)を使わない。agent-browserからの
/// 操作性(H-02)のため、削除フォームは通常のPOSTフォームのみで構成され、
/// `confirm(`やインラインの`onclick`ハンドラを含まない。
#[sqlx::test]
async fn delete_form_does_not_use_a_native_confirm_dialog(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    bbs::db::comments::insert(&pool, tid, uid, "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let detail = get(&pool, &cookie_header, &format!("/threads/{tid}")).await;
    let html = get_body_text(detail).await;
    assert!(!html.contains("confirm("));
    assert!(!html.contains("onclick"));
}
