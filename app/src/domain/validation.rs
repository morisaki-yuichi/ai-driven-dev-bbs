//! formal/Bbs/Validation.lean の対応先。C-02(パスワード強度)/C-03(表示名)/
//! C-04(ユニークID)/AC05-2・AC07-2(空チェック)を純粋な述語として実装する。
//! `register_validation`はF01登録ハンドラが、`create_thread_validation`はF05
//! スレッド作成ハンドラ(web/thread_create.rs)が、`create_comment_validation`はF07
//! コメント作成ハンドラ(web/thread_detail.rs)が使う。
//!
//! F04プロフィール編集のハンドラは foundation-plan.md §5の範囲外(機能実装フェーズ)
//! のため、それまでの間 `dead_code` を抑止する。
//!
//! ### 文字数の数え方(decision 0003)
//! 「15文字以内」「12文字以上」はUnicodeコードポイント数で数える。バイト数は誤り。
//! Rustでは`str::len()`はバイト長を返すため、必ず`.chars().count()`を使う。
//!
//! ### 空白の扱い(decision 0004)
//! 全角スペース(U+3000)を空白とみなさない言語処理系があるため要注意(Leanの
//! `Char.isWhitespace`はASCII限定で該当する)。**Rustの`char::is_whitespace()`は
//! U+3000・U+00A0を含め正しく判定することを実測で確認済み**(`str::trim()`は
//! decision 0004の落とし穴テーブルにも「はい」と記載されている)。よって
//! Lean側のような独自`isSpaceChar`は不要で、標準の`str::trim()`をそのまま使う。

#![allow(dead_code)]

use crate::domain::model::{PasswordWeakness, ValidationFailure};

/// C-02: 許可された記号。issues/01の列挙をそのまま写したもの。
/// バックスラッシュ・空白・全角記号は含まれない。
const ALLOWED_SYMBOLS: &str = "!@#$%^&*()_+-=[]{}|;':\",./<>?";

fn is_symbol(c: char) -> bool {
    ALLOWED_SYMBOLS.contains(c)
}

/// 「英字」はASCIIのa-z/A-Zと解釈する。非ASCII文字(例: é、全角Ａ)は英字に
/// 数えない(原典は「英字」としか書いておらず、decision 0003の解釈)。
fn is_alpha(c: char) -> bool {
    c.is_ascii_alphabetic()
}

fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// 前後の空白を落とす(decision 0004)。
pub fn trim(s: &str) -> String {
    s.trim().to_string()
}

/// AC05-2/AC07-2の「空」＝トリム後に長さ0(decision 0004)。
pub fn is_blank(s: &str) -> bool {
    trim(s).is_empty()
}

/// C-02のパスワード強度違反を**すべて**列挙する。シナリオ01-1-5は`password`に
/// 対して複数観点のエラー提示を示唆するため、最初の1件で打ち切らない(decision 0006)。
/// 列挙順はformal/Bbs/Validation.leanの`passwordWeaknesses`と一致させる。
pub fn password_weaknesses(p: &str) -> Vec<PasswordWeakness> {
    let chars: Vec<char> = p.chars().collect();
    let mut out = Vec::new();

    if chars.len() < 12 {
        out.push(PasswordWeakness::TooShort);
    }
    if !chars.iter().any(|&c| is_alpha(c)) {
        out.push(PasswordWeakness::NoAlpha);
    }
    if !chars.iter().any(|&c| is_digit(c)) {
        out.push(PasswordWeakness::NoDigit);
    }
    if !chars.iter().any(|&c| is_symbol(c)) {
        out.push(PasswordWeakness::NoSymbol);
    }
    // 「許可された記号のみ」を、英数字と許可記号以外を一切禁じる意味に取る。
    // 空白・非ASCII文字(日本語等)もここで弾かれる(decision 0003)。
    if !chars
        .iter()
        .all(|&c| is_alpha(c) || is_digit(c) || is_symbol(c))
    {
        out.push(PasswordWeakness::DisallowedChar);
    }

    out
}

pub fn password_strong(p: &str) -> bool {
    password_weaknesses(p).is_empty()
}

/// C-03: 表示名は15コードポイント以内。空文字列は許さない(decision 0005:
/// 安全側に倒して「1文字以上15文字以内」とする)。長さはトリム後に数える(decision 0004)。
pub fn display_name_failure(n: &str) -> Option<ValidationFailure> {
    let t = trim(n);
    let len = t.chars().count();
    if len == 0 {
        Some(ValidationFailure::DisplayNameEmpty)
    } else if len > 15 {
        Some(ValidationFailure::DisplayNameTooLong)
    } else {
        None
    }
}

pub fn display_name_valid(n: &str) -> bool {
    display_name_failure(n).is_none()
}

/// C-04が要求するのは一意性のみで、文字種・長さ・大文字小文字の同一視は
/// 一切規定がない(decision 0003)。ここでは最小限「空でない」だけを形式条件とし、
/// それ以上の制限は置かない。一意性の判定はDBを見る必要があるためdb/層の責務。
///
/// 「空」の判定は`display_name`と同じくトリム後に行う(decision 0004)。
/// Why-not: `!u.is_empty()`だと空白のみのIDが通ってしまい、AC05-2/AC07-2で
/// 「空」と定義した基準と同じ画面上で扱いが食い違う。
pub fn unique_id_well_formed(u: &str) -> bool {
    !is_blank(u)
}

pub fn non_empty_text(s: &str) -> bool {
    !is_blank(s)
}

/// F01登録の項目間検査順序(decision 0006): 形式検査 → 強度検査 → 表示名検査の順で
/// 最初に失敗した項目のエラーのみを返す。ユニークID重複はDBを見る必要があるため
/// ここに含めない(Action層 = db/users.rsの責務、formal/Bbs/Op.lean `register` の
/// 後半に対応)。成功時はトリム済みの表示名を返す(decision 0004: 保存はトリム後の値)。
/// この検査列の場合分けは`formal/Bbs/Invariant.lean`の`register_atomic`が
/// オラクルとして参照した`register`の実装と一致させてある。
pub fn register_validation(
    unique_id: &str,
    password: &str,
    display_name: &str,
) -> Result<String, ValidationFailure> {
    if !unique_id_well_formed(unique_id) {
        return Err(ValidationFailure::UniqueIdInvalid);
    }
    let weaknesses = password_weaknesses(password);
    if !weaknesses.is_empty() {
        return Err(ValidationFailure::PasswordWeak(weaknesses));
    }
    if let Some(failure) = display_name_failure(display_name) {
        return Err(failure);
    }
    Ok(trim(display_name))
}

/// F05スレッド作成の項目間検査順序: タイトル→本文の順で、最初に失敗した項目の
/// エラーのみを返す(AC05-2)。この順序は`formal/Bbs/Op.lean`の`createThread`
/// (`ensure title` → `ensure body`)と一致させてあり、`createThread_atomic`/
/// `createThread_does_not_modify_existing_threads`がオラクルとして参照した実装。
/// 成功時はトリム済みの`(title, body)`を返す(decision 0004: 保存はトリム後の値)。
pub fn create_thread_validation(
    title: &str,
    body: &str,
) -> Result<(String, String), ValidationFailure> {
    if !non_empty_text(title) {
        return Err(ValidationFailure::TitleEmpty);
    }
    if !non_empty_text(body) {
        return Err(ValidationFailure::BodyEmpty);
    }
    Ok((trim(title), trim(body)))
}

/// F07コメント作成の検査: 本文の空チェックのみ(AC07-2)。`formal/Bbs/Op.lean`の
/// `createComment`(`ensure body`)と一致させ、`createComment_atomic`/
/// `createComment_does_not_modify_existing_comments`がオラクルとして参照した実装。
/// 成功時はトリム済みの本文を返す(decision 0004: 保存はトリム後の値)。
pub fn create_comment_validation(body: &str) -> Result<String, ValidationFailure> {
    if !non_empty_text(body) {
        return Err(ValidationFailure::BodyEmpty);
    }
    Ok(trim(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_removes_fullwidth_and_nbsp_space() {
        assert_eq!(trim("　　本文　　"), "本文");
        assert_eq!(trim("\u{00A0}本文\u{00A0}"), "本文");
    }

    #[test]
    fn is_blank_true_for_fullwidth_space_only() {
        assert!(is_blank("　　"));
        assert!(is_blank(""));
        assert!(is_blank("   "));
        assert!(!is_blank("　本文　"));
    }

    #[test]
    fn password_weaknesses_reports_all_violations_like_scenario_01_1_5() {
        // シナリオ01-1-5: "password" は短すぎる・数字なし・記号なし。
        let weaknesses = password_weaknesses("password");
        assert_eq!(
            weaknesses,
            vec![
                PasswordWeakness::TooShort,
                PasswordWeakness::NoDigit,
                PasswordWeakness::NoSymbol,
            ]
        );
    }

    #[test]
    fn password_weaknesses_rejects_japanese_characters() {
        // 12文字以上・英数字・記号を含んでいても、日本語文字が混ざると
        // disallowedChar になる(decision 0003: 許可記号のみ)。
        let weaknesses = password_weaknesses("Password123!あ");
        assert!(weaknesses.contains(&PasswordWeakness::DisallowedChar));
    }

    #[test]
    fn password_weaknesses_empty_for_strong_password() {
        assert!(password_weaknesses("Correct1Horse!").is_empty());
        assert!(password_strong("Correct1Horse!"));
    }

    #[test]
    fn display_name_empty_after_trim_is_rejected() {
        assert_eq!(
            display_name_failure("　　"),
            Some(ValidationFailure::DisplayNameEmpty)
        );
        assert!(!display_name_valid("　　"));
    }

    #[test]
    fn display_name_exactly_15_after_trim_is_valid() {
        let name = " あいうえおかきくけこさしすせそ ";
        assert_eq!(trim(name).chars().count(), 15);
        assert_eq!(display_name_failure(name), None);
    }

    #[test]
    fn display_name_16_after_trim_is_too_long() {
        let name = "あいうえおかきくけこさしすせそた";
        assert_eq!(
            display_name_failure(name),
            Some(ValidationFailure::DisplayNameTooLong)
        );
    }

    #[test]
    fn unique_id_well_formed_rejects_blank_like_display_name() {
        assert!(unique_id_well_formed("testuser_01"));
        assert!(!unique_id_well_formed(""));
        // 空白のみのIDは「空」と同じ扱いにする(decision 0004の空の定義を適用)。
        assert!(!unique_id_well_formed("   "));
        assert!(!unique_id_well_formed("　　"));
        assert!(!unique_id_well_formed("\u{00A0}"));
    }

    #[test]
    fn register_validation_rejects_blank_unique_id_before_checking_password() {
        let result = register_validation("　　", "password", "テストユーザー01");
        assert_eq!(result, Err(ValidationFailure::UniqueIdInvalid));
    }

    #[test]
    fn register_validation_succeeds_and_trims_display_name_like_scenario_01_1_6() {
        // シナリオ01-1-6: testuser_01 / TestPassword123! / テストユーザー01
        let result = register_validation("testuser_01", "TestPassword123!", " テストユーザー01 ");
        assert_eq!(result, Ok("テストユーザー01".to_string()));
    }

    #[test]
    fn register_validation_reports_password_weaknesses_like_scenario_01_1_5() {
        // register_atomicの証明が確定した順序: 形式検査(uniqueId)は通過し、
        // 次の強度検査で"password"の全違反がまとめて返る(display nameは未検査)。
        let result = register_validation("testuser_01", "password", "テストユーザー01");
        assert_eq!(
            result,
            Err(ValidationFailure::PasswordWeak(vec![
                PasswordWeakness::TooShort,
                PasswordWeakness::NoDigit,
                PasswordWeakness::NoSymbol,
            ]))
        );
    }

    #[test]
    fn register_validation_rejects_empty_unique_id_before_checking_password() {
        // 空IDと弱いパスワードが同時に不正でも、形式検査(ID)が先に失敗する
        // (decision 0006: 形式→強度→表示名の順で最初の1件のみ返す)。
        let result = register_validation("", "password", "テストユーザー01");
        assert_eq!(result, Err(ValidationFailure::UniqueIdInvalid));
    }

    #[test]
    fn register_validation_rejects_display_name_only_after_id_and_password_pass() {
        let too_long = "あいうえおかきくけこさしすせそた";
        let result = register_validation("testuser_01", "TestPassword123!", too_long);
        assert_eq!(result, Err(ValidationFailure::DisplayNameTooLong));
    }

    #[test]
    fn create_thread_validation_accepts_nonempty_title_and_body() {
        let result = create_thread_validation("AI駆動開発の未来について", "本文です");
        assert_eq!(
            result,
            Ok((
                "AI駆動開発の未来について".to_string(),
                "本文です".to_string()
            ))
        );
    }

    #[test]
    fn create_thread_validation_rejects_empty_title_before_checking_body() {
        // AC05-2: タイトル・本文どちらも空でも、タイトル検査が先に失敗する
        // (formal/Bbs/Op.leanのcreateThreadと同じ順序)。
        let result = create_thread_validation("", "");
        assert_eq!(result, Err(ValidationFailure::TitleEmpty));
    }

    #[test]
    fn create_thread_validation_rejects_blank_title_like_fullwidth_space_only() {
        // decision 0004: 全角スペースのみは「空」として扱う。
        let result = create_thread_validation("　　", "本文です");
        assert_eq!(result, Err(ValidationFailure::TitleEmpty));
    }

    #[test]
    fn create_thread_validation_rejects_empty_body_after_title_passes() {
        let result = create_thread_validation("タイトル", "");
        assert_eq!(result, Err(ValidationFailure::BodyEmpty));
    }

    #[test]
    fn create_thread_validation_trims_title_and_body() {
        // decision 0004: 保存はトリム後の値。
        let result = create_thread_validation(" タイトル ", " 本文 ");
        assert_eq!(result, Ok(("タイトル".to_string(), "本文".to_string())));
    }

    #[test]
    fn create_comment_validation_accepts_nonempty_body() {
        let result = create_comment_validation("コメント本文です");
        assert_eq!(result, Ok("コメント本文です".to_string()));
    }

    #[test]
    fn create_comment_validation_rejects_empty_body() {
        let result = create_comment_validation("");
        assert_eq!(result, Err(ValidationFailure::BodyEmpty));
    }

    #[test]
    fn create_comment_validation_rejects_blank_body_like_fullwidth_space_only() {
        // decision 0004: 全角スペースのみは「空」として扱う。
        let result = create_comment_validation("　　");
        assert_eq!(result, Err(ValidationFailure::BodyEmpty));
    }

    #[test]
    fn create_comment_validation_trims_body() {
        // decision 0004: 保存はトリム後の値。
        let result = create_comment_validation(" コメント ");
        assert_eq!(result, Ok("コメント".to_string()));
    }
}
