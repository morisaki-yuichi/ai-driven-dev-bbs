//! `q` / `sort` / `page` の共通パース。GET /?q=Rust&sort=created_desc&page=2 の形
//! (decision 0011 §影響)。ページネーション遷移でソート・検索条件を維持する(C-13)。
//! `SortKey`自体はdomain/query.rs(Bbs.Query.SortKeyの対応先)が持ち、ここではHTTPの
//! クエリ文字列をその型へ変換するパースのみを担う。
//!
//! 呼び出し元(F09/F11/F12のスレッド一覧ハンドラ)はfoundation-plan.md §5の範囲外
//! (機能実装フェーズ)のため、それまでの間 `dead_code` を抑止する。

#![allow(dead_code)]

use std::collections::HashMap;

use crate::domain::query::SortKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListParams {
    /// 空文字列は「全件表示」(decision 0011: containsSubstr s "" = true)。
    pub q: String,
    pub sort: SortKey,
    /// 1始まり。不正な値(0以下・非数値)は1ページ目に丸める(decision 0013)。
    pub page: u32,
}

impl ListParams {
    pub fn parse(raw: &HashMap<String, String>) -> Self {
        let q = raw.get("q").cloned().unwrap_or_default();
        let sort = SortKey::parse(raw.get("sort").map(String::as_str));
        let page = raw
            .get("page")
            .and_then(|s| s.parse::<i64>().ok())
            .filter(|&n| n >= 1)
            .map(|n| n as u32)
            .unwrap_or(1);
        Self { q, sort, page }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn empty_query_defaults_to_all_created_desc_page_1() {
        let p = ListParams::parse(&params(&[]));
        assert_eq!(p.q, "");
        assert_eq!(p.sort, SortKey::CreatedDesc);
        assert_eq!(p.page, 1);
    }

    #[test]
    fn parses_all_fields() {
        let p = ListParams::parse(&params(&[
            ("q", "Rust"),
            ("sort", "comment_count_desc"),
            ("page", "3"),
        ]));
        assert_eq!(p.q, "Rust");
        assert_eq!(p.sort, SortKey::CommentCountDesc);
        assert_eq!(p.page, 3);
    }

    #[test]
    fn unrecognized_sort_falls_back_to_created_desc() {
        let p = ListParams::parse(&params(&[("sort", "nonsense")]));
        assert_eq!(p.sort, SortKey::CreatedDesc);
    }

    #[test]
    fn page_zero_negative_or_non_numeric_clamps_to_1() {
        for page in ["0", "-1", "abc"] {
            let p = ListParams::parse(&params(&[("page", page)]));
            assert_eq!(p.page, 1, "page={page} should clamp to 1");
        }
    }
}
