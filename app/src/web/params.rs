//! `q` / `sort` / `page` の共通パース。GET /?q=Rust&sort=created_desc&page=2 の形
//! (decision 0011 §影響)。ページネーション遷移でソート・検索条件を維持する(C-13)。
//! `SortKey`自体はdomain/query.rs(Bbs.Query.SortKeyの対応先)が持ち、ここではHTTPの
//! クエリ文字列をその型へ変換するパースのみを担う。
//!
//! 呼び出し元(F09/F11/F12のスレッド一覧ハンドラ)はfoundation-plan.md §5の範囲外
//! (機能実装フェーズ)のため、それまでの間 `dead_code` を抑止する。

#![allow(dead_code)]

use std::collections::HashMap;

use serde::Deserialize;

use crate::domain::query::SortKey;
use crate::web::csrf::HasCsrfToken;

/// POST /register のフォーム(P02)。decision 0021によりCSRFトークンを必須で持つ。
#[derive(Deserialize)]
pub struct RegisterForm {
    pub unique_id: String,
    pub password: String,
    pub display_name: String,
    pub csrf_token: String,
}

/// Why-not: `#[derive(Debug)]` にしない。この構造体はハンドラのエラー経路や
/// `tracing` のイベントに載りうるため、derive のままだと平文パスワードが
/// ログ・パニックメッセージへそのまま出る。パスワードとCSRFトークンは伏字にする。
impl std::fmt::Debug for RegisterForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisterForm")
            .field("unique_id", &self.unique_id)
            .field("password", &"[redacted]")
            .field("display_name", &self.display_name)
            .field("csrf_token", &"[redacted]")
            .finish()
    }
}

impl HasCsrfToken for RegisterForm {
    fn csrf_token(&self) -> &str {
        &self.csrf_token
    }
}

/// POST /login のフォーム(P01)。decision 0021によりCSRFトークンを必須で持つ。
#[derive(Deserialize)]
pub struct LoginForm {
    pub unique_id: String,
    pub password: String,
    pub csrf_token: String,
}

/// Why-not: `RegisterForm`と同じ理由(このファイル冒頭のコメント参照)で
/// `#[derive(Debug)]`にしない。
impl std::fmt::Debug for LoginForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginForm")
            .field("unique_id", &self.unique_id)
            .field("password", &"[redacted]")
            .field("csrf_token", &"[redacted]")
            .finish()
    }
}

impl HasCsrfToken for LoginForm {
    fn csrf_token(&self) -> &str {
        &self.csrf_token
    }
}

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
    fn register_form_debug_does_not_leak_password() {
        let form = RegisterForm {
            unique_id: "testuser_01".to_string(),
            password: "TestPassword123!".to_string(),
            display_name: "テストユーザー01".to_string(),
            csrf_token: "0f3d-secret".to_string(),
        };
        let rendered = format!("{form:?}");
        assert!(!rendered.contains("TestPassword123!"));
        assert!(!rendered.contains("0f3d-secret"));
        // 伏字にしない項目は追跡できるよう残す。
        assert!(rendered.contains("testuser_01"));
    }

    #[test]
    fn login_form_debug_does_not_leak_password() {
        let form = LoginForm {
            unique_id: "testuser_01".to_string(),
            password: "TestPassword123!".to_string(),
            csrf_token: "0f3d-secret".to_string(),
        };
        let rendered = format!("{form:?}");
        assert!(!rendered.contains("TestPassword123!"));
        assert!(!rendered.contains("0f3d-secret"));
        assert!(rendered.contains("testuser_01"));
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
