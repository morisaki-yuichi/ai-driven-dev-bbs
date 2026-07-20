//! formal/Bbs/Query.lean の対応先(読み取り専用ロジック: 検索・ソート・ページネーション)。
//! `Db`全体を持つLeanモデルと異なり、ここでは「既に取得済みの行」に対する
//! 純粋な計算のみを置く。DB問い合わせ自体は`db/`層(未実装、機能実装フェーズ)の責務。
//!
//! 時刻はLeanの`Time := Nat`(単調な論理時刻の抽象)に倣い、`i64`のミリ秒エポック値
//! として扱う。domain層に`chrono`/`time`crateへの依存を持ち込まないための選択。
//!
//! `SortKey`・`contains_substr`・`escape_like_pattern`・`Hit`・`hit_location`・
//! `render_comment_body`・`paginate`はF09/F11(`db/threads.rs`・`web/thread_list.rs`・
//! `web/thread_detail.rs`)が使用している。`ThreadSortFields`/`sort_thread_fields`は
//! F09/F11の時点ではF12向けの先行実装で未使用だったが、**F12(ソートUI)の実装で
//! 実際に使われるようになった** ―― `web/thread_list.rs`が`db::threads::search`の
//! 結果を`ThreadSortFields`へ写し、この`sort_thread_fields`で整列してから描画する。
//! 一覧の表示順を決めているのはSQLの`order by`ではなくこの純粋関数のほう
//! (`db/threads.rs`冒頭のdocコメント参照)。

use crate::domain::model::DELETED_COMMENT_TEXT;

/// formal/Bbs/Query.lean の `SortKey` に1対1対応する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    CreatedAsc,
    CreatedDesc,
    CommentCountDesc,
    LastUpdatedDesc,
}

impl SortKey {
    /// クエリ文字列上の表現。ページネーションリンクの再構築(C-13)に使う。
    pub fn as_query_value(self) -> &'static str {
        match self {
            SortKey::CreatedAsc => "created_asc",
            SortKey::CreatedDesc => "created_desc",
            SortKey::CommentCountDesc => "comment_count_desc",
            SortKey::LastUpdatedDesc => "last_updated_desc",
        }
    }

    /// 不正・未指定な値は既定(作成日時降順、decision 0009: 一覧の初期表示)に丸める。
    pub fn parse(s: Option<&str>) -> Self {
        match s {
            Some("created_asc") => SortKey::CreatedAsc,
            Some("comment_count_desc") => SortKey::CommentCountDesc,
            Some("last_updated_desc") => SortKey::LastUpdatedDesc,
            _ => SortKey::CreatedDesc,
        }
    }
}

/// D07/decision 0011: 素朴な部分文字列一致。大文字小文字・全角半角の正規化はしない。
/// 空クエリ(`needle == ""`)は常に真(全件表示)。
pub fn contains_substr(haystack: &str, needle: &str) -> bool {
    haystack.contains(needle)
}

/// decision 0032: `LIKE`のワイルドカード文字(`%`・`_`)、およびエスケープ文字自身
/// (`\`)をエスケープし、`db::threads::search`が投げる`LIKE`パターンの中で
/// ユーザー入力を**リテラルな部分文字列**として扱えるようにする。
///
/// **エスケープしないとどうなるか**: バインドパラメータは値の型注入(SQLインジェクション)
/// を防ぐが、`LIKE`パターン内の`%`・`_`の**意味**までは中和しない。ユーザーが
/// `50%`や`a_b`を検索すると、エスケープなしでは`%`が「任意の0文字以上」・`_`が
/// 「任意の1文字」として解釈され、`contains_substr`(素朴な部分文字列判定、
/// decision 0011)が返す結果と食い違う。この関数を通すことで、`format!("%{}%", ...)`
/// で組み立てた最終パターンが`contains_substr(haystack, needle)`と同じ判定に一致する
/// (Leanの`containsSubstr`をオラクルとして揃える)。
///
/// **処理順序が本質**: `\`を最初に(単独で)処理しないと、後段で`%`→`\%`が生成した
/// `\`を再度エスケープしてしまい二重エスケープになる。1回のループで文字ごとに
/// 判定することで、生成した`\`を再走査しない。
pub fn escape_like_pattern(needle: &str) -> String {
    let mut escaped = String::with_capacity(needle.len());
    for ch in needle.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

/// AC11-3(D19)のスクロール先決定。`formal/Bbs/Query.lean`の`Hit`に対応する。
/// タイトルは検索対象外(decision 0012)なので、本文・コメントの2択で足りる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Body,
    /// コメントのID。フラグメント識別子(`#comment-{id}`)の組み立てに使う
    /// (呼び出し元のweb層、decision 0008のJSなしスクロール連携)。
    Comment(i64),
}

/// `formal/Bbs/Query.lean`の`hitIn`に対応する純粋関数。**本文優先**
/// (`t.body`が一致すれば無条件に`Hit::Body`、コメント側の場合分けを見ない)。
///
/// `first_matching_comment_id`は、DB層(`db::threads::search`)が
/// 「未削除コメントに限定して(decision 0012)`LIKE`一致するものの中で最古(作成日時昇順、
/// idタイブレーク)の1件」を既に絞り込んだ結果を渡す想定 ―― `Query.lean`の
/// `searchableComments (commentsOf db t.id) |>.find? (...)`と同じ「最初の1件」を、
/// SQL側の`order by created_at asc, id asc limit 1`で再現する。
///
/// 本文もコメントも一致しない`None`は本来`db::threads::search`のWHERE句が
/// 除外するため到達しない想定だが、SQLとこの関数の対応をここで検査するのではなく
/// `Option`を返すことで、呼び出し側が不整合時にパニックしない形にしてある。
pub fn hit_location(body: &str, kw: &str, first_matching_comment_id: Option<i64>) -> Option<Hit> {
    if contains_substr(body, kw) {
        Some(Hit::Body)
    } else {
        first_matching_comment_id.map(Hit::Comment)
    }
}

/// AC08-2/AC10-3: 削除済みコメントの本文は固定文言に差し替える。
/// 作成者・作成日時は維持する(呼び出し側の責務、ここでは本文のみを扱う)。
pub fn render_comment_body(body: &str, deleted: bool) -> &str {
    if deleted { DELETED_COMMENT_TEXT } else { body }
}

/// C-12/decision 0013: 1ページ10件。
pub const PAGE_SIZE: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// 1始まり。
    pub page_number: u32,
    pub has_prev: bool,
    pub has_next: bool,
}

/// decision 0013: 範囲外のページ番号は空リストを返す(404にしない)。
/// `page == 0`は1ページ目として扱う(HTTPパース層での丸めと二重に安全側へ倒す)。
pub fn paginate<T>(page: u32, items: Vec<T>) -> Page<T> {
    let p = if page == 0 { 1 } else { page };
    let skip = (p as usize - 1) * PAGE_SIZE;
    let remaining: Vec<T> = items.into_iter().skip(skip).collect();
    let has_next = remaining.len() > PAGE_SIZE;
    let page_items: Vec<T> = remaining.into_iter().take(PAGE_SIZE).collect();
    Page {
        items: page_items,
        page_number: p,
        has_prev: p > 1,
        has_next,
    }
}

/// ソートに必要な列のみを持つ行。実際のDB行(db/threads.rs、機能実装フェーズで追加)
/// はこれに変換してから`sort_threads`に渡す想定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadSortFields {
    pub id: i64,
    pub created_at_millis: i64,
    pub comment_count: i64,
    pub last_updated_at_millis: i64,
}

/// formal/Bbs/Query.lean の `leOf` に対応する安定ソート。
/// **同値キーのタイブレークはidの昇順**(decision 0009。全ソートキーに第2キーidを持たせ全順序にする)。
pub fn sort_thread_fields(items: &mut [ThreadSortFields], key: SortKey) {
    items.sort_by(|a, b| match key {
        SortKey::CreatedAsc => a
            .created_at_millis
            .cmp(&b.created_at_millis)
            .then(a.id.cmp(&b.id)),
        SortKey::CreatedDesc => b
            .created_at_millis
            .cmp(&a.created_at_millis)
            .then(a.id.cmp(&b.id)),
        SortKey::CommentCountDesc => b.comment_count.cmp(&a.comment_count).then(a.id.cmp(&b.id)),
        SortKey::LastUpdatedDesc => b
            .last_updated_at_millis
            .cmp(&a.last_updated_at_millis)
            .then(a.id.cmp(&b.id)),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_substr_is_case_sensitive() {
        let body = "プログラミング言語Rustの特徴";
        assert!(contains_substr(body, "Rust"));
        assert!(!contains_substr(body, "rust"));
    }

    #[test]
    fn contains_substr_does_not_normalize_fullwidth() {
        let body = "プログラミング言語Rustの特徴";
        assert!(!contains_substr(body, "Ｒｕｓｔ"));
    }

    #[test]
    fn empty_needle_matches_everything() {
        assert!(contains_substr("何でもいい本文", ""));
        assert!(contains_substr("", ""));
    }

    #[test]
    fn render_comment_body_replaces_only_when_deleted() {
        assert_eq!(render_comment_body("元の本文", false), "元の本文");
        assert_eq!(
            render_comment_body("元の本文", true),
            "＜このコメントは削除されました＞"
        );
    }

    /// decision 0032: `%`・`_`はバックスラッシュでエスケープされる。
    #[test]
    fn escape_like_pattern_escapes_percent_and_underscore() {
        assert_eq!(escape_like_pattern("50%"), "50\\%");
        assert_eq!(escape_like_pattern("a_b"), "a\\_b");
    }

    /// decision 0032: エスケープ文字自身(`\`)も二重にエスケープする
    /// (先に`\`を処理しないと、後段の`%`→`\%`が生成した`\`を再エスケープしてしまう)。
    #[test]
    fn escape_like_pattern_escapes_the_escape_character_itself() {
        assert_eq!(escape_like_pattern("a\\b"), "a\\\\b");
        assert_eq!(escape_like_pattern("50\\%"), "50\\\\\\%");
    }

    /// 通常の文字列はそのまま(ワイルドカードを含まない入力での回帰崩れが無いこと)。
    #[test]
    fn escape_like_pattern_is_identity_without_wildcards() {
        assert_eq!(escape_like_pattern("Rust"), "Rust");
        assert_eq!(escape_like_pattern(""), "");
        assert_eq!(escape_like_pattern("プログラミング"), "プログラミング");
    }

    /// AC11-2: 本文が一致すれば、コメント側の候補が来ていても本文優先(`hitIn`と同型)。
    #[test]
    fn hit_location_prioritizes_body_over_comment() {
        let body = "プログラミング言語Rustの特徴";
        assert_eq!(hit_location(body, "Rust", Some(42)), Some(Hit::Body));
        assert_eq!(hit_location(body, "Rust", None), Some(Hit::Body));
    }

    /// AC11-3: 本文が一致しなければ、DB側が絞り込んだ最初の一致コメントを指す。
    #[test]
    fn hit_location_falls_back_to_first_matching_comment() {
        let body = "本文には含まれない";
        assert_eq!(hit_location(body, "Rust", Some(7)), Some(Hit::Comment(7)));
    }

    /// 本文にもコメント候補にも一致しない(WHERE句の想定外)場合はパニックせず`None`。
    #[test]
    fn hit_location_is_none_when_neither_matches() {
        assert_eq!(hit_location("本文には含まれない", "Rust", None), None);
    }

    fn ids(page: &Page<i32>) -> Vec<i32> {
        page.items.clone()
    }

    #[test]
    fn paginate_first_page_has_no_prev() {
        let items: Vec<i32> = (1..=25).collect();
        let page = paginate(1, items);
        assert_eq!(ids(&page), (1..=10).collect::<Vec<_>>());
        assert!(!page.has_prev);
        assert!(page.has_next);
    }

    #[test]
    fn paginate_last_page_has_no_next() {
        let items: Vec<i32> = (1..=25).collect();
        let page = paginate(3, items);
        assert_eq!(ids(&page), (21..=25).collect::<Vec<_>>());
        assert!(page.has_prev);
        assert!(!page.has_next);
    }

    #[test]
    fn paginate_out_of_range_page_is_empty_not_404() {
        let items: Vec<i32> = (1..=5).collect();
        let page = paginate(999, items);
        assert!(page.items.is_empty());
        assert!(page.has_prev);
        assert!(!page.has_next);
    }

    #[test]
    fn paginate_zero_items_is_page_1_with_no_prev_or_next() {
        let page: Page<i32> = paginate(1, vec![]);
        assert_eq!(page.page_number, 1);
        assert!(!page.has_prev);
        assert!(!page.has_next);
    }

    #[test]
    fn paginate_page_zero_is_treated_as_page_1() {
        let items: Vec<i32> = (1..=5).collect();
        let page = paginate(0, items);
        assert_eq!(page.page_number, 1);
        assert!(!page.has_prev);
    }

    fn field(id: i64, created: i64, comments: i64, updated: i64) -> ThreadSortFields {
        ThreadSortFields {
            id,
            created_at_millis: created,
            comment_count: comments,
            last_updated_at_millis: updated,
        }
    }

    #[test]
    fn sort_created_desc_breaks_ties_by_id_ascending() {
        let mut items = vec![field(2, 100, 0, 100), field(1, 100, 0, 100)];
        sort_thread_fields(&mut items, SortKey::CreatedDesc);
        assert_eq!(items.iter().map(|f| f.id).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn sort_created_asc_orders_by_time_ascending() {
        let mut items = vec![field(1, 200, 0, 200), field(2, 100, 0, 100)];
        sort_thread_fields(&mut items, SortKey::CreatedAsc);
        assert_eq!(items.iter().map(|f| f.id).collect::<Vec<_>>(), vec![2, 1]);
    }

    #[test]
    fn sort_comment_count_desc_breaks_ties_by_id() {
        let mut items = vec![
            field(3, 100, 5, 100),
            field(1, 200, 5, 200),
            field(2, 50, 9, 50),
        ];
        sort_thread_fields(&mut items, SortKey::CommentCountDesc);
        assert_eq!(
            items.iter().map(|f| f.id).collect::<Vec<_>>(),
            vec![2, 1, 3]
        );
    }

    #[test]
    fn sort_last_updated_desc_orders_by_last_updated() {
        let mut items = vec![field(1, 0, 0, 50), field(2, 0, 0, 200)];
        sort_thread_fields(&mut items, SortKey::LastUpdatedDesc);
        assert_eq!(items.iter().map(|f| f.id).collect::<Vec<_>>(), vec![2, 1]);
    }
}
