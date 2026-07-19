//! formal/Bbs/Query.lean の対応先(読み取り専用ロジック: 検索・ソート・ページネーション)。
//! `Db`全体を持つLeanモデルと異なり、ここでは「既に取得済みの行」に対する
//! 純粋な計算のみを置く。DB問い合わせ自体は`db/`層(未実装、機能実装フェーズ)の責務。
//!
//! 時刻はLeanの`Time := Nat`(単調な論理時刻の抽象)に倣い、`i64`のミリ秒エポック値
//! として扱う。domain層に`chrono`/`time`crateへの依存を持ち込まないための選択。
//!
//! `db/threads.rs`等の呼び出し元(F09〜F13のハンドラ)はfoundation-plan.md §5の
//! 範囲外(機能実装フェーズ)のため、それまでの間 `dead_code` を抑止する。
//! `SortKey`はweb/params.rsが既に使用している。

#![allow(dead_code)]

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
