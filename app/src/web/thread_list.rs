//! GET / (P03スレッド一覧画面)。F09(スレッド一覧表示、issues/09)。
//!
//! **範囲は「初期表示順のみ」**(ユーザー承認済みのスコープ)。decision 0009が定める
//! 作成日時降順(idタイブレーク)と、ページネーション・空状態表示を実装する。
//! ソート切替UI・他のソートキーはF12の範囲であり、ここでは実装しない
//! ――`domain::query::SortKey`はF12全体を見越した先行実装だが、ここではDB側の
//! `order by created_at desc, id desc`(decision 0009)がその`CreatedDesc`1本だけを
//! 常に使う形に相当する。
//!
//! 一覧の取得方式は「全件取得 → `domain::query::paginate`(純粋関数)に渡す」。
//! SQL側にLIMIT/OFFSETを足さない(functional coreの設計思想に整合させ、
//! ページングを純粋関数としてテスト可能に保つ判断)。

use std::collections::HashMap;

use askama::Template;
use axum::{
    extract::{Extension, Query, State},
    response::{Html, IntoResponse, Response},
};
use sqlx::PgPool;

use crate::db;
use crate::db::sessions::AuthenticatedUser;
use crate::domain::query;
use crate::web::csrf::CsrfToken;
use crate::web::error::AppError;
use crate::web::format::format_created_at;
use crate::web::params::ListParams;
use crate::web::views::CurrentUser;

/// 一覧に描画する1件ぶんの行。`db::threads::ThreadListRow`をテンプレートが
/// 扱いやすい形(日時を表示用文字列に変換済み)へ写す。
struct ThreadListItem {
    /// F10詳細画面への導線(`/threads/{id}`)に使う(ユーザー承認済みのスコープ)。
    id: i64,
    title: String,
    body: String,
    author_display_name: String,
    created_at: String,
    /// D13/decision 0010: 削除済みコメントも数える。
    comment_count: i64,
    /// C-15/AC09-4/decision 0010: スレッド作成時刻または最新コメント投稿時刻。
    last_updated_at: String,
}

#[derive(Template)]
#[template(path = "thread_list.html")]
struct ThreadListTemplate {
    current_user: Option<CurrentUser>,
    threads: Vec<ThreadListItem>,
    /// 空リストの理由を区別するために要る。`threads`が空でも、スレッド自体は
    /// 存在して指定ページが範囲外なだけ(decision 0013)という場合があり、
    /// 「スレッドが0件」と同じ文言を出すと誤読される。
    has_any_threads: bool,
    /// C-12: 1ページ目では出さない。`{% if %}`で要素ごと消す
    /// (ui-ux-guidelines §6: 無効化してラベルを残すだけの実装は不可)。
    has_prev: bool,
    prev_page: u32,
    /// C-12: 最終ページでは出さない。
    has_next: bool,
    next_page: u32,
    /// F06/decision 0024と同じクエリパラメータ方式: スレッド削除成功後の
    /// リダイレクト先(`?thread_deleted=1`、`web/thread_detail.rs::delete_thread`)が
    /// このフラッシュ通知を出す。削除は`/threads/{id}`ではなくここに戻ってくる
    /// (削除後はスレッド自体が404になり元の詳細画面へは戻れないため)。
    thread_deleted_message: Option<String>,
}

/// GET /。`require_auth`ミドルウェア配下のルートなので、ここに到達した時点で
/// `AuthenticatedUser`がリクエスト拡張に必ず存在する(C-09、AC09-1)。
/// `Cache-Control: no-store`は`require_auth`側で一括付与される(C-11)。
///
/// `ListParams::parse`はF11(検索)/F12(ソート)向けに`q`/`sort`も汎用にパースするが、
/// このハンドラが読むのは`page`のみ(decision 0013の範囲外ページ丸めを含む)。
/// `q`/`sort`がクエリ文字列に付いていても無視する(検索・ソートUIを一覧に出していない
/// ので、この段階では到達しえない値のはず)。
pub async fn show(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
    Query(raw_params): Query<HashMap<String, String>>,
) -> Result<Response, AppError> {
    let params = ListParams::parse(&raw_params);

    // decision 0009: 初期表示順(作成日時降順・idタイブレーク)はSQL側の
    // `order by`で確定させる(db::threads::list_all)。SortKey::CreatedDesc以外の
    // 分岐はF12の範囲でありここでは使わない。
    let rows = db::threads::list_all(&pool).await?;
    let items: Vec<ThreadListItem> = rows
        .into_iter()
        .map(|r| ThreadListItem {
            id: r.id,
            title: r.title,
            body: r.body,
            author_display_name: r.author_display_name,
            // decision 0009: UTC保存・JST表示。相対時刻表示("3分前"等)は
            // 原典が求めておらず、導入しない。
            created_at: format_created_at(r.created_at),
            comment_count: r.comment_count,
            last_updated_at: format_created_at(r.last_updated_at),
        })
        .collect();

    // 全件取得 → 純粋関数`paginate`でページ分割(SQL側にLIMIT/OFFSETを足さない)。
    let has_any_threads = !items.is_empty();
    let page = query::paginate(params.page, items);

    // F06: スレッド削除成功後のフラッシュ(`?thread_deleted=1`、値は問わずキーの
    // 有無のみ、decision 0024と同じ方式)。
    let thread_deleted_message = raw_params
        .contains_key("thread_deleted")
        .then(|| "スレッドを削除しました。".to_string());

    let tmpl = ThreadListTemplate {
        current_user: Some(CurrentUser {
            display_name: user.display_name,
            csrf_token,
        }),
        threads: page.items,
        has_any_threads,
        has_prev: page.has_prev,
        prev_page: page.page_number.saturating_sub(1),
        has_next: page.has_next,
        thread_deleted_message,
        // Why: `?page=4294967295`(u32::MAX)は`ListParams::parse`を素通りするため、
        // 素の`+ 1`はdebugビルドで算術オーバーフローのpanicになる(＝クエリ文字列から
        // ハンドラを落とせる)。`prev_page`側の`saturating_sub`と対称に飽和させる。
        // このページでは`has_next = false`なのでテンプレートは値を読まない。
        next_page: page.page_number.saturating_add(1),
    };
    match tmpl.render() {
        Ok(body) => Ok(Html(body).into_response()),
        Err(e) => Err(AppError::from(e)),
    }
}
