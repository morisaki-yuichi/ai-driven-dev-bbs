//! GET / (P03スレッド一覧画面)。F09(スレッド一覧表示、issues/09)・
//! F11(検索、issues/11_search_function.md)。
//!
//! F09の範囲は「初期表示順のみ」(ユーザー承認済みのスコープ)。decision 0009が定める
//! 作成日時降順(idタイブレーク)と、ページネーション・空状態表示を実装する。
//! ソート切替UI・他のソートキーはF12の範囲であり、ここでは実装しない
//! ――`domain::query::SortKey`はF12全体を見越した先行実装だが、ここではDB側の
//! `order by created_at desc, id desc`(decision 0009)がその`CreatedDesc`1本だけを
//! 常に使う形に相当する。`ListParams`は`sort`もパースするが、このハンドラは
//! 値を検証せずページネーションリンクへ素通りさせるだけ(C-13、下記参照)。
//!
//! 一覧の取得方式は「全件取得 → `domain::query::paginate`(純粋関数)に渡す」。
//! SQL側にLIMIT/OFFSETを足さない(functional coreの設計思想に整合させ、
//! ページングを純粋関数としてテスト可能に保つ判断)。
//!
//! **F11(検索)**: `db::threads::search`を使う ―― 空クエリ(`q == ""`)は全件表示
//! (decision 0011)なので、一覧表示(F09)は検索の特殊ケースとして統一的に扱える
//! (`db::threads::search`のdocコメント参照)。ヒット箇所(本文/コメント)の決定は
//! `domain::query::hit_location`
//! (`formal/Bbs/Query.lean`の`hitIn`に対応する純粋関数)が担い、コメントヒットの
//! 場合は詳細画面への導線に`#comment-{id}`フラグメントを付ける(AC11-3、D19)。
//! フラグメント識別子はブラウザ標準機能でスクロールするため、decision 0008
//! (JSなし)の範囲に収まる。
//!
//! **C-13**: ページ送りリンクは`q`・`sort`を保持したまま`page`だけを変える
//! (`q`はURLクエリ値としてパーセントエンコードが要るため`web::params::encode_query_component`
//! を通す。`sort`は`SortKey::as_query_value()`が返す固定のASCII文字列なので不要)。

use std::collections::HashMap;

use askama::Template;
use axum::{
    extract::{Extension, Query, State},
    response::{Html, IntoResponse, Response},
};
use sqlx::PgPool;

use crate::db;
use crate::db::sessions::AuthenticatedUser;
use crate::domain::query::{self, Hit};
use crate::web::csrf::CsrfToken;
use crate::web::error::AppError;
use crate::web::format::format_created_at;
use crate::web::params::{ListParams, encode_query_component};
use crate::web::views::CurrentUser;

/// 一覧に描画する1件ぶんの行。`db::threads::SearchRow`をテンプレートが
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
    /// AC11-3/D19: ヒットしたコメントへのスクロール先。`Some("#comment-42")`の形
    /// (本文ヒット・検索なしのときは`None`、詳細画面のURLに何も付けない)。
    hit_fragment: Option<String>,
}

#[derive(Template)]
#[template(path = "thread_list.html")]
struct ThreadListTemplate {
    current_user: Option<CurrentUser>,
    threads: Vec<ThreadListItem>,
    /// 空リストの理由を区別するために要る。`threads`(現在のページ)が空でも、
    /// 検索条件(`q`)に一致するスレッドが他のページに存在して指定ページが
    /// 範囲外なだけ(decision 0013)という場合があり、「一致するスレッドが無い」と
    /// 同じ文言を出すと誤読される。**F11実装後の実態**: `search`は空クエリでも
    /// 全件表示(decision 0011)を返すため、この値は「(検索条件込みの)問い合わせが
    /// 1件以上ヒットしたか」を表す ―― 検索していない通常の一覧でも常にこの値を
    /// 経由する(空クエリは常に全件にヒットする特殊ケース)。
    has_matching_threads: bool,
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
    /// F11: 検索窓の初期値(HTML属性値としてそのまま出す。Askamaが`"`等をHTMLエスケープする)。
    /// トリム済み・`MAX_QUERY_LEN`以内に切り詰め済み(decision 0033、`ListParams::parse`)
    /// ―― 実際に検索へ使った値と画面表示が一致する。
    q: String,
    /// F11/C-13: ページ送りリンクの`q`(パーセントエンコード済み、URLクエリ値として使う)。
    q_encoded: String,
    /// C-13: ページ送りリンクの`sort`。`SortKey::as_query_value()`はASCII固定文字列
    /// なのでエンコード不要。
    sort_value: &'static str,
    /// F11: 検索中かどうか(`q`が空でないか)。空状態の文言分岐に使う。
    is_searching: bool,
    /// decision 0033: `q`が`MAX_QUERY_LEN`を超えていたため切り詰められたかどうか。
    /// 黙って切り詰めない ―― `true`のとき画面上に切り詰めが起きた旨を表示する。
    q_truncated: bool,
}

/// GET /。`require_auth`ミドルウェア配下のルートなので、ここに到達した時点で
/// `AuthenticatedUser`がリクエスト拡張に必ず存在する(C-09、AC09-1、AC11-1)。
/// `Cache-Control: no-store`は`require_auth`側で一括付与される(C-11)。
///
/// `ListParams::parse`が`q`/`sort`/`page`を一体でパースする。`sort`は現状
/// `SortKey::CreatedDesc`固定(F12は範囲外、上記モジュールdocコメント参照)で、
/// ページネーションリンクへ素通りさせるだけ(C-13)。`q`はF11検索に使う。
pub async fn show(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
    Query(raw_params): Query<HashMap<String, String>>,
) -> Result<Response, AppError> {
    let params = ListParams::parse(&raw_params);

    // F11/decision 0011: `search`は空クエリで全件を返すので、一覧表示(F09)と
    // 検索(F11)を同じ取得経路に統一できる(decision 0009の初期表示順=
    // `order by created_at desc, id desc`はSQL側で確定、db::threads::search参照)。
    let rows = db::threads::search(&pool, &params.q).await?;
    let items: Vec<ThreadListItem> = rows
        .into_iter()
        .map(|r| {
            // AC11-3/D19: ヒット箇所(本文優先、`formal/Bbs/Query.lean`の`hitIn`と同型)。
            // 空クエリでは常に本文ヒットになる(`contains_substr(_, "") == true`)ので、
            // 検索していない通常の一覧ではフラグメントは付かない。
            let hit_fragment = match query::hit_location(&r.body, &params.q, r.hit_comment_id) {
                Some(Hit::Comment(cid)) => Some(format!("#comment-{cid}")),
                Some(Hit::Body) | None => None,
            };
            ThreadListItem {
                id: r.id,
                title: r.title,
                body: r.body,
                author_display_name: r.author_display_name,
                // decision 0009: UTC保存・JST表示。相対時刻表示("3分前"等)は
                // 原典が求めておらず、導入しない。
                created_at: format_created_at(r.created_at),
                comment_count: r.comment_count,
                last_updated_at: format_created_at(r.last_updated_at),
                hit_fragment,
            }
        })
        .collect();

    // 全件取得 → 純粋関数`paginate`でページ分割(SQL側にLIMIT/OFFSETを足さない)。
    // decision 0011: 検索結果にもページネーションを適用する。
    let has_matching_threads = !items.is_empty();
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
        has_matching_threads,
        has_prev: page.has_prev,
        prev_page: page.page_number.saturating_sub(1),
        has_next: page.has_next,
        thread_deleted_message,
        // Why: `?page=4294967295`(u32::MAX)は`ListParams::parse`を素通りするため、
        // 素の`+ 1`はdebugビルドで算術オーバーフローのpanicになる(＝クエリ文字列から
        // ハンドラを落とせる)。`prev_page`側の`saturating_sub`と対称に飽和させる。
        // このページでは`has_next = false`なのでテンプレートは値を読まない。
        next_page: page.page_number.saturating_add(1),
        is_searching: !params.q.is_empty(),
        q_encoded: encode_query_component(&params.q),
        q_truncated: params.q_truncated,
        q: params.q,
        sort_value: params.sort.as_query_value(),
    };
    match tmpl.render() {
        Ok(body) => Ok(Html(body).into_response()),
        Err(e) => Err(AppError::from(e)),
    }
}
