//! formal/Bbs/Basic.lean の対応先。DB・HTTPから独立した純粋な型のみを置く。
//!
//! `PasswordWeakness`・`ValidationFailure`・`Error`
//! の一部バリアントは、`domain/validation.rs`(foundation-plan.md §5 #12)が
//! まだ存在しないため未構築。それまでの間 `dead_code` を抑止する。

#![allow(dead_code)]

/// パスワード強度違反の内訳(C-02)。AC01-3はエラー理由の個別提示を要求する(decision 0006)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordWeakness {
    TooShort,
    NoAlpha,
    NoDigit,
    NoSymbol,
    DisallowedChar,
}

/// バリデーション違反の種類。文言そのものはUI層の関心であり、ここでは持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationFailure {
    UniqueIdInvalid,
    PasswordWeak(Vec<PasswordWeakness>),
    DisplayNameTooLong,
    DisplayNameEmpty,
    TitleEmpty,
    BodyEmpty,
}

/// 操作が失敗する理由。formal/Bbs/Basic.lean の `Error` に1対1対応する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// 未ログインで認証必須操作(C-09)
    NotAuthenticated,
    /// 認可違反: 他人の資源への破壊的操作(AC06-3, AC08-3)
    Forbidden,
    /// 存在しない/削除済み資源(C-10)
    NotFound,
    /// ユニークID重複(AC01-2/C-04)
    DuplicateUniqueId,
    /// 認証失敗(AC02-3)
    InvalidCredentials,
    /// 入力バリデーション違反
    Validation(ValidationFailure),
    /// コメントが1件以上あるスレッドの削除(AC06-2/C-06)
    ThreadHasComments,
    /// 削除済みコメントの再削除(AC08-4)
    AlreadyDeleted,
}

/// 削除済みコメントの本文に表示する固定文言(C-01、AC08-2/AC10-3)。
/// テンプレート側はこの定数を参照するのみとし、リテラルを書き写さない(decision 0017)。
pub const DELETED_COMMENT_TEXT: &str = "＜このコメントは削除されました＞";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleted_comment_text_matches_c01_exactly() {
        assert_eq!(DELETED_COMMENT_TEXT, "＜このコメントは削除されました＞");
    }
}
