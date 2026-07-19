//! decision 0021: CSRF対策(二重送信トークン + 同一オリジン検証)の純粋部分。
//! 副作用(Cookie読み書き・トークン生成・応答生成)は `web/csrf.rs` に隔離する。
//!
//! - `tokens_match`: フォーム値とCookie値の定数時間比較。空文字列は常に不一致。
//! - `is_same_origin`: `Origin`(無ければ`Referer`)のオリジン部分と`Host`の突き合わせ。
//!   期待値を固定文字列で持たず、リクエスト自身の`Host`と比較する(環境非依存、H-13)。

/// フォーム値とCookie値を比較する。タイミング攻撃を避けるため、一致・不一致に
/// かかわらず全バイトを走査する(decision 0021: 定数時間比較)。
/// 空文字列は(Cookie側・フォーム側のいずれであっても)常に不一致として扱う
/// (未発行のCookie・未入力のフォームを「たまたま等しい」と誤判定しないため)。
pub fn tokens_match(cookie_token: &str, form_token: &str) -> bool {
    if cookie_token.is_empty() || form_token.is_empty() {
        return false;
    }
    let a = cookie_token.as_bytes();
    let b = form_token.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// `scheme://host[:port][/...]` 形式の文字列から `host[:port]` 部分(authority)を取り出す。
/// スキーム区切り `://` が無い場合(例: `Origin: null`)は `None`。
fn extract_authority(url: &str) -> Option<&str> {
    let after_scheme = url.split_once("://")?.1;
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        None
    } else {
        Some(authority)
    }
}

/// `Origin`ヘッダを優先し、無ければ`Referer`のオリジン部分で代替する。
/// どちらも無い、またはいずれも`host`(例: `localhost:3000`)と一致しなければ拒否する。
/// 大文字小文字は無視する(ホスト名の大小は意味を持たない)。
pub fn is_same_origin(origin: Option<&str>, referer: Option<&str>, host: &str) -> bool {
    let candidate = origin.or(referer);
    match candidate.and_then(extract_authority) {
        Some(authority) => authority.eq_ignore_ascii_case(host),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_match_identical_nonempty_tokens() {
        assert!(tokens_match("abc-123", "abc-123"));
    }

    #[test]
    fn tokens_match_rejects_different_tokens() {
        assert!(!tokens_match("abc-123", "abc-124"));
    }

    #[test]
    fn tokens_match_rejects_different_length() {
        assert!(!tokens_match("abc", "abcd"));
    }

    #[test]
    fn tokens_match_empty_cookie_never_matches() {
        // 未発行のCookie(空文字列)は、フォーム側が偶然空でも一致させない。
        assert!(!tokens_match("", ""));
        assert!(!tokens_match("", "abc"));
    }

    #[test]
    fn tokens_match_empty_form_never_matches() {
        assert!(!tokens_match("abc", ""));
    }

    #[test]
    fn same_origin_matches_origin_header_case_insensitive() {
        assert!(is_same_origin(
            Some("http://LocalHost:3000"),
            None,
            "localhost:3000"
        ));
    }

    #[test]
    fn same_origin_prefers_origin_over_referer() {
        assert!(is_same_origin(
            Some("http://localhost:3000"),
            Some("http://evil.example/x"),
            "localhost:3000"
        ));
    }

    #[test]
    fn same_origin_falls_back_to_referer_when_origin_missing() {
        assert!(is_same_origin(
            None,
            Some("http://localhost:3000/register"),
            "localhost:3000"
        ));
    }

    #[test]
    fn same_origin_rejects_when_both_headers_missing() {
        assert!(!is_same_origin(None, None, "localhost:3000"));
    }

    #[test]
    fn same_origin_rejects_different_port() {
        assert!(!is_same_origin(
            Some("http://localhost:4000"),
            None,
            "localhost:3000"
        ));
    }

    #[test]
    fn same_origin_rejects_different_scheme_treated_as_different_authority_string() {
        // schemeはauthorityに含まれないため一致するはずのケース(スキームは見ない)。
        assert!(is_same_origin(
            Some("https://localhost:3000"),
            None,
            "localhost:3000"
        ));
    }

    #[test]
    fn same_origin_rejects_malformed_origin_without_scheme_separator() {
        // ブラウザがサンドボックス化されたオリジンで送る `Origin: null` を想定。
        assert!(!is_same_origin(Some("null"), None, "localhost:3000"));
    }
}
