/-
  Bbs.Db — 状態 `Db` と作用モナド `Action`

  状態は「永続化される事実の集合」であり、UI・HTTP・セッションCookieの
  表現は含めない。リストは**挿入順（古い順）**で保持し、並べ替えは
  問い合わせ側 (Bbs.Query) の責務とする。
-/
import Bbs.Basic

namespace Bbs

/-- 登録ユーザー (F01)。
    投稿側は `authorId` のみを持ち表示名は持たない ＝ D03 の方式①（JOIN解決）を
    モデル上採用している。方式②（非正規化＋カスケード）を採る場合は
    Thread/Comment に `authorDisplayName` が生え、`updateDisplayName` が
    全投稿を書き換える操作になる。 -/
structure User where
  id : UserId
  uniqueId : String
  passwordHash : PasswordHash
  displayName : String
deriving Repr, DecidableEq

/-- スレッド (F05)。タイトル・本文は作成後不変 (C-05) なので、
    これらを書き換える操作は Bbs.Op に**存在しない**（不変性の表現）。 -/
structure Thread where
  id : ThreadId
  authorId : UserId
  title : String
  body : String
  createdAt : Time
deriving Repr, DecidableEq

/-- コメント (F07/F08)。削除は論理削除 (C-07) なので、
    `deleted = true` になっても行は消えず `body` も保持する。
    表示時に固定文言へ差し替えるのは Query 層 (`Query.renderCommentBody`)。 -/
structure Comment where
  id : CommentId
  threadId : ThreadId
  authorId : UserId
  body : String
  createdAt : Time
  deleted : Bool
deriving Repr, DecidableEq

/-- ログインセッション (D04)。実装方式（Cookie/JWT/保存先）は抽象化し、
    「有効なセッションIDからユーザーが一意に定まる」ことだけをモデル化する。
    有効期限は持たない（原典に規定なし）。 -/
structure Session where
  id : SessionId
  userId : UserId
deriving Repr, DecidableEq

/-- 永続化される全状態。
    `clock` は論理時刻で、状態を変える操作ごとに 1 進む（D17/decision 0009 参照）。 -/
structure Db where
  users : List User
  threads : List Thread
  comments : List Comment
  sessions : List Session
  clock : Time
  nextUserId : UserId
  nextThreadId : ThreadId
  nextCommentId : CommentId
  nextSessionId : SessionId
deriving Repr

/-- H-08「評価は完全に空のDBから開始する」に対応する初期状態。 -/
def Db.empty : Db where
  users := []
  threads := []
  comments := []
  sessions := []
  clock := 0
  nextUserId := 0
  nextThreadId := 0
  nextCommentId := 0
  nextSessionId := 0

/-! ### 作用モナド

`Action α = StateM Db (Except Error α)`。
すなわち「状態は必ず返るが、値は失敗しうる」形。`StateT Db (Except Error)` を
使わずこの形にしてあるのは、**失敗時にどの状態を返すかを明示的な選択にする**ため。
`Action.bind` は失敗時に**束縛前の状態へ巻き戻す**（＝部分的な書き込みを残さない）。
この巻き戻しは自明ではなく、原典が明言していない設計判断である（decision 0002）。 -/
def Action (α : Type) : Type := StateM Db (Except Error α)

namespace Action

def run (x : Action α) (s : Db) : Except Error α × Db := x s

/-- 成功値を返し、状態は変えない。 -/
def pure (a : α) : Action α := fun s => (.ok a, s)

/-- 失敗。状態は変えない。 -/
def fail (e : Error) : Action α := fun s => (.error e, s)

/-- 失敗時は `x` の実行前の状態に戻す（原子性）。 -/
def bind (x : Action α) (f : α → Action β) : Action β := fun s =>
  match x s with
  | (.error e, _) => (.error e, s)
  | (.ok a, s') => f a s'

def get : Action Db := fun s => (.ok s, s)

def set (s' : Db) : Action Unit := fun _ => (.ok (), s')

def modify (f : Db → Db) : Action Unit := fun s => (.ok (), f s)

/-- 述語が偽なら指定のエラーで失敗する（ガード）。 -/
def ensure (b : Bool) (e : Error) : Action Unit :=
  if b then pure () else fail e

def liftOption (o : Option α) (e : Error) : Action α :=
  match o with
  | some a => pure a
  | none => fail e

/-- `liftOption`の逆向き：`none`なら成功、`some`ならその値からエラーを組み立てて
    失敗する（早期リターン式の妥当性検査に使う。例: register の表示名検査・
    重複検査）。`register`をこの補助で書くと、do記法の中に`match`が直接複数
    現れなくなる。Lean の do 記法は同一do ブロック内に複数の早期リターン用
    `match`があると継続を共有する join point を作る形にコンパイルされ、
    `Action.bind x f`という単純形にならない。その結果`register_atomic`の証明で
    `split`タクティクが自己参照的な判別式（生成元と適用先が同じ状態）を
    扱えず内部エラーになる（decision対象外・実装上の回避）。 -/
def guardNone (o : Option β) (mk : β → Error) : Action Unit :=
  match o with
  | some b => fail (mk b)
  | none => pure ()

end Action

instance : Monad Action where
  pure := Action.pure
  bind := Action.bind

/-- 論理時刻を 1 進め、進めた**後**の値を返す。
    「同時刻に2つの投稿が並ばない」ことをモデルの側で保証する（decision 0009）。 -/
def tick : Action Time := do
  Action.modify (fun s => { s with clock := s.clock + 1 })
  let s ← Action.get
  return s.clock

end Bbs
