/-
  Bbs.Scenario — 評価シナリオ 01〜05 をモデル上で再生する煙試験

  証明ではなく実行による健全性確認。`docs/evaluation/scenarios/` の手順を
  そのまま辿り、期待どおりの成功/失敗が出るかを `#eval` で確かめる。
  H-08〜H-10 に合わせて **DB は空から始め、シナリオ間でクリアしない**。
-/
import Bbs.Op
import Bbs.Query

namespace Bbs
namespace Scenario

open Op Query

/-- 期待どおりか判定するための小さなチェック機構。 -/
structure Check where
  label : String
  ok : Bool

def report (cs : List Check) : String :=
  let lines := cs.map fun c => (if c.ok then "  ok   " else "  FAIL ") ++ c.label
  let failed := (cs.filter (fun c => !c.ok)).length
  String.intercalate "\n" lines ++
    s!"\n{cs.length - failed}/{cs.length} passed"

def isErr : Except Error α → Bool
  | .error _ => true
  | .ok _ => false

def errIs (e : Error) : Except Error α → Bool
  | .error e' => e' = e
  | .ok _ => false

/-! ### シナリオ01: 認証 -/

-- 弱いパスワード `password` は 12文字未満・数字なし・記号なしの3違反 (D11)
#eval Validation.passwordWeaknesses "password"
-- => [tooShort, noDigit, noSymbol]

#eval Validation.passwordStrong "TestPassword123!"  -- => true

/-- 空DBから開始し、シナリオ01の登録・ログインまでを実行する。 -/
def s01 : Action (SessionId × UserId) := do
  let uid ← register "testuser_01" "TestPassword123!" "テストユーザー01"
  let sid ← login "testuser_01" "TestPassword123!"
  return (sid, uid)

#eval IO.println <|
  let (r, db) := (s01 Db.empty)
  report [
    { label := "AC01-1 登録成功", ok := !isErr r },
    { label := "AC01-6 ユーザーが永続化されている", ok := db.users.length = 1 },
    { label := "AC02-2 ログイン成功しセッションが1件", ok := db.sessions.length = 1 } ]

#eval IO.println <|
  let (_, db) := (s01 Db.empty)
  report [
    -- AC01-2: 同一ユニークIDでの再登録は重複エラー
    { label := "AC01-2 ID重複を拒否",
      ok := errIs .duplicateUniqueId (register "testuser_01" "Another123!xyz" "べつ" db).1 },
    -- AC02-3: 誤ったパスワード
    { label := "AC02-3 誤パスワードを拒否",
      ok := errIs .invalidCredentials (login "testuser_01" "WrongPassword!" db).1 },
    -- AC01-4 / C-03: 16文字の表示名
    { label := "AC01-4 表示名16文字を拒否",
      ok := isErr (register "u2" "TestPassword123!" "あいうえおかきくけこさしすせそた" db).1 },
    -- AC09-1 / C-09: 無効セッションでは一覧が見えない
    { label := "AC09-1 未ログインで一覧不可",
      ok := errIs .notAuthenticated (viewThreadList 999 .createdDesc 1 db).1 } ]

/-! ### シナリオ02〜03: スレッド・コメント -/

/-- シナリオ01の続きから、02（スレッド作成・他者削除拒否・自削除）と
    03（コメント作成・論理削除・コメント有りスレッドの削除拒否）を実行。 -/
def s0203 : Action Unit := do
  let _ ← register "testuser_01" "TestPassword123!" "テストユーザー01"
  let sid1 ← login "testuser_01" "TestPassword123!"
  let _ ← register "testuser_02" "TestPassword123!" "テストユーザー02"
  let _sid2 ← login "testuser_02" "TestPassword123!"
  let _t1 ← createThread sid1 "AI駆動開発の未来について" "本文A"
  let t2 ← createThread sid1 "コメントテスト用" "本文B"
  let c1 ← createComment sid1 t2 "テストコメント1"
  deleteComment sid1 c1
  let _ ← createComment sid1 t2 "テストコメント2"
  -- 以降のチェックのため t1/t2/sid2 を状態に残したまま終える
  Action.pure ()

#eval IO.println <|
  let (_, db) := (s0203 Db.empty)
  let sid1 : SessionId := 0
  let sid2 : SessionId := 1
  let t1 : ThreadId := 0
  let t2 : ThreadId := 1
  let c1 : CommentId := 0
  report [
    { label := "AC06-3 他者スレッドを削除できない",
      ok := errIs .forbidden (deleteThread sid2 t1 db).1 },
    { label := "AC06-2 コメント有り(削除済み1+通常1)スレッドは削除不可",
      ok := errIs .threadHasComments (deleteThread sid1 t2 db).1 },
    { label := "AC06-1 コメント0件の自スレッドは削除できる",
      ok := !isErr (deleteThread sid1 t1 db).1 },
    { label := "AC06-4 削除後は一覧から消える",
      ok := ((deleteThread sid1 t1 db).2.threads.length = 1) },
    { label := "AC10-5 削除済みスレッドURLは404",
      ok := errIs .notFound (viewThreadDetail sid1 t1 (deleteThread sid1 t1 db).2).1 },
    { label := "AC08-3 他者コメントは削除できない",
      ok := errIs .forbidden (deleteComment sid2 c1 db).1 },
    { label := "AC08-4 削除済みコメントの再削除は不可",
      ok := errIs .alreadyDeleted (deleteComment sid1 c1 db).1 },
    { label := "C-07 論理削除: 行は残る", ok := db.comments.length = 2 },
    { label := "AC08-2 削除済み本文は固定文言",
      ok := (db.comments.filter (fun (c : Comment) => c.deleted)).all (fun (c : Comment) => renderCommentBody c = deletedCommentText) },
    { label := "C-07 元本文はDB上に保持されている",
      ok := (db.comments.find? (fun (c : Comment) => c.id = c1)).any (fun (c : Comment) => c.body = "テストコメント1") },
    { label := "AC07-2 空コメントは拒否",
      ok := isErr (createComment sid1 t2 "   " db).1 },
    -- decision 0012: 削除済みコメントは元本文ごと検索対象外。
    -- 「テストコメント1」は削除済みなので、元本文でも固定文言でもヒットしない。
    { label := "0012 削除済みコメントの元本文は検索でヒットしない",
      ok := (search db "テストコメント1").isEmpty },
    { label := "0012 固定文言も検索でヒットしない",
      ok := (search db "削除されました").isEmpty },
    { label := "0012 未削除のコメントはヒットする",
      ok := (search db "テストコメント2").length = 1 },
    { label := "AC05-2 空タイトルは拒否",
      ok := isErr (createThread sid1 "" "本文" db).1 },
    -- decision 0004: 全角スペースのみも「空」。Lean の Char.isWhitespace は
    -- U+3000 を拾わないので、独自の isSpaceChar が効いていることの確認。
    { label := "0004 全角スペースのみのタイトルは拒否",
      ok := isErr (createThread sid1 "　　" "本文" db).1 },
    { label := "0004 前後空白はトリムして保存",
      ok := ((createThread sid1 "  タイトル  " "  本文  " db).2.threads.find?
               (fun (t : Thread) => t.title = "タイトル")).any
               (fun (t : Thread) => t.body = "本文") } ]

/-! ### シナリオ04: 検索・ソート・ページネーション -/

/-- 一覧を11件にするための埋めスレッド。 -/
def fillThreads (sid : SessionId) : Nat → Action Unit
  | 0 => Action.pure ()
  | n + 1 => do
      let _ ← createThread sid s!"埋め{n}" s!"本文{n}"
      fillThreads sid n

def s04 : Action Unit := do
  let _ ← register "testuser_01" "TestPassword123!" "テストユーザー01"
  let sid ← login "testuser_01" "TestPassword123!"
  let _ ← createThread sid "スレッドA" "プログラミング言語Rustの特徴"
  let tb ← createThread sid "スレッドB" "この本文にキーワードは含まれない"
  let _ ← createComment sid tb "メモリ安全性が高いのがRustの魅力です"
  Action.pure ()

/-- シナリオ04 §3。§2 のソート確認が済んだ**後**に 11 件へ増やす。 -/
def s04pagination : Action Unit := do
  let sid ← login "testuser_01" "TestPassword123!"
  fillThreads sid 9

#eval IO.println <|
  let (_, db) := (s04 Db.empty)
  let (_, db2) := (s04pagination db)
  let sid : SessionId := 0
  let hits := search db "Rust"
  let p1 := threadList db2 .createdAsc 1
  let p2 := threadList db2 .createdAsc 2
  report [
    { label := "AC11-2 スレッドA(本文)とB(コメント)の両方がヒット", ok := hits.length = 2 },
    { label := "AC11-3 スレッドBのヒット位置はコメント",
      ok := (hits.find? (fun (r : SearchResult) => r.thread.title = "スレッドB")).any
              (fun (r : SearchResult) => r.hit ≠ Hit.inBody) },
    { label := "シナリオ04-2-2 作成日時昇順の先頭が最古",
      ok := p1.items.head?.any (fun (r : ThreadRow) => r.id = 0) },
    { label := "AC09-3 1ページ目は10件", ok := p1.items.length = 10 },
    { label := "C-12 1ページ目に「前に戻る」は出ない", ok := p1.hasPrev = false },
    { label := "AC09-5 1ページ目に「次に進む」が出る", ok := p1.hasNext = true },
    { label := "AC09-3 2ページ目に残り1件", ok := p2.items.length = 1 },
    { label := "C-12 最終ページに「次に進む」は出ない", ok := p2.hasNext = false },
    { label := "AC12-4 最終更新日時順の先頭はスレッドB",
      ok := (threadList db .lastUpdatedDesc 1).items.head?.any (fun (r : ThreadRow) => r.title = "スレッドB") },
    { label := "AC12-3/C-16 コメント数順の先頭はスレッドB",
      ok := (threadList db .commentCountDesc 1).items.head?.any (fun (r : ThreadRow) => r.title = "スレッドB") },
    { label := "AC09-4 コメント付きスレッドの最終更新 > 作成日時",
      ok := (db.threads.find? (fun (t : Thread) => t.title = "スレッドB")).any
              (fun (t : Thread) => lastUpdatedAt db t > t.createdAt) },
    { label := "C-09 未ログインでは検索できない",
      ok := errIs .notAuthenticated (viewSearch 999 "Rust" db).1 },
    { label := "AC11-1 認証済みなら検索できる",
      ok := !isErr (viewSearch sid "Rust" db).1 },
    -- decision 0009: 9件がコメント数0で同順位になる状況で、
    -- ページをまたいでも重複も欠落も起きないこと（C-13）。
    -- 第2キー(id)が無いとここが壊れる。
    { label := "0009 コメント数順: 2ページで11件を過不足なく覆う",
      ok := (((threadList db2 .commentCountDesc 1).items
              ++ (threadList db2 .commentCountDesc 2).items).map
                (fun (r : ThreadRow) => r.id)).eraseDups.length = 11 },
    { label := "0009 コメント数順: 全ページ連結がソート済み全体と一致",
      ok := ((threadList db2 .commentCountDesc 1).items
             ++ (threadList db2 .commentCountDesc 2).items).map
               (fun (r : ThreadRow) => r.id)
            = (sortThreads db2 .commentCountDesc db2.threads).map (fun (t : Thread) => t.id) } ]

/-! ### シナリオ05: 表示名変更の全投稿反映 (AC04-2) -/

def s05 : Action Unit := do
  let _ ← register "testuser_01" "TestPassword123!" "テストユーザー01"
  let sid ← login "testuser_01" "TestPassword123!"
  let t ← createThread sid "タイトル" "本文"
  let _ ← createComment sid t "コメント"
  updateDisplayName sid "変更後のユーザー名"

#eval IO.println <|
  let (r, db) := (s05 Db.empty)
  let sid : SessionId := 0
  report [
    { label := "AC04-1 表示名を保存できる", ok := !isErr r },
    { label := "AC04-2 一覧の作成者名が新しい表示名",
      ok := (threadList db .createdDesc 1).items.all
              (fun (row : ThreadRow) => row.authorDisplayName = some "変更後のユーザー名") },
    { label := "AC04-2 詳細のコメント作成者名も新しい表示名",
      ok := (threadDetail db 0).any (fun (d : ThreadDetail) =>
              d.comments.all (fun (c : CommentView) => c.authorDisplayName = some "変更後のユーザー名")) },
    { label := "AC04-3 16文字の表示名は保存できない",
      ok := isErr (updateDisplayName sid "あいうえおかきくけこさしすせそた" db).1 },
    { label := "シナリオ05-1 失敗後も直前の保存値は壊れない",
      ok := ((updateDisplayName sid "あいうえおかきくけこさしすせそた" db).2.users.head?).any
              (fun (u : User) => u.displayName = "変更後のユーザー名") },
    -- decision 0004: 表示名もトリム後に15文字判定・トリム後の値を保存
    { label := "0004 表示名の前後空白はトリムして保存",
      ok := ((updateDisplayName sid "  空白付き  " db).2.users.head?).any
              (fun (u : User) => u.displayName = "空白付き") },
    { label := "0004 トリム後15文字なら通る",
      ok := !isErr (updateDisplayName sid " あいうえおかきくけこさしすせそ " db).1 },
    { label := "AC03-1 ログアウトでセッションが消える",
      ok := (logout sid db).2.sessions.isEmpty },
    { label := "AC03-3 ログアウト後は一覧にアクセスできない",
      ok := errIs .notAuthenticated (viewThreadList sid .createdDesc 1 (logout sid db).2).1 } ]

end Scenario
end Bbs
