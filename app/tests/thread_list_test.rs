//! F09スレッド一覧表示(P03、issues/09)の結合テスト。
//!
//! **範囲は「初期表示順のみ」**(ユーザー承認済みのスコープ)。decision 0009の
//! 作成日時降順、decision 0013のページネーション境界・空状態表示、decision 0010の
//! コメント数・最終更新日時を対象にする。ソート切替(F12)・検索(F11)はここでは扱わない。
//!
//! F07(コメント作成)が未実装のため、コメントは`comments`テーブルへ直接INSERTする
//! (`db/threads.rs`のテストと同じ扱い)。

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

async fn get_list(pool: &PgPool, cookie_header: &str, path: &str) -> axum::response::Response {
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

/// 2099-01-01T00:00:00Z。実行環境の実時計がいつであっても確実に未来。
/// `created_at`を明示的に固定し、`now()`のタイミング差によるテストのフレーク化を避ける。
fn far_future() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(4_070_908_800).unwrap()
}

async fn insert_comment(pool: &PgPool, thread_id: i64, author_id: i64, deleted: bool) {
    if deleted {
        sqlx::query!(
            "insert into comments (thread_id, author_id, body, created_at, deleted_at) \
             values ($1, $2, 'コメント本文', $3, $3)",
            thread_id,
            author_id,
            far_future(),
        )
        .execute(pool)
        .await
        .unwrap();
    } else {
        sqlx::query!(
            "insert into comments (thread_id, author_id, body, created_at) \
             values ($1, $2, 'コメント本文', $3)",
            thread_id,
            author_id,
            far_future(),
        )
        .execute(pool)
        .await
        .unwrap();
    }
}

/// AC09-2: コメントが無いスレッドはコメント数0件と表示される。
#[sqlx::test]
async fn thread_list_shows_zero_comment_count_without_comments(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_list(&pool, &cookie_header, "/").await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    assert!(html.contains("コメント 0 件"));
    // 未来日付が出ていない ＝ 最終更新日時が作成日時のまま(コメントが無いので進んでいない)。
    assert!(!html.contains("2099-01-01"));
}

/// AC09-4/decision 0010: コメント投稿(削除済み込み・D13)がコメント数・最終更新日時に反映される。
#[sqlx::test]
async fn thread_list_shows_comment_count_and_last_updated_at_after_comments(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let tid = bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    insert_comment(&pool, tid, uid, false).await;
    insert_comment(&pool, tid, uid, true).await; // 削除済みも数える(decision 0010)。
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_list(&pool, &cookie_header, "/").await;
    let html = get_body_text(response).await;
    assert!(html.contains("コメント 2 件"));
    // 最終更新日時がコメントの投稿日時(未来日付・JSTでも同じ日付になる)まで進んでいる。
    assert!(html.contains("2099-01-01"));
}

/// decision 0013 §2 / ui-ux-guidelines §1: スレッドが1件も無い場合、
/// 空白画面ではなく該当0件であることが分かる表示になる。
#[sqlx::test]
async fn thread_list_shows_empty_state_message_when_no_threads(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_list(&pool, &cookie_header, "/").await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    assert!(html.contains("表示できるスレッドがありません"));
    assert!(!html.contains(r#"class="thread-card""#));
    // C-12: 0件時は1ページ目扱いなので前後どちらのリンクも出ない。
    assert!(!html.contains("前に戻る"));
    assert!(!html.contains("次に進む"));
    // 中身が空の`<nav aria-label="ページ送り">`だけが残らないこと。空の要素が残ると
    // 支援技術・`agent-browser`に「ページ送りがある」と観測されうる(ui-ux-guidelines §6)。
    assert!(
        !html.contains(r#"aria-label="ページ送り""#),
        "前後リンクが無いときは<nav>ごと出してはいけない"
    );
}

/// AC09-3/AC09-5: 11件以上のスレッドがあると1ページ目は10件・「前に戻る」非表示・
/// 「次に進む」表示になる。
#[sqlx::test]
async fn thread_list_first_page_has_ten_items_and_only_next_link(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    for i in 1..=11 {
        bbs::db::threads::insert(&pool, uid, &format!("スレッド{i:02}"), "本文")
            .await
            .unwrap();
    }
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_list(&pool, &cookie_header, "/").await;
    let html = get_body_text(response).await;
    // decision 0009: 新しい(=後で作られた)ものが先頭。11番目〜2番目がここに乗る。
    for i in 2..=11 {
        assert!(
            html.contains(&format!("スレッド{i:02}")),
            "スレッド{i:02}が1ページ目に無い"
        );
    }
    assert!(
        !html.contains("スレッド01"),
        "11件目(最古)は1ページ目に出てはいけない"
    );
    assert!(!html.contains("前に戻る"), "1ページ目に「前に戻る」は不可");
    // C-13: ページ送りリンクは`q`・`sort`を保持したまま`page`だけを変える
    // (decision 0011 §影響、web/thread_list.rs::show)。
    assert!(
        html.contains(r#"href="/?q=&sort=created_desc&page=2""#),
        "「次に進む」が無い"
    );
}

/// AC09-3/AC09-5: 2ページ目には残りの1件だけが表示され、「前に戻る」が有効、
/// 「次に進む」は出ない(最終ページ)。
#[sqlx::test]
async fn thread_list_second_page_has_remaining_item_and_only_prev_link(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    for i in 1..=11 {
        bbs::db::threads::insert(&pool, uid, &format!("スレッド{i:02}"), "本文")
            .await
            .unwrap();
    }
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_list(&pool, &cookie_header, "/?page=2").await;
    let html = get_body_text(response).await;
    assert!(html.contains("スレッド01"), "最古の1件が2ページ目に無い");
    for i in 2..=11 {
        assert!(
            !html.contains(&format!("スレッド{i:02}")),
            "スレッド{i:02}が2ページ目にも出てしまっている"
        );
    }
    assert!(
        html.contains(r#"href="/?q=&sort=created_desc&page=1""#),
        "2ページ目に「前に戻る」が無い"
    );
    assert!(!html.contains("次に進む"), "最終ページに「次に進む」は不可");
}

/// decision 0013: 範囲外のページ番号は404にせず空リストを返す。総件数を超えるページでも
/// 1ページ目より後ろなので「前に戻る」だけが出る。
///
/// このとき出る文言は「スレッドが1件も無い」場合とは**別**でなければならない。
/// 同じ「表示できるスレッドがありません」を出すと、スレッドが1件存在するのに
/// 0件だと誤読される(利用者にも`agent-browser`にも区別がつかない)。
#[sqlx::test]
async fn thread_list_out_of_range_page_is_empty_not_404(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_list(&pool, &cookie_header, "/?page=999").await;
    assert_eq!(response.status(), StatusCode::OK, "404にしてはいけない");
    let html = get_body_text(response).await;
    assert!(
        html.contains("このページには表示するものがありません"),
        "範囲外ページ専用の文言が出ていない"
    );
    assert!(
        !html.contains("表示できるスレッドがありません"),
        "スレッドが1件あるのに0件時の文言を出してはいけない"
    );
    assert!(!html.contains(r#"class="thread-card""#));
    assert!(html.contains(r#"href="/?q=&sort=created_desc&page=998""#));
    assert!(!html.contains("次に進む"));
}

/// 回帰: `?page=4294967295`(u32::MAX)でハンドラが落ちない。
/// `ListParams::parse`はu32に収まる値をそのまま通すため、テンプレートに渡す
/// 「次のページ番号」を素の`+ 1`で計算するとdebugビルドで算術オーバーフローの
/// panicになる ―― クエリ文字列だけでリクエストを落とせてしまう。
#[sqlx::test]
async fn thread_list_max_page_number_does_not_overflow(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_list(&pool, &cookie_header, "/?page=4294967295").await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    // 範囲外なので空・「次に進む」は出ない(「前に戻る」だけ)。
    assert!(html.contains("このページには表示するものがありません"));
    assert!(!html.contains("次に進む"));
}

/// decision 0013: `?page=0`は1ページ目として扱われる(パース層の丸め、
/// `web/params.rs`の`ListParams::parse`が既にテスト済みのロジックのエンドツーエンド確認)。
#[sqlx::test]
async fn thread_list_page_zero_is_treated_as_first_page(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    bbs::db::threads::insert(&pool, uid, "スレッド", "本文")
        .await
        .unwrap();
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get_list(&pool, &cookie_header, "/?page=0").await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    assert!(html.contains("スレッド"));
    assert!(!html.contains("前に戻る"));
}
