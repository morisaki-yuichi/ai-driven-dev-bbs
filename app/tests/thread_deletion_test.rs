//! F06スレッド削除の結合テスト(Router全体 = ミドルウェア込みでAction層まで通す)。
//! AC06-1〜AC06-4、C-06(コメントが1件でもあれば削除不可・削除済みも数える)、
//! C-08(不可逆)、C-09(未ログイン不可)、C-10(存在しないIDは404)、decision 0002
//! (1リクエスト=1トランザクション)、decision 0014(物理削除)、decision 0021(CSRF)。
//! `comment_deletion_test.rs`と同じ形。
//!
//! formal/Bbs/Invariant.leanの`deleteThread_atomic`(decision 0002)・
//! `deleteThread_needs_owner`・`deleteThread_blocked_by_any_comment`/
//! `deleteThread_blocked_by_deleted_comment`(C-06/AC06-1〜AC06-2)をオラクルとして、
//! 対応する結合テストを置く。

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;
use sqlx::types::time::OffsetDateTime;
use tower::ServiceExt;

mod common;
use common::{HOST, insert_test_user, origin_header, urlencoding_stub};

/// ユーザーを登録済みの状態から実際に`POST /login`を通し、
/// (`session_id=...; csrf_token=...`のCookieヘッダ)を返す。comment_deletion_test.rsと同じ形。
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

fn post_delete_thread_request(
    thread_id: i64,
    cookie_header: &str,
    csrf_token: &str,
) -> Request<Body> {
    let form = format!("csrf_token={}", urlencoding_stub(csrf_token));
    Request::builder()
        .method("POST")
        .uri(format!("/threads/{thread_id}/delete"))
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

async fn thread_exists(pool: &PgPool, thread_id: i64) -> bool {
    let row: Option<(i64,)> = sqlx::query_as("select id from threads where id = $1")
        .bind(thread_id)
        .fetch_optional(pool)
        .await
        .unwrap();
    row.is_some()
}

fn far_future() -> OffsetDateTime {
    // 2099-01-01T00:00:00Z。実行環境の実時計がいつであっても確実に未来。
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

/// AC06-1: 作成者本人が、コメント0件の自分のスレッドを削除でき、一覧画面へ
/// リダイレクトされ(シナリオ02)、成功の通知が出ること(ui-ux-guidelines §2)。
/// 削除確認ダイアログを経由せず1回のPOSTで完了すること自体が「確認なしで
/// 即削除する」設計の証跡になる。
#[sqlx::test]
async fn owner_can_delete_own_thread_with_no_comments_and_sees_success_message(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "削除されるスレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_thread_request(tid, &cookie_header, &csrf_token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        "/?thread_deleted=1"
    );
    assert!(!thread_exists(&pool, tid).await, "物理削除されているはず");

    let list = get(&pool, &cookie_header, "/?thread_deleted=1").await;
    assert_eq!(list.status(), StatusCode::OK);
    let html = get_body_text(list).await;
    assert!(html.contains("スレッドを削除しました。"));
    assert!(
        !html.contains("削除されるスレッド"),
        "削除済みスレッドが一覧に残ってはならない"
    );
}

/// decision 0014: 削除は物理削除。詳細画面へのアクセスは404になる(C-10、
/// 「存在しない」と区別が無い)。
#[sqlx::test]
async fn deleted_thread_detail_returns_404(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    app.oneshot(post_delete_thread_request(tid, &cookie_header, &csrf_token))
        .await
        .unwrap();

    let detail = get(&pool, &cookie_header, &format!("/threads/{tid}")).await;
    assert_eq!(detail.status(), StatusCode::NOT_FOUND);
}

/// AC06-2/C-06: 未削除コメントが1件でもあれば削除できず、画面上で観測できる
/// フィードバックが返ること(F08と同じ観測可能性の方針)。
#[sqlx::test]
async fn deleting_a_thread_with_a_comment_is_rejected_and_shows_message(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "コメント付きスレッド", "本文")
        .await
        .unwrap();
    insert_comment(&pool, tid, uid, "コメント本文", far_future(), false).await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_thread_request(tid, &cookie_header, &csrf_token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        &format!("/threads/{tid}?thread_delete_error=has_comments")
    );
    assert!(
        thread_exists(&pool, tid).await,
        "コメントがあるスレッドは削除されてはならない"
    );

    let detail = get(
        &pool,
        &cookie_header,
        &format!("/threads/{tid}?thread_delete_error=has_comments"),
    )
    .await;
    assert_eq!(detail.status(), StatusCode::OK);
    let html = get_body_text(detail).await;
    assert!(html.contains("コメントが1件以上あるスレッドは削除できません。"));
    assert!(
        html.contains("コメント付きスレッド"),
        "スレッドは残り続ける"
    );
}

/// AC06-2: **削除済みコメントだけ**でも削除を阻む(C-06は削除済みも数える)。
/// `db::threads::delete`の`not exists`が`deleted_at`で絞らないことの回帰。
#[sqlx::test]
async fn deleting_a_thread_with_only_a_deleted_comment_is_rejected(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    insert_comment(&pool, tid, uid, "削除済みコメント", far_future(), true).await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_thread_request(tid, &cookie_header, &csrf_token))
        .await
        .unwrap();
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        &format!("/threads/{tid}?thread_delete_error=has_comments")
    );
    assert!(thread_exists(&pool, tid).await);
}

/// AC06-3: 自分以外が作成したスレッドを直接POSTで削除しようとしても拒否され、
/// 権限がないことが画面上で観測できるフィードバックとして返ること。
#[sqlx::test]
async fn deleting_another_users_thread_is_forbidden_and_shows_message(pool: PgPool) {
    let owner = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    insert_test_user(&pool, "testuser_02", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, owner, "他人のスレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_02", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_thread_request(tid, &cookie_header, &csrf_token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        &format!("/threads/{tid}?thread_delete_error=forbidden")
    );
    assert!(
        thread_exists(&pool, tid).await,
        "他人のスレッドは削除されてはならない"
    );

    let detail = get(
        &pool,
        &cookie_header,
        &format!("/threads/{tid}?thread_delete_error=forbidden"),
    )
    .await;
    let html = get_body_text(detail).await;
    assert!(html.contains("このスレッドを削除する権限がありません。"));
    assert!(
        html.contains("他人のスレッド"),
        "本文はそのまま表示され続ける"
    );
}

/// C-09: 未ログインでのPOSTはログイン画面へリダイレクトされ、削除は行われない。
#[sqlx::test]
async fn delete_without_login_redirects_and_does_not_delete(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/threads/{tid}/delete"))
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
    assert!(thread_exists(&pool, tid).await);
}

/// C-10: 存在しないスレッドIDへの削除は404になる。
#[sqlx::test]
async fn delete_nonexistent_thread_returns_404(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_thread_request(
            999_999,
            &cookie_header,
            &csrf_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// decision 0021: POST /threads/{id}/deleteもCSRF二重送信トークン検証の対象
/// (例外なし)。
#[sqlx::test]
async fn delete_with_mismatched_csrf_token_is_rejected_with_403(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_thread_request(
            tid,
            &cookie_header,
            "totally-different-token",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(thread_exists(&pool, tid).await);
}

/// ui-ux-guidelines §1 / AC06-3: 自分以外が作成したスレッドの詳細画面には
/// 削除ボタンが表示されない(操作不可の状態は要素ごと非表示にする)。
#[sqlx::test]
async fn delete_button_is_hidden_for_another_users_thread(pool: PgPool) {
    let owner = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    insert_test_user(&pool, "testuser_02", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, owner, "他人のスレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_02", "TestPassword123!").await;

    let detail = get(&pool, &cookie_header, &format!("/threads/{tid}")).await;
    let html = get_body_text(detail).await;
    assert!(
        !html.contains(&format!("/threads/{tid}/delete")),
        "他人のスレッドに削除フォームが出てはならない(AC06-3)"
    );
    assert!(!html.contains("スレッドを削除する"));
}

/// ui-ux-guidelines §1 / AC06-2: コメントが1件でもあるスレッドには(所有者が
/// 見ても)削除ボタンが表示されない。
#[sqlx::test]
async fn delete_button_is_hidden_for_a_thread_with_a_comment(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    insert_comment(&pool, tid, uid, "コメント", far_future(), false).await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let detail = get(&pool, &cookie_header, &format!("/threads/{tid}")).await;
    let html = get_body_text(detail).await;
    assert!(
        !html.contains(&format!("/threads/{tid}/delete")),
        "コメント付きスレッドに削除フォームが出てはならない(AC06-2)"
    );
}

/// 対になる正常系: 自分の・コメント0件のスレッドには削除ボタンが表示される。
#[sqlx::test]
async fn delete_button_is_shown_for_owners_thread_with_no_comments(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let detail = get(&pool, &cookie_header, &format!("/threads/{tid}")).await;
    let html = get_body_text(detail).await;
    assert!(html.contains(&format!("/threads/{tid}/delete")));
    assert!(html.contains("スレッドを削除する"));
}

/// 持ち越し事項(1): 対象限定を壊す変異(`db/threads.rs::delete`の`where`を`or true`等に
/// 壊す)の検出層。結合層(Router全体)を通した状態で、削除対象と無関係な**別スレッド**が
/// 巻き添えで消えないことを見る(F04で結合スイートがこの種の分離ケースを欠いていたことが
/// 判明したのと同じパターン)。
#[sqlx::test]
async fn deleting_a_thread_does_not_delete_another_thread(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let target = bbs::db::threads::insert(&pool, uid, "削除される", "本文")
        .await
        .unwrap();
    let victim = bbs::db::threads::insert(&pool, uid, "無関係なスレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_thread_request(
            target,
            &cookie_header,
            &csrf_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(!thread_exists(&pool, target).await);
    assert!(
        thread_exists(&pool, victim).await,
        "無関係な別スレッドが巻き添えで消えてはならない"
    );

    let detail = get(&pool, &cookie_header, &format!("/threads/{victim}")).await;
    assert_eq!(
        detail.status(),
        StatusCode::OK,
        "無関係なスレッドは引き続き表示できるはず"
    );
}

/// 持ち越しレビュー指摘で名称・コメントを訂正: このテストが実際に固定しているのは
/// **`comments.thread_id`の`on delete no action`というFK制約そのもの**であり、
/// `threads::delete`の`where`条件のスコープではない。`where`条件を`or true`等に
/// 広げる変異が入っても、victimスレッドにはコメントが紐づいているため、
/// `delete from threads where ...`の実行自体がFK制約違反として失敗する
/// (`.execute(...)?`がErrを返し、呼び出し元がpanicする)。この場合、下の
/// `assert!(thread_exists(&pool, victim).await)`まで到達する前に検出されており、
/// この assertion 自体は原理的に失敗し得ない(victimに実際にコメントが紐づく限り、
/// FKがDBレベルで巻き添え削除を拒否するため)。
///
/// 「`where`条件のスコープが別スレッドまで広がっていないか」を直接見たい場合は、
/// コメントを持たない victim を使う`deleting_a_thread_does_not_delete_another_thread`
/// (下記)がそれをスレッド単位でカバーしている ―― そちらはFKの保護を受けない
/// (victimにコメントが無い)ため、`where`条件が壊れれば`thread_exists`の
/// assertionが実際に赤くなる。
///
/// テストのロジック自体(FK制約が実際に効いていることの固定)には価値があるため
/// 変更しない。
#[sqlx::test]
async fn fk_constraint_prevents_thread_deletion_from_removing_comments_of_another_thread(
    pool: PgPool,
) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let target = bbs::db::threads::insert(&pool, uid, "削除される", "本文")
        .await
        .unwrap();
    let victim = bbs::db::threads::insert(&pool, uid, "無関係なスレッド", "本文")
        .await
        .unwrap();
    insert_comment(&pool, victim, uid, "無関係なコメント", far_future(), false).await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_delete_thread_request(
            target,
            &cookie_header,
            &csrf_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(thread_exists(&pool, victim).await);

    let detail = get(&pool, &cookie_header, &format!("/threads/{victim}")).await;
    assert_eq!(detail.status(), StatusCode::OK);
    let html = get_body_text(detail).await;
    assert!(
        html.contains("無関係なコメント"),
        "無関係なスレッドのコメントが巻き添えで消えてはならない"
    );
}

/// D18相当(decision 0030と同じ裁定をスレッド削除に適用): 確認ダイアログ
/// (`window.confirm`等)を使わない。agent-browserからの操作性(H-02)のため、
/// 削除フォームは通常のPOSTフォームのみで構成され、`confirm(`や
/// インラインの`onclick`ハンドラを含まない。
///
/// レビュー指摘により検査対象を修正: 削除フォームがあるのは一覧(`/`)ではなく
/// スレッド詳細ページ(`/threads/{id}`)。一覧を検査していては、詳細側が将来
/// `confirm(`を使い始めても検出できない(comment_deletion_test.rsの同名テストと
/// 同じ検査対象に揃える)。
#[sqlx::test]
async fn delete_form_does_not_use_a_native_confirm_dialog(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let detail = get(&pool, &cookie_header, &format!("/threads/{tid}")).await;
    let html = get_body_text(detail).await;
    assert!(
        html.contains(&format!("/threads/{tid}/delete")),
        "削除フォームが存在すること"
    );
    assert!(!html.contains("confirm("));
    assert!(!html.contains("onclick"));
}
