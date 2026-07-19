/-
  Bbs.Basic — 基本型・エラー・作用モナド

  対応する原典/分析: dev-docs/requirements-analysis.md §2.2 (C-01〜C-18)
-/

namespace Bbs

/-- 論理時刻。原典はタイムゾーンも粒度も規定していない（D17）。
    ここでは「単調増加する抽象時刻」としてのみモデル化する。 -/
abbrev Time := Nat

abbrev UserId := Nat
abbrev ThreadId := Nat
abbrev CommentId := Nat
abbrev SessionId := Nat

/-- パスワードハッシュ。D04 の[空白]（方式未定）を隠蔽するための抽象。
    `hash` が単射であることだけを仮定に置く（平文保存はしない前提）。 -/
structure PasswordHash where
  repr : String
deriving DecidableEq, Repr

def hashPassword (p : String) : PasswordHash := ⟨p⟩

/-- パスワード強度違反の内訳 (C-02)。AC01-3 / シナリオ01-1-5 は
    「短すぎます」「記号が含まれていません」の**個別提示**を示唆する（D11）。 -/
inductive PasswordWeakness where
  | tooShort
  | noAlpha
  | noDigit
  | noSymbol
  | disallowedChar
deriving Repr, DecidableEq

/-- バリデーション違反の種類。AC01-3 が違反理由の判別を要求するため、
    複数違反を保持できるよう `passwordWeak` は理由のリストを持つ。 -/
inductive ValidationFailure where
  | uniqueIdInvalid
  | passwordWeak (reasons : List PasswordWeakness)
  | displayNameTooLong
  | displayNameEmpty
  | titleEmpty
  | bodyEmpty
deriving Repr, DecidableEq

/-- 操作が失敗する理由。原典の AC が要求する区別のみを列挙する。
    文言そのものは UI 層の関心（D11）なのでここでは持たない。 -/
inductive Error where
  /-- 未ログインで認証必須操作 (C-09 / AC02-1, AC09-1, AC10-1, AC11-1, AC12-1) -/
  | notAuthenticated
  /-- 認可違反: 他人の資源への破壊的操作 (AC06-3, AC08-3) -/
  | forbidden
  /-- 存在しない / 削除済み資源 (C-10 / AC10-5) -/
  | notFound
  /-- ユニークID重複 (AC01-2 / C-04) -/
  | duplicateUniqueId
  /-- 認証失敗 (AC02-3) -/
  | invalidCredentials
  /-- 入力バリデーション違反。違反した述語の識別子を保持する。 -/
  | validation (v : ValidationFailure)
  /-- コメントが1件以上あるスレッドの削除 (AC06-2 / C-06) -/
  | threadHasComments
  /-- 削除済みコメントの再削除 (AC08-4) -/
  | alreadyDeleted
deriving Repr, DecidableEq

end Bbs
