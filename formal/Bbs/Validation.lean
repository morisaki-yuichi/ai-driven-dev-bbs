/-
  Bbs.Validation — 入力バリデーション述語

  原典 C-02 (パスワード強度) / C-03 (表示名) / C-04 (ユニークID) と
  AC05-2 / AC07-2 (空チェック) を述語として書き下す。
  述語を書く過程で原典が沈黙している点は、各定義のコメントに F-番号で示し
  dev-docs/decisions/ に起票してある。
-/
import Bbs.Basic

namespace Bbs
namespace Validation

/-! ### 文字数の数え方 (D15)

`String.length` は Lean では **Unicode コードポイント数**を返す。
原典の「15文字以内」「12文字以上」はこの単位で解釈する（バイト数は誤り）。
書記素クラスタ単位との差（結合文字・絵文字）は本モデルでは無視する。 -/

/-- C-02: 許可された記号。原典 issues/01 の列挙をそのまま写したもの。
    バックスラッシュ・空白・全角記号は**含まれない**。 -/
def allowedSymbols : List Char :=
  "!@#$%^&*()_+-=[]{}|;':\",./<>?".toList

def isSymbol (c : Char) : Bool := allowedSymbols.contains c

/-- 「英字」は ASCII の a-z / A-Z と解釈する。
    原典は「英字」としか書いておらず、`Char.isAlpha` 相当の非ASCII文字
    （例: `é`, 全角 `Ａ`）を英字に数えるかは未規定（decision 0003）。 -/
def isAlpha (c : Char) : Bool :=
  ('a' ≤ c && c ≤ 'z') || ('A' ≤ c && c ≤ 'Z')

def isDigit (c : Char) : Bool := '0' ≤ c && c ≤ '9'

/-! ### 空白とトリム (decision 0004)

**注意: Lean の `Char.isWhitespace` は ASCII 限定**で、全角スペース `U+3000` に対して
`false` を返す。日本語入力を扱う以上これでは不十分なので、独自に空白集合を定義する。
実装言語でも同じ罠がある（Java の `Character.isWhitespace` は U+3000 を含むが
`String.trim` は含まない、JS の `trim` は含む、Go の `strings.TrimSpace` は含む、など
**言語・関数ごとに違う**）。採用する言語で必ず確認すること。 -/

/-- 空白とみなす文字。ASCII 空白に加え、全角スペース `U+3000` と
    ノーブレークスペース `U+00A0` を含める。 -/
def isSpaceChar (c : Char) : Bool :=
  c.isWhitespace || c = '　' || c = ' '

/-- 前後の空白を落とす。 -/
def trim (s : String) : String :=
  (s.toList.dropWhile isSpaceChar |>.reverse.dropWhile isSpaceChar |>.reverse |> String.ofList)

/-- AC05-2 / AC07-2 の「空」＝トリム後に長さ0（decision 0004）。 -/
def isBlank (s : String) : Bool := (trim s).length = 0

/-- C-02 のパスワード強度違反を**すべて**列挙する。
    シナリオ01-1-5 は `password` に対して複数観点のエラー提示を示唆するため、
    最初の1件で打ち切らずリストで返す（D11）。 -/
def passwordWeaknesses (p : String) : List PasswordWeakness :=
  let cs := p.toList
  let tooShort := if p.length < 12 then [PasswordWeakness.tooShort] else []
  let noAlpha := if cs.any isAlpha then [] else [PasswordWeakness.noAlpha]
  let noDigit := if cs.any isDigit then [] else [PasswordWeakness.noDigit]
  let noSymbol := if cs.any isSymbol then [] else [PasswordWeakness.noSymbol]
  -- 「許可された記号のみ」を、英数字と許可記号以外を一切禁じる意味に取る。
  -- 空白・非ASCII文字（日本語等）もここで弾かれる（decision 0003）。
  let bad :=
    if cs.all (fun c => isAlpha c || isDigit c || isSymbol c) then []
    else [PasswordWeakness.disallowedChar]
  tooShort ++ noAlpha ++ noDigit ++ noSymbol ++ bad

def passwordStrong (p : String) : Bool := (passwordWeaknesses p).isEmpty

/-- C-03: 表示名は 15 コードポイント以内。
    **空文字列を許すかは原典に規定がない**（decision 0005）。
    ここでは安全側に倒して「1文字以上15文字以内」とする。
    重複は明示的に許可されているので一意性チェックは無い。
    表示名にも本文と同じトリム規則を適用する（decision 0004）ので、
    長さは**トリム後**に数える。 -/
def displayNameFailure (n : String) : Option ValidationFailure :=
  let t := trim n
  if t.length = 0 then some .displayNameEmpty
  else if t.length > 15 then some .displayNameTooLong
  else none

def displayNameValid (n : String) : Bool := (displayNameFailure n).isNone

/-- C-04 が要求するのは**一意性のみ**で、文字種・長さ・大文字小文字の
    同一視は一切規定がない（D16 / decision 0003）。
    ここでは最小限「空でない」だけを形式条件とし、それ以上の制限は置かない。
    一意性は Db を見ないと判定できないので Op 側で扱う。

    「空」は `displayNameFailure` と同じく**トリム後**で判定する（decision 0004）。
    Why-not: `u.length > 0` だと空白のみのIDが通り、AC05-2 / AC07-2 で定義した
    「空」の基準と食い違う。 -/
def uniqueIdWellFormed (u : String) : Bool := !isBlank u

def nonEmptyText (s : String) : Bool := !isBlank s

end Validation
end Bbs
