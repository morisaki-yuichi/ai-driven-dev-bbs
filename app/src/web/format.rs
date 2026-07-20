//! 画面表示用の日時整形。`web/thread_list.rs`(F09)・`web/thread_detail.rs`(F10)の
//! 両方が同じ変換(UTC保存・JST表示、分までの精度)を必要とするため、ここに1箇所へ
//! まとめる(decision 0009)。純粋関数だが、`OffsetDateTime`(`time`crate)に依存する
//! ためdomain層(CLAUDE.mdの方針でDB/フレームワーク非依存)ではなくweb層に置く。

use sqlx::types::time::{OffsetDateTime, UtcOffset};

/// 表示用タイムゾーン。decision 0009: 保存はUTC、**表示はJST**。
/// アプリ固定のオフセットであり、利用者ごとのタイムゾーン設定は原典にない。
const DISPLAY_OFFSET_HOURS: i8 = 9;

/// 作成日時を画面表示用の文字列へ整形する(純粋関数)。
///
/// 書式はdecision 0009が例示する `2026-07-19 14:30`(JST)に合わせた。同decisionは
/// 「表示値から順序が読める形式」「同日内の複数スレッドを区別できる粒度」を要求しており、
/// 分までの表示でこれを満たす。秒・ミリ秒・オフセットまで出す`OffsetDateTime::to_string()`
/// (`2026-07-20 0:10:03.917 +00:00:00`)は利用者向けの表示としては粗いので使わない。
pub fn format_created_at(at: OffsetDateTime) -> String {
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

/// `OffsetDateTime` を`domain::query::ThreadSortFields`が要求する`i64`ミリ秒
/// エポック値へ変換する(F12)。domain層は`chrono`/`time`crateに依存しない方針
/// (`domain/query.rs`冒頭のdocコメント)なので、DB由来の`OffsetDateTime`を
/// 純粋関数`sort_thread_fields`に渡す前にここで変換する(`format_created_at`と
/// 同じ理由でweb層に置く)。
///
/// `unix_timestamp_nanos()`(`i128`)をナノ秒からミリ秒へ切り捨てる。
///
/// **精度低下は実際に起きる**。DBの列はマイクロ秒精度(PostgreSQLの`timestamptz`)
/// なので、この変換はマイクロ秒以下の3桁を捨てている。許容できる理由は
/// 「失われない」からではなく、**失われる精度がdecision 0009の要求粒度
/// (ミリ秒以上)を下回るから**。
///
/// 帰結として、`created_at`／`last_updated_at`が**1ミリ秒未満しか違わない**2件は
/// この変換で同値に潰れ、`sort_thread_fields`のタイブレーク(id昇順)に落ちる。
/// decision 0029の通り1トランザクション内の`now()`は同値になりうるため、これは
/// 到達可能な経路だが、id昇順への退避は決定的なので表示順が不定になることはない。
pub fn to_millis(at: OffsetDateTime) -> i64 {
    (at.unix_timestamp_nanos() / 1_000_000) as i64
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

    /// F12: UNIXエポックはちょうど0ミリ秒。
    #[test]
    fn to_millis_of_unix_epoch_is_zero() {
        assert_eq!(to_millis(OffsetDateTime::UNIX_EPOCH), 0);
    }

    /// F12: `sort_thread_fields`のタイブレークは秒未満の差も区別できる必要がある
    /// (decision 0009: タイムスタンプはミリ秒以上の精度)ので、ミリ秒未満切り捨てにより
    /// 順序関係(前後)が保たれることを確認する。
    #[test]
    fn to_millis_preserves_ordering_within_the_same_second() {
        let earlier = utc(2026, 200, 5, 30, 0);
        let later = Date::from_ordinal_date(2026, 200)
            .unwrap()
            .with_hms_milli(5, 30, 0, 500)
            .unwrap()
            .assume_utc();
        assert!(to_millis(earlier) < to_millis(later));
    }
}
