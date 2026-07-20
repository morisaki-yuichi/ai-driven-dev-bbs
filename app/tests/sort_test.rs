//! F12ソート(P03、issues/12_sort_function.md)の結合テスト。
//!
//! シナリオ04-2(`docs/evaluation/scenarios/04_search_and_sort.md`)の手順(作成日時昇順→
//! コメント数順→最終更新日時順の順に切り替え、そのつど並びを確認する)をHTTP経路で追う。
//!
//! `db::threads::insert`/`db::comments::insert`は`now()`任せなのでテスト内で
//! 作成順を制御できない。ここでは`thread_list_test.rs`と同じ方針で、`created_at`を
//! 明示指定した生SQLで直接INSERTする(`db/threads.rs`のテストとも同じ扱い)。

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

/// 2026年内の基準時刻に`offset_secs`秒を足した時刻。作成順を明示的に制御するために使う
/// (`thread_list_test.rs::far_future`と同じ理由で、`now()`任せにしない)。
fn at(offset_secs: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_780_000_000 + offset_secs).unwrap()
}

async fn insert_thread_at(
    pool: &PgPool,
    author_id: i64,
    title: &str,
    body: &str,
    created_at: OffsetDateTime,
) -> i64 {
    sqlx::query_scalar!(
        "insert into threads (author_id, title, body, created_at) values ($1, $2, $3, $4) returning id",
        author_id,
        title,
        body,
        created_at,
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn insert_comment_at(
    pool: &PgPool,
    thread_id: i64,
    author_id: i64,
    created_at: OffsetDateTime,
) {
    sqlx::query!(
        "insert into comments (thread_id, author_id, body, created_at) values ($1, $2, 'コメント本文', $3)",
        thread_id,
        author_id,
        created_at,
    )
    .execute(pool)
    .await
    .unwrap();
}

/// 一覧HTML内でのスレッドタイトルの出現位置。並び順の検証に使う
/// (`assert!(a_pos < b_pos)`でAがBより先に表示されていることを確認する)。
fn position_of(html: &str, title: &str) -> usize {
    html.find(title)
        .unwrap_or_else(|| panic!("「{title}」が一覧に見つからない: {html}"))
}

/// シナリオ04-2-2/AC12-?: 作成日時を昇順に切り替えると、一番古いスレッドが先頭に表示される。
#[sqlx::test]
async fn sort_created_asc_shows_the_oldest_thread_first(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    insert_thread_at(&pool, uid, "古いスレッド", "本文", at(0)).await;
    insert_thread_at(&pool, uid, "新しいスレッド", "本文", at(100)).await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get(&pool, &cookie_header, "/?sort=created_asc").await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    assert!(
        position_of(&html, "古いスレッド") < position_of(&html, "新しいスレッド"),
        "作成日時昇順なら最古のスレッドが先頭に来るはず: {html}"
    );
}

/// AC12-3/C-16: コメント数順に切り替えると、コメントが多いスレッドが上位に来る
/// (削除済みコメントも数える、decision 0010)。
/// **作成日時の新旧をコメント数の多少とわざと逆にする**(`few`を後から作る)。
/// そうしないと、既定のSQL順(作成日時降順)がたまたま期待する並びと一致し、
/// ソート未適用でもテストが偶然通ってしまう(実際にこの取り違えで一度Red確認が
/// すり抜けた ―― `sort_thread_fields`の呼び出しを外しても本テストだけは緑のままだった)。
#[sqlx::test]
async fn sort_comment_count_desc_shows_the_most_commented_thread_first(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let many = insert_thread_at(&pool, uid, "コメント多いスレッド", "本文", at(0)).await;
    let few = insert_thread_at(&pool, uid, "コメント少ないスレッド", "本文", at(10)).await;
    insert_comment_at(&pool, few, uid, at(20)).await;
    insert_comment_at(&pool, many, uid, at(30)).await;
    insert_comment_at(&pool, many, uid, at(40)).await;
    insert_comment_at(&pool, many, uid, at(50)).await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get(&pool, &cookie_header, "/?sort=comment_count_desc").await;
    let html = get_body_text(response).await;
    assert!(
        position_of(&html, "コメント多いスレッド") < position_of(&html, "コメント少ないスレッド"),
        "コメント数降順ならコメントが多いスレッドが先頭に来るはず: {html}"
    );
}

/// シナリオ04-2-3: 最終更新日時順に切り替えると、直前にコメントが付いたスレッドが
/// 先頭に来る(作成日時自体は古くてもよい)。
#[sqlx::test]
async fn sort_last_updated_desc_shows_the_most_recently_commented_thread_first(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let older_created_but_recently_commented =
        insert_thread_at(&pool, uid, "スレッドB", "関係ない本文", at(0)).await;
    insert_thread_at(&pool, uid, "スレッドA", "本文", at(100)).await;
    insert_comment_at(&pool, older_created_but_recently_commented, uid, at(200)).await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get(&pool, &cookie_header, "/?sort=last_updated_desc").await;
    let html = get_body_text(response).await;
    assert!(
        position_of(&html, "スレッドB") < position_of(&html, "スレッドA"),
        "最終更新日時降順なら直前にコメントが付いたスレッドBが先頭に来るはず: {html}"
    );
}

/// decision 0009: 既定(sortパラメータ無し)は作成日時降順のまま(F12でも既定を変えない)。
#[sqlx::test]
async fn default_sort_is_still_created_desc(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    insert_thread_at(&pool, uid, "古いスレッド", "本文", at(0)).await;
    insert_thread_at(&pool, uid, "新しいスレッド", "本文", at(100)).await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get(&pool, &cookie_header, "/").await;
    let html = get_body_text(response).await;
    assert!(
        position_of(&html, "新しいスレッド") < position_of(&html, "古いスレッド"),
        "既定は作成日時降順のはず: {html}"
    );
}

/// P03: 選択中のソートが`<select>`の`selected`属性に反映されている
/// (ui-ux-guidelines §1 初期表示/状態保持)。
#[sqlx::test]
async fn sort_select_reflects_the_current_sort_value(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    insert_thread_at(&pool, uid, "スレッド", "本文", at(0)).await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get(&pool, &cookie_header, "/?sort=comment_count_desc").await;
    let html = get_body_text(response).await;
    assert!(
        html.contains(r#"<option value="comment_count_desc" selected>"#),
        "選択中のソートにselected属性が付いていない: {html}"
    );
    // 他の選択肢にはselectedが付かない。
    assert!(!html.contains(r#"<option value="created_desc" selected>"#));
}

/// scope 3(G1承認事項): 検索・ソート・ページ送りの3つを同時に使っても、
/// 検索語`q`とソート`sort`の両方がページ送りリンクに保持される(C-13)。
/// 検索フォームと同じ`<form>`にソートの`<select>`を置いた(独立formにしない)ことの
/// 直接的な回帰テスト ―― 独立formだと、ソート変更時に`q`が失われうる。
#[sqlx::test]
async fn pagination_links_preserve_both_query_and_sort(pool: PgPool) {
    let uid = insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    for i in 1..=11 {
        insert_thread_at(
            &pool,
            uid,
            &format!("スレッド{i:02}"),
            "Rustの話題です",
            at(i),
        )
        .await;
    }
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let response = get(
        &pool,
        &cookie_header,
        "/?q=Rust&sort=comment_count_desc&page=1",
    )
    .await;
    let html = get_body_text(response).await;
    assert!(
        html.contains(r#"href="/?q=Rust&sort=comment_count_desc&page=2""#),
        "ページ送りリンクにq・sortの両方が保持されていない: {html}"
    );
}
