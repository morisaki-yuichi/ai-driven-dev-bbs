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

/// POST /logout のフォーム(F03)。ボタン以外に入力欄は無いが、decision 0021の
/// 決定1(「例外なしの全POST」)によりCSRFトークンは必須で持つ
/// (`templates/layout.html`のログアウトフォームのhidden input)。
#[derive(Debug, Deserialize)]
pub struct LogoutForm {
    pub csrf_token: String,
}

impl HasCsrfToken for LogoutForm {
    fn csrf_token(&self) -> &str {
        &self.csrf_token
    }
}

/// POST /threads/new のフォーム(P05)。decision 0021によりCSRFトークンを必須で持つ。
/// タイトル・本文は機微情報ではないため`RegisterForm`/`LoginForm`と異なり
/// `#[derive(Debug)]`をそのまま使う(`LogoutForm`と同じ扱い)。
#[derive(Debug, Deserialize)]
pub struct CreateThreadForm {
    pub title: String,
    pub body: String,
    pub csrf_token: String,
}

impl HasCsrfToken for CreateThreadForm {
    fn csrf_token(&self) -> &str {
        &self.csrf_token
    }
}

/// POST /threads/{id}/comments のフォーム(P04)。decision 0021によりCSRFトークンを
/// 必須で持つ。本文は機微情報ではないため`CreateThreadForm`と同様
/// `#[derive(Debug)]`をそのまま使う。
#[derive(Debug, Deserialize)]
pub struct CreateCommentForm {
    pub body: String,
    pub csrf_token: String,
}

impl HasCsrfToken for CreateCommentForm {
    fn csrf_token(&self) -> &str {
        &self.csrf_token
    }
}

/// POST /threads/{thread_id}/comments/{comment_id}/delete のフォーム(F08)。
/// D18(削除確認ダイアログ): 確認なしで即削除するため、ボタン以外に入力欄は無い。
/// decision 0021によりCSRFトークンは必須で持つ(`LogoutForm`と同じ形)。
#[derive(Debug, Deserialize)]
pub struct DeleteCommentForm {
    pub csrf_token: String,
}

impl HasCsrfToken for DeleteCommentForm {
    fn csrf_token(&self) -> &str {
        &self.csrf_token
    }
}

/// POST /threads/{id}/delete のフォーム(F06)。`DeleteCommentForm`と同じ理由
/// (decision 0030と同型の裁定。スレッド削除は確認ダイアログを設けない、
/// `web/thread_detail.rs::delete_thread`のdocコメント参照)でボタン以外に
/// 入力欄は無い。decision 0021によりCSRFトークンは必須で持つ。
#[derive(Debug, Deserialize)]
pub struct DeleteThreadForm {
    pub csrf_token: String,
}

impl HasCsrfToken for DeleteThreadForm {
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
        // Why-not: `parse::<i64>()`で受けてから`as u32`で落とさない。`as`は
        // 黙って下位32bitに切り詰めるため、`?page=4294967296`(2^32)が`0`に化けて
        // `n >= 1`の検査を通り抜け、この構造体の「1始まり」という不変条件を破る
        // (`page = 0`のまま`paginate`へ渡る)。`u32`で直接パースすれば、u32に
        // 収まらない入力は`None`になり、他の不正値と同じく1ページ目へ丸まる。
        let page = raw
            .get("page")
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|&n| n >= 1)
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

    /// u32の上限ちょうどは有効な値としてそのまま通す(丸めの対象ではない)。
    /// `web/thread_list.rs`の`next_page`が`saturating_add`である必要はここに由来する。
    #[test]
    fn page_at_u32_max_is_kept_as_is() {
        let p = ListParams::parse(&params(&[("page", "4294967295")]));
        assert_eq!(p.page, u32::MAX);
    }

    /// 回帰: u32に収まらない値は「不正値」として1ページ目に丸まる。
    /// `i64`で受けてから`as u32`していた頃は下位32bitに切り詰められ、
    /// 2^32が`page = 0`(1始まりの不変条件違反)、2^32+1が黙って1ページ目になっていた。
    #[test]
    fn page_beyond_u32_range_clamps_to_1() {
        for page in ["4294967296", "4294967297", "99999999999999999999"] {
            let p = ListParams::parse(&params(&[("page", page)]));
            assert_eq!(p.page, 1, "page={page} should clamp to 1");
        }
    }
}
