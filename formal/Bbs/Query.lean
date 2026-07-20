/-
  Bbs.Query — 読み取り（一覧・詳細・検索・ソート・ページネーション）

  F09〜F13。状態を変えないので `Db → _` の純関数として書き、
  認証ガードが要る入口だけ `Action` で包む。
-/
import Bbs.Db
import Bbs.Validation

namespace Bbs
namespace Query

open Action

/-- C-01: 削除済みコメントの表示本文。全角山括弧、厳密一致。 -/
def deletedCommentText : String := "＜このコメントは削除されました＞"

/-! ### 部分一致

D07 は照合方式を規定していない。全文検索エンジンの分かち書きだと
`Rust` の部分一致が壊れうるため、モデルでは **素朴な部分文字列一致**を採る。
大文字小文字・全角半角の正規化は行わない（未規定 → decision 0011）。 -/

def isPrefixOfL : List Char → List Char → Bool
  | [], _ => true
  | _, [] => false
  | a :: as, b :: bs => a == b && isPrefixOfL as bs

def containsSubstrL (hay needle : List Char) : Bool :=
  if isPrefixOfL needle hay then true
  else match hay with
    | [] => false
    | _ :: rest => containsSubstrL rest needle

def containsSubstr (hay needle : String) : Bool :=
  containsSubstrL hay.toList needle.toList

/-! ### 導出値 -/

def commentsOf (db : Db) (tid : ThreadId) : List Comment :=
  db.comments.filter (·.threadId = tid)

/-- D13: 一覧に出すコメント数。ソート基準 (C-16) と食い違わないよう
    **削除済みを含める**。 -/
def commentCount (db : Db) (tid : ThreadId) : Nat :=
  (commentsOf db tid).length

def maxTime : List Time → Time
  | [] => 0
  | t :: ts => Nat.max t (maxTime ts)

/-- C-15: 最終更新日時 ＝ スレッド作成時刻と全コメント作成時刻の最大。
    削除済みコメントもここでは数える（投稿された事実は消えないため）。 -/
def lastUpdatedAt (db : Db) (t : Thread) : Time :=
  Nat.max t.createdAt (maxTime ((commentsOf db t.id).map (·.createdAt)))

/-- 表示名は投稿に持たず、常にユーザーから解決する（D03 方式①）。
    これにより AC04-2 が構造的に満たされる。 -/
def displayNameOf (db : Db) (uid : UserId) : Option String :=
  (db.users.find? (·.id = uid)).map (·.displayName)

/-- AC08-2 / AC10-3: 削除済みコメントの本文は固定文言に差し替える。
    作成者・作成日時は維持する（C-07 の[解釈]側の方針）。 -/
def renderCommentBody (c : Comment) : String :=
  if c.deleted then deletedCommentText else c.body

/-! ### 一覧行 (AC09-2) -/

structure ThreadRow where
  id : ThreadId
  title : String
  body : String
  createdAt : Time
  authorDisplayName : Option String
  commentCount : Nat
  lastUpdatedAt : Time
deriving Repr

def toRow (db : Db) (t : Thread) : ThreadRow where
  id := t.id
  title := t.title
  body := t.body
  createdAt := t.createdAt
  authorDisplayName := displayNameOf db t.authorId
  commentCount := commentCount db t.id
  lastUpdatedAt := lastUpdatedAt db t

/-! ### ソート (F12)

D12: シナリオ04-2-2 が「作成日時（**昇順**）」を名指しするため、
昇順は選択肢として**必ず存在しなければならない**。ui_design の3択例のままでは
シナリオが失敗する。ここでは昇順・降順を別の値としてモデル化する。 -/

inductive SortKey where
  | createdAsc
  | createdDesc
  | commentCountDesc
  | lastUpdatedDesc
deriving Repr, DecidableEq

/-- 安定な挿入ソート。`le` が真なら順序を保つ。 -/
def insertBy (le : α → α → Bool) (a : α) : List α → List α
  | [] => [a]
  | b :: bs => if le a b then a :: b :: bs else b :: insertBy le a bs

def sortBy (le : α → α → Bool) : List α → List α
  | [] => []
  | a :: as => insertBy le a (sortBy le as)

/-- **同値キーの並び順（タイブレーク）は原典に規定がない**（decision 0009）。
    ここでは第2キーとして id を使い、全順序にして決定的にする。
    `clock` が単調なので createdAt は実際には衝突しないが、
    コメント数順・最終更新日時順では容易に衝突する。 -/
def leOf (db : Db) : SortKey → Thread → Thread → Bool
  | .createdAsc, a, b => a.createdAt < b.createdAt || (a.createdAt = b.createdAt && a.id ≤ b.id)
  | .createdDesc, a, b => b.createdAt < a.createdAt || (a.createdAt = b.createdAt && a.id ≤ b.id)
  | .commentCountDesc, a, b =>
      let ca := commentCount db a.id
      let cb := commentCount db b.id
      cb < ca || (ca = cb && a.id ≤ b.id)
  | .lastUpdatedDesc, a, b =>
      let la := lastUpdatedAt db a
      let lb := lastUpdatedAt db b
      lb < la || (la = lb && a.id ≤ b.id)

def sortThreads (db : Db) (k : SortKey) (ts : List Thread) : List Thread :=
  sortBy (leOf db k) ts

/-! ### ページネーション (F13 / C-12) -/

def pageSize : Nat := 10

structure Page (α : Type) where
  items : List α
  pageNumber : Nat      -- 1 始まり
  hasPrev : Bool
  hasNext : Bool
deriving Repr

/-- 1ページ目では hasPrev = false、最終ページでは hasNext = false (C-12)。
    **範囲外のページ番号を要求されたときの挙動は未規定**（decision 0013）。
    ここでは空リストを返す（404 にはしない）。 -/
def paginate (n : Nat) (xs : List α) : Page α :=
  let p := if n = 0 then 1 else n
  let dropped := xs.drop ((p - 1) * pageSize)
  { items := dropped.take pageSize
    pageNumber := p
    hasPrev := p > 1
    hasNext := dropped.length > pageSize }

/-! ### 一覧・詳細 -/

/-- F09 スレッド一覧。C-13: ソートキーとページ番号は独立に効く。 -/
def threadList (db : Db) (k : SortKey) (page : Nat) : Page ThreadRow :=
  paginate page ((sortThreads db k db.threads).map (toRow db))

structure CommentView where
  id : CommentId
  authorDisplayName : Option String
  body : String          -- 削除済みなら固定文言
  createdAt : Time
  deleted : Bool
deriving Repr

structure ThreadDetail where
  thread : Thread
  authorDisplayName : Option String
  comments : List CommentView
deriving Repr

/-- F10 スレッド詳細。コメントは作成日時の昇順（会話の文脈順）。
    **詳細ページのコメントはページネーションしない**（decision 0013 §3・決定済）。
    AC10-2「関連する全コメントが表示される」に素直で、AC11-3 の自動スクロール先が
    別ページに落ちない。コメント数が膨大だと重くなるトレードオフは明示的に受容している。 -/
def threadDetail (db : Db) (tid : ThreadId) : Option ThreadDetail :=
  (db.threads.find? (·.id = tid)).map fun t =>
    { thread := t
      authorDisplayName := displayNameOf db t.authorId
      comments := (sortBy (fun a b => a.createdAt ≤ b.createdAt) (commentsOf db t.id)).map
        fun c => { id := c.id
                   authorDisplayName := displayNameOf db c.authorId
                   body := renderCommentBody c
                   createdAt := c.createdAt
                   deleted := c.deleted } }

/-! ### 検索 (F11) -/

/-- ヒット箇所。AC11-3 のスクロール先を決めるために必要（D19）。 -/
inductive Hit where
  | inBody
  | inComment (cid : CommentId)
deriving Repr, DecidableEq

structure SearchResult where
  thread : Thread
  hit : Hit
deriving Repr

/-- 検索対象のコメント。**decision 0012: 論理削除コメントは元本文ごと除外する。**
    AC11-4 は「検索対象にしない」「ヒットしても遷移時に矛盾がない」の両方を許容していたが、
    前者に確定した。固定文言だけでなく**元の本文も**ヒットしない。 -/
def searchableComments (cs : List Comment) : List Comment :=
  cs.filter (fun c => !c.deleted)

/-- スレッド1件に対する最初のヒット。本文を優先し、次に古い順のコメント。
    **decision 0012: タイトルは検索対象に含めない**（issues/11 が本文とコメント本文のみを列挙）。 -/
def hitIn (db : Db) (kw : String) (t : Thread) : Option Hit :=
  if containsSubstr t.body kw then some .inBody
  else
    let cs := searchableComments (commentsOf db t.id)
    (cs.find? (fun c => containsSubstr c.body kw)).map (fun c => .inComment c.id)

/-- F11 検索。空クエリは全件にマッチする（`containsSubstr _ "" = true`、decision 0011）。
    検索結果にもソート・ページネーションを適用する（decision 0011）ので、
    呼び出し側は結果のスレッドを `sortThreads` / `paginate` に通す。 -/
def search (db : Db) (kw : String) : List SearchResult :=
  db.threads.filterMap fun t => (hitIn db kw t).map fun h => { thread := t, hit := h }

/-! ### 認証ガード付きの入口 (C-09)

未ログインでは一覧・詳細・検索・ソートのいずれにも到達できない。 -/

def guarded (sid : SessionId) (f : Db → α) : Action α := fun s =>
  match s.sessions.find? (·.id = sid) with
  | none => (.error .notAuthenticated, s)
  | some _ => (.ok (f s), s)

def viewThreadList (sid : SessionId) (k : SortKey) (page : Nat) : Action (Page ThreadRow) :=
  guarded sid (fun db => threadList db k page)

/-- AC10-5: 存在しない／削除済みスレッドは 404。 -/
def viewThreadDetail (sid : SessionId) (tid : ThreadId) : Action ThreadDetail := do
  let od ← guarded sid (fun db => threadDetail db tid)
  liftOption od .notFound

def viewSearch (sid : SessionId) (kw : String) : Action (List SearchResult) :=
  guarded sid (fun db => search db kw)

end Query
end Bbs
