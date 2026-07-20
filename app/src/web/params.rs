//! `q` / `sort` / `page` の共通パース。GET /?q=Rust&sort=created_desc&page=2 の形
//! (decision 0011 §影響)。ページネーション遷移でソート・検索条件を維持する(C-13)。
//! `SortKey`自体はdomain/query.rs(Bbs.Query.SortKeyの対応先)が持ち、ここではHTTPの
//! クエリ文字列をその型へ変換するパースのみを担う。
//!
//! 呼び出し元はF09/F11(`web/thread_list.rs::show`)。

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

/// POST /profile/edit のフォーム(P06)。decision 0021によりCSRFトークンを必須で持つ。
/// issue 04: 編集可能なのは表示名のみ(ユニークID・パスワードの変更フィールドは無い)。
/// 表示名は機微情報ではないため`CreateThreadForm`と同様`#[derive(Debug)]`をそのまま使う。
#[derive(Debug, Deserialize)]
pub struct ProfileEditForm {
    pub display_name: String,
    pub csrf_token: String,
}

impl HasCsrfToken for ProfileEditForm {
    fn csrf_token(&self) -> &str {
        &self.csrf_token
    }
}

/// クエリ文字列の値として安全な形にパーセントエンコードする
/// (`&`・`=`・`%`・空白・非ASCIIなどをURL上意味を持たない`%XX`表現に変換)。
///
/// F11検索窓の`q`をC-13のページ送りリンク(`/?q=...&sort=...&page=...`)へ埋め込む際に使う。
/// テンプレート側の静的な`&`(パラメータ区切り)と違い、`q`自体に`&`・`=`が
/// 含まれうる(例: `A&B`というキーワード)ため、埋め込む前にエンコードしないと
/// URLのパラメータ境界が壊れる。専用crateを増やさず`encodeURIComponent`相当
/// (英数字と`-_.~`以外は全て`%XX`)の最小実装にする。
///
/// バイト単位でエンコードする(コードポイント単位ではない) ―― UTF-8の複数バイト文字は
/// バイトごとに`%XX`化されるのがURLパーセントエンコーディングの仕様どおりの挙動。
pub fn encode_query_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// decision 0033: 検索語`q`の上限（コードポイント数、decision 0003と同じ数え方）。
/// `LIKE`の全走査コストと、C-13のページ送りリンクへの重複展開(パーセントエンコードで
/// 最大3倍長)が入力長に比例して膨張するのを防ぐ。シナリオが使う長さ(数文字〜十数文字)を
/// 大きく超える参考値であり、実測に基づく値ではない(decision 0033 §提案)。
pub const MAX_QUERY_LEN: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListParams {
    /// トリム済み・`MAX_QUERY_LEN`以内に切り詰め済み(decision 0033)。
    /// 空文字列は「全件表示」(decision 0011: containsSubstr s "" = true)。
    pub q: String,
    /// `q`が`MAX_QUERY_LEN`を超えていたため切り詰めたかどうか(decision 0033)。
    /// 呼び出し側(`web/thread_list.rs`)がこれを見て、切り詰めが起きた旨を
    /// 画面上に明示する(黙って切り詰めない、ui-ux-guidelines §1)。
    pub q_truncated: bool,
    pub sort: SortKey,
    /// 1始まり。不正な値(0以下・非数値)は1ページ目に丸める(decision 0013)。
    pub page: u32,
}

impl ListParams {
    pub fn parse(raw: &HashMap<String, String>) -> Self {
        let raw_q = raw.get("q").cloned().unwrap_or_default();
        // decision 0033: 本文・コメント本文は保存時にトリムされる(decision 0004)ため、
        // 検索語もトリムしないと末尾空白付きの入力が原理的に一致しなくなる。
        // `domain::validation::trim`(decision 0004で確立済みの実装、全角スペース・
        // NBSPを含め正しく判定する)をそのまま再利用する。
        let trimmed = crate::domain::validation::trim(&raw_q);
        let (q, q_truncated) = if trimmed.chars().count() > MAX_QUERY_LEN {
            (trimmed.chars().take(MAX_QUERY_LEN).collect(), true)
        } else {
            (trimmed, false)
        };
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
        Self {
            q,
            q_truncated,
            sort,
            page,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C-13: `&`・`=`はページ送りリンクのパラメータ境界を壊すので必ずエンコードされる。
    #[test]
    fn encode_query_component_escapes_ampersand_and_equals() {
        assert_eq!(encode_query_component("A&B=C"), "A%26B%3DC");
    }

    /// 空白・`#`もURL上意味を持つ・持ちうる文字なのでエンコードされる。
    #[test]
    fn encode_query_component_escapes_space_and_hash() {
        assert_eq!(encode_query_component("A B#C"), "A%20B%23C");
    }

    /// 英数字と`-_.~`はエンコードしない(`encodeURIComponent`と同じ保守的な非予約文字集合)。
    #[test]
    fn encode_query_component_keeps_unreserved_characters_as_is() {
        assert_eq!(encode_query_component("Rust-1_2.3~4"), "Rust-1_2.3~4");
    }

    /// 非ASCII(日本語)はUTF-8バイト列単位で`%XX`化される。
    #[test]
    fn encode_query_component_encodes_non_ascii_by_utf8_bytes() {
        assert_eq!(encode_query_component("検索"), "%E6%A4%9C%E7%B4%A2");
    }

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

    /// decision 0033: 保存された本文はトリム済み(decision 0004)なので、検索語も
    /// 同じくトリムしないと末尾空白付きの入力が原理的に一致しなくなる。
    #[test]
    fn q_is_trimmed_like_body_and_display_name() {
        let p = ListParams::parse(&params(&[("q", "  Rust  ")]));
        assert_eq!(p.q, "Rust");
        assert!(!p.q_truncated);
    }

    /// decision 0033: 全角スペースのみの検索語もトリムすると空文字列になり、
    /// 空クエリ(decision 0011: 全件表示)と同じ扱いになる。
    #[test]
    fn q_of_only_fullwidth_space_trims_to_empty() {
        let p = ListParams::parse(&params(&[("q", "　　")]));
        assert_eq!(p.q, "");
        assert!(!p.q_truncated);
    }

    /// decision 0033: トリム後ちょうど`MAX_QUERY_LEN`は切り詰めの対象ではない。
    #[test]
    fn q_at_max_len_is_not_truncated() {
        let q = "あ".repeat(MAX_QUERY_LEN);
        let p = ListParams::parse(&params(&[("q", &q)]));
        assert_eq!(p.q.chars().count(), MAX_QUERY_LEN);
        assert!(!p.q_truncated);
    }

    /// decision 0033: `MAX_QUERY_LEN`を超える検索語はトリム後の先頭
    /// `MAX_QUERY_LEN`文字に切り詰められ、`q_truncated`が立つ
    /// (黙って切り詰めない・画面上で観測可能にする、ui-ux-guidelines §1)。
    #[test]
    fn q_beyond_max_len_is_truncated_and_flagged() {
        let q = "あ".repeat(MAX_QUERY_LEN + 1);
        let p = ListParams::parse(&params(&[("q", &q)]));
        assert_eq!(p.q.chars().count(), MAX_QUERY_LEN);
        assert_eq!(p.q, "あ".repeat(MAX_QUERY_LEN));
        assert!(p.q_truncated);
    }

    /// decision 0033: トリムしてから長さ判定する。トリム前は超過していても
    /// トリム後に収まるなら切り詰めない(コードポイント数はトリム後に数える、
    /// decision 0004/0003と同じ規約)。
    #[test]
    fn q_trim_happens_before_length_check() {
        let padded_with_spaces = format!("  {}  ", "あ".repeat(MAX_QUERY_LEN));
        let p = ListParams::parse(&params(&[("q", &padded_with_spaces)]));
        assert_eq!(p.q.chars().count(), MAX_QUERY_LEN);
        assert!(!p.q_truncated);
    }
}
