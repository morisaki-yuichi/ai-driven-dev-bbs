//! GET / (P03スレッド一覧画面)。
//!
//! **検索・ソート・ページネーション・空状態表示(F09〜F13)はまだ実装しない**
//! (ユーザー承認済みのスコープ、dev-docs/foundation-plan.md)。ここで満たすのは
//! F05のAC05-3(作成後、一覧に新しいスレッドが表示される)を成立させるために
//! 必要な最小限の実データ表示 ―― `db::threads::list_all`をそのまま作成日時降順で
//! 並べるだけで、`domain/query.rs`のソート・ページネーション純関数(F09で使う)は
//! まだ呼ばない。

use askama::Template;
use axum::{
    extract::{Extension, State},
    response::{Html, IntoResponse, Response},
};
use sqlx::PgPool;
use sqlx::types::time::{OffsetDateTime, UtcOffset};

use crate::db;
use crate::db::sessions::AuthenticatedUser;
use crate::web::csrf::CsrfToken;
use crate::web::error::AppError;
use crate::web::views::CurrentUser;

/// 一覧に描画する1件ぶんの行。`db::threads::ThreadListRow`をテンプレートが
/// 扱いやすい形(作成日時を表示用文字列に変換済み)へ写す。
struct ThreadListItem {
    title: String,
    body: String,
    author_display_name: String,
    created_at: String,
}

/// 表示用タイムゾーン。decision 0009: 保存はUTC、**表示はJST**。
/// アプリ固定のオフセットであり、利用者ごとのタイムゾーン設定は原典にない。
const DISPLAY_OFFSET_HOURS: i8 = 9;

/// 作成日時を画面表示用の文字列へ整形する（純粋関数）。
///
/// 書式はdecision 0009が例示する `2026-07-19 14:30`（JST）に合わせた。同decisionは
/// 「表示値から順序が読める形式」「同日内の複数スレッドを区別できる粒度」を要求しており、
/// 分までの表示でこれを満たす。秒・ミリ秒・オフセットまで出す`OffsetDateTime::to_string()`
/// （`2026-07-20 0:10:03.917 +00:00:00`）は利用者向けの表示としては粗いので使わない。
fn format_created_at(at: OffsetDateTime) -> String {
    let offset = UtcOffset::from_hms(DISPLAY_OFFSET_HOURS, 0, 0).expect("JST is a valid offset");
    let local = at.to_offset(offset);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        local.year(),
        local.month() as u8,
        local.day(),
        local.hour(),
        local.minute(),
    )
}

#[derive(Template)]
#[template(path = "thread_list.html")]
struct ThreadListTemplate {
    current_user: Option<CurrentUser>,
    threads: Vec<ThreadListItem>,
}

/// GET /。`require_auth`ミドルウェア配下のルートなので、ここに到達した時点で
/// `AuthenticatedUser`がリクエスト拡張に必ず存在する(C-09、AC09-1)。
/// `Cache-Control: no-store`は`require_auth`側で一括付与される(C-11)。
pub async fn show(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(CsrfToken(csrf_token)): Extension<CsrfToken>,
) -> Result<Response, AppError> {
    let rows = db::threads::list_all(&pool).await?;
    let threads = rows
        .into_iter()
        .map(|r| ThreadListItem {
            title: r.title,
            body: r.body,
            author_display_name: r.author_display_name,
            // decision 0009: UTC保存・JST表示。相対時刻表示("3分前"等)は
            // 原典が求めておらず、導入しない。
            created_at: format_created_at(r.created_at),
        })
        .collect();

    let tmpl = ThreadListTemplate {
        current_user: Some(CurrentUser {
            display_name: user.display_name,
            csrf_token,
        }),
        threads,
    };
    match tmpl.render() {
        Ok(body) => Ok(Html(body).into_response()),
        Err(e) => Err(AppError::from(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::types::time::Date;

    /// `sqlx::types::time`は`Month`を再エクスポートしないため、日付は年内通算日
    /// (ordinal)で組む。`time`クレートを直接の依存に足すほどの話ではない。
    fn utc(year: i32, ordinal: u16, h: u8, min: u8, s: u8) -> OffsetDateTime {
        Date::from_ordinal_date(year, ordinal)
            .unwrap()
            .with_hms(h, min, s)
            .unwrap()
            .assume_utc()
    }

    /// decision 0009 が例示する変換そのもの: DB `2026-07-19T05:30:00Z` → 画面 `2026-07-19 14:30`。
    /// 2026年は平年で、7/19は第200日。
    #[test]
    fn formats_utc_as_jst_minutes() {
        assert_eq!(
            format_created_at(utc(2026, 200, 5, 30, 0)),
            "2026-07-19 14:30"
        );
    }

    /// JSTへの変換で日付が繰り上がる場合(UTCの15時以降)も正しく桁揃えされる。
    #[test]
    fn rolls_over_to_the_next_day_in_jst() {
        assert_eq!(
            format_created_at(utc(2026, 200, 20, 10, 3)),
            "2026-07-20 05:10"
        );
    }

    /// 秒・ミリ秒は表示しない(`to_string()`の`2026-07-20 0:10:03.917 +00:00:00`と違う)。
    /// 第2日 = 1/2、繰り上がって 1/3。月・日・時が1桁の場合のゼロ埋めも見る。
    #[test]
    fn drops_seconds_and_subsecond_precision() {
        assert_eq!(
            format_created_at(utc(2026, 2, 15, 4, 59)),
            "2026-01-03 00:04"
        );
    }
}
