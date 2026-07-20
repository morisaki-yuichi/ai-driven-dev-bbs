//! F05スレッド作成の結合テスト(Router全体 = ミドルウェア込みでAction層まで通す)。
//! AC05-1〜AC05-4、C-09(未ログイン不可)、decision 0002(1リクエスト=1トランザクション)、
//! decision 0021(CSRF)。formal/Bbs/Invariant.leanの`createThread_requires_auth`
//! (未認証では状態を書き換えず失敗する)・`createThread_atomic`(decision 0002)・
//! `createThread_does_not_modify_existing_threads`(C-05/AC05-4)をオラクルとして、
//! 対応する結合テストを置く。

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;
use common::{HOST, insert_test_user, origin_header, urlencoding_stub};

/// ユーザーを登録済みの状態から実際に`POST /login`を通し、
/// (`session_id=...; csrf_token=...`のCookieヘッダ)を返す。logout_test.rsと同じ形。
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

fn post_create_thread_request(
    cookie_header: &str,
    csrf_token: &str,
    title: &str,
    body: &str,
) -> Request<Body> {
    let form = format!(
        "title={}&body={}&csrf_token={}",
        urlencoding_stub(title),
        urlencoding_stub(body),
        urlencoding_stub(csrf_token),
    );
    Request::builder()
        .method("POST")
        .uri("/threads/new")
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

/// AC05-1の前提: ログイン状態でGET /threads/newを開くとフォームが描画される。
#[sqlx::test]
async fn get_thread_new_renders_form_with_csrf_hidden_input(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/threads/new")
                .header(header::HOST, HOST)
                .header(header::COOKIE, cookie_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    assert!(html.contains("スレッド作成"));
    assert!(html.contains(r#"action="/threads/new""#));
    assert!(html.contains(r#"name="csrf_token""#));
    assert!(html.contains(r#"name="title""#));
    assert!(html.contains(r#"name="body""#));
}

/// C-09/AC09-1相当: 未ログインでGET /threads/newへアクセスするとログイン画面へ
/// リダイレクトされる(formal/Bbs/Invariant.leanの`createThread_requires_auth`と
/// 同じガード)。
#[sqlx::test]
async fn get_thread_new_without_login_redirects_to_login(pool: PgPool) {
    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/threads/new")
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");
}

/// 詳細要件/C-09: 未ログインでのPOST /threads/newもログイン画面へリダイレクトされ、
/// スレッドは作られない(`createThread_requires_auth`: 状態を書き換えず失敗する)。
#[sqlx::test]
async fn post_thread_new_without_login_redirects_and_creates_no_thread(pool: PgPool) {
    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/threads/new")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, origin_header())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("title=t&body=b&csrf_token=guessed-token"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");

    let count: (i64,) = sqlx::query_as("select count(*) from threads")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

/// AC05-1・AC05-3: ログイン状態でタイトル・本文を入力して作成すると、
/// 一覧画面(P03)へリダイレクトされ、DBに永続化される。
#[sqlx::test]
async fn post_thread_new_with_valid_data_redirects_to_list_and_persists_thread(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_create_thread_request(
            &cookie_header,
            &csrf_token,
            "AI駆動開発の未来について",
            "本文です",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/");

    let saved: (String, String) =
        sqlx::query_as("select title, body from threads where title = $1")
            .bind("AI駆動開発の未来について")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(saved.0, "AI駆動開発の未来について");
    assert_eq!(saved.1, "本文です");
}

/// AC05-3: 作成後、スレッド一覧画面(GET /)に新しいスレッドが表示される。
#[sqlx::test]
async fn created_thread_appears_on_thread_list(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let create_response = app
        .oneshot(post_create_thread_request(
            &cookie_header,
            &csrf_token,
            "AI駆動開発の未来について",
            "本文です",
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::SEE_OTHER);

    let app2 = bbs::web::build_router(pool.clone());
    let list_response = app2
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::HOST, HOST)
                .header(header::COOKIE, cookie_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let html = get_body_text(list_response).await;
    assert!(html.contains("AI駆動開発の未来について"));
    assert!(html.contains("本文です"));
    assert!(html.contains("テストユーザー01"));
}

/// AC05-2: タイトルが空の状態で作成しようとすると、エラーが表示され作成されない。
#[sqlx::test]
async fn post_thread_new_blank_title_is_rejected_and_creates_no_thread(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    // 全角スペースのみのタイトル。decision 0004の「空」の定義を適用する。
    let response = app
        .oneshot(post_create_thread_request(
            &cookie_header,
            &csrf_token,
            "　　",
            "本文です",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    assert!(html.contains("タイトルを入力してください"));
    // 失敗後も入力済みの本文は消えない(ui-ux-guidelines §2)。
    assert!(html.contains(r#">本文です<"#));
    // 共通メッセージエリアに失敗の要約が出る。
    assert!(html.contains(r#"aria-live="assertive""#));
    assert!(html.contains("スレッドを作成できませんでした。入力内容を確認してください。"));

    let count: (i64,) = sqlx::query_as("select count(*) from threads")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

/// AC05-2: 本文が空の状態で作成しようとすると、エラーが表示され作成されない。
#[sqlx::test]
async fn post_thread_new_blank_body_is_rejected_and_creates_no_thread(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_create_thread_request(
            &cookie_header,
            &csrf_token,
            "タイトル",
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = get_body_text(response).await;
    assert!(html.contains("本文を入力してください"));
    // 失敗後も入力済みのタイトルは消えない。
    assert!(html.contains(r#"value="タイトル""#));

    let count: (i64,) = sqlx::query_as("select count(*) from threads")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

/// 一覧HTMLからスレッド1件ぶんのカード(`<article class="thread-card">`〜`</article>`)を
/// すべて切り出す。C-05の検証をこの領域に限定するために使う。
fn thread_cards(html: &str) -> Vec<String> {
    html.split(r#"<article class="thread-card">"#)
        .skip(1)
        .map(|rest| {
            rest.split("</article>")
                .next()
                .expect("thread-card should be closed")
                .to_string()
        })
        .collect()
}

/// HTML断片から`href="..."`の値をすべて取り出す。属性値にダブルクォートを含む
/// hrefは書かない前提の簡易パーサで足りる(Askamaのエスケープが`"`を`&quot;`にする)。
fn hrefs_in(fragment: &str) -> Vec<String> {
    fragment
        .split(r#"href=""#)
        .skip(1)
        .map(|rest| {
            rest.split('"')
                .next()
                .expect("href attribute should be closed")
                .to_string()
        })
        .collect()
}

/// C-05/AC05-4: 作成したスレッドに、作成後の編集を行うUIが存在しない
/// (formal/Bbs/Op.leanにスレッド更新操作が無いことに対応)。
///
/// ページ全体に対する`!html.contains("編集")`では検証しない。それだと
/// (a) F04のプロフィール編集UIがlayoutに入った時点、(b) 利用者が「編集」を含む
/// タイトルを投稿した時点、のどちらでも壊れる ―― C-05とは無関係な理由で落ちる
/// 脆いテストになる。代わりに「スレッドカードの中に**変更手段**が無い」ことと
/// 「編集用のエンドポイントが存在しない」ことを直接確かめる。
///
/// Why-not: 「カード内に`<a`も含めて操作要素が皆無」までは要求しない。C-05が
/// 禁じるのは作成後の**編集**であって、スレッドに対するあらゆる操作要素ではない。
/// 一律禁止にすると、F10(スレッド詳細)が一覧から詳細へ張る当然のリンクを
/// C-05の名目で塞いでしまう。代わりに、
/// - `<form`/`<button`/`<input`: 禁止を維持する。decision 0021により状態変更は
///   すべてPOSTなので、編集の手段はこの3つのいずれかを必ず伴う。
/// - `<a`(GET遷移): カード内の全hrefが`/threads/<数字>`の形で、かつ"edit"を
///   含まないことを検証する。編集フォームへの導線(`/threads/1/edit`等)はこれで弾ける。
///
/// なお現時点の実装はカード内にリンクを持たない(F10が入るまで`hrefs`は空)。
/// このテストはF10でリンクが入っても壊れないよう先に緩めてあるだけで、
/// リンクの追加自体はF10の範囲。
#[sqlx::test]
async fn thread_list_has_no_edit_ui_for_created_thread(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;
    let csrf_token = csrf_token_from_cookie_header(&cookie_header);

    let app = bbs::web::build_router(pool.clone());
    // タイトル自体に「編集」を含めておく。C-05の検証がスレッド本文の字面に
    // 引きずられないこと(上記の理由(b))をテスト側で担保する。
    let title = "編集できないことの確認用スレッド";
    let create_response = app
        .oneshot(post_create_thread_request(
            &cookie_header,
            &csrf_token,
            title,
            "本文です",
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::SEE_OTHER);

    let app2 = bbs::web::build_router(pool.clone());
    let list_response = app2
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
    let html = get_body_text(list_response).await;

    let cards = thread_cards(&html);
    assert_eq!(cards.len(), 1, "作成した1件だけが一覧に出ているはず");
    let card = &cards[0];
    assert!(card.contains(title), "対象スレッドのカードであること");
    // 状態変更の手段(decision 0021により全てPOST)がカード内に無い ＝
    // 個々のスレッドを変更する操作が存在しない。
    for control in ["<form", "<button", "<input"] {
        assert!(
            !card.contains(control),
            "スレッドカードに状態変更の手段 {control} があってはならない: {card}"
        );
    }
    // GET遷移(リンク)は詳細への導線として許すが、行き先は`/threads/<数字>`に限る。
    for href in hrefs_in(card) {
        let id = href
            .strip_prefix("/threads/")
            .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()));
        assert!(
            id.is_some(),
            "スレッドカード内のリンク先は/threads/<数字>のみ許される: {href}"
        );
        assert!(
            !href.contains("edit"),
            "スレッドカードに編集画面への導線があってはならない: {href}"
        );
    }

    // 編集用のエンドポイント自体が存在しない(ルータに生えていない)。
    for (method, uri) in [
        ("GET", "/threads/1/edit"),
        ("POST", "/threads/1/edit"),
        ("POST", "/threads/1"),
    ] {
        let app = bbs::web::build_router(pool.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
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
}

/// decision 0021: POST /threads/newもCSRF二重送信トークン検証の対象(例外なし)。
#[sqlx::test]
async fn post_thread_new_with_mismatched_csrf_token_is_rejected_with_403(pool: PgPool) {
    insert_test_user(&pool, "testuser_01", "TestPassword123!").await;
    let cookie_header = login(&pool, "testuser_01", "TestPassword123!").await;

    let app = bbs::web::build_router(pool.clone());
    let response = app
        .oneshot(post_create_thread_request(
            &cookie_header,
            "totally-different-token",
            "タイトル",
            "本文",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // ログイン中でもCSRFエラー画面はログアウトフォームを描画しない(web/error.rsの
    // `csrf_error`のWhy-not)。有効なトークンを持てない画面に、押せば必ず403になる
    // ボタンを置かないための不変条件。
    let html = get_body_text(response).await;
    assert!(
        !html.contains(r#"<button type="submit">ログアウト</button>"#),
        "CSRFエラー画面にログアウトフォームが描画されている: {html}"
    );

    let count: (i64,) = sqlx::query_as("select count(*) from threads")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}
