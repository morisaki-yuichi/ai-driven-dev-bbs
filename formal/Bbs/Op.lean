/-
  Bbs.Op — BBS の状態遷移操作

  原典 F01〜F08 に対応する操作をすべて `Action` で書く。
  ここに**存在しない操作は仕様上存在してはならない**操作である:
    - スレッドのタイトル/本文の更新 (C-05 / AC05-4)
    - コメント本文の更新 (C-05 / AC07-4)
    - コメントの物理削除 (C-07)
    - 削除の取り消し (C-08)
    - パスワード・ユニークIDの変更 (issues/04 詳細要件)
-/
import Bbs.Db
import Bbs.Validation

namespace Bbs
namespace Op

open Action

/-! ### 参照ヘルパ -/

def findUser (uid : UserId) : Action User := do
  let s ← get
  liftOption (s.users.find? (·.id = uid)) .notFound

def findUserByUniqueId (u : String) : Action (Option User) := do
  let s ← get
  return s.users.find? (·.uniqueId = u)

/-- 存在するスレッドを引く。**削除されたスレッドは物理的に消える**ので、
    「削除済み」と「元から存在しない」は区別できない。
    C-10 は両者を一律 404 とするため、この区別の消失は仕様上許される。 -/
def findThread (tid : ThreadId) : Action Thread := do
  let s ← get
  liftOption (s.threads.find? (·.id = tid)) .notFound

def findComment (cid : CommentId) : Action Comment := do
  let s ← get
  liftOption (s.comments.find? (·.id = cid)) .notFound

/-- スレッドに紐づくコメント（**削除済みを含む**）。
    C-06（削除可否）と C-16（ソート用件数）はいずれも削除済みを数える。 -/
def commentsOf (tid : ThreadId) : Action (List Comment) := do
  let s ← get
  return s.comments.filter (·.threadId = tid)

/-! ### 認証 (F01〜F03) -/

/-- C-09 の認証ガード。認証必須操作はすべてこれを最初に通す。 -/
def requireAuth (sid : SessionId) : Action UserId := do
  let s ← get
  match s.sessions.find? (·.id = sid) with
  | some sess => return sess.userId
  | none => fail .notAuthenticated

/-- F01 ユーザー登録。AC01-1〜AC01-6。
    **バリデーションと重複チェックの順序は原典に規定がない**（decision 0006）。
    ここでは形式検査 → 強度検査 → 表示名 → 重複、の順で最初の1つを返す。
    ただしパスワード違反だけは内訳をまとめて返す（D11）。 -/
def register (uniqueId password displayName : String) : Action UserId := do
  ensure (Validation.uniqueIdWellFormed uniqueId) (.validation .uniqueIdInvalid)
  let weak := Validation.passwordWeaknesses password
  ensure weak.isEmpty (.validation (.passwordWeak weak))
  match Validation.displayNameFailure displayName with
  | some v => fail (.validation v)
  | none => Action.pure ()
  let displayName := Validation.trim displayName   -- decision 0004
  match ← findUserByUniqueId uniqueId with
  | some _ => fail .duplicateUniqueId
  | none => Action.pure ()
  let s ← get
  let uid := s.nextUserId
  set { s with
    users := s.users ++ [{ id := uid, uniqueId := uniqueId,
                           passwordHash := hashPassword password,
                           displayName := displayName }]
    nextUserId := uid + 1 }
  -- C-18: 登録は**セッションを作らない**（登録後はログイン画面へ）。
  return uid

/-- F02 ログイン。AC02-2 / AC02-3。
    ID不存在とパスワード誤りを同一エラーに潰す（列挙攻撃を避けるためであり、
    AC02-3 も両者に同じ文言を要求している）。 -/
def login (uniqueId password : String) : Action SessionId := do
  let s ← get
  match s.users.find? (·.uniqueId = uniqueId) with
  | none => fail .invalidCredentials
  | some u =>
    ensure (u.passwordHash = hashPassword password) .invalidCredentials
    -- **既存セッションを無効化するかは未規定**（decision 0007）。ここでは併存を許す。
    let sid := s.nextSessionId
    set { s with
      sessions := s.sessions ++ [{ id := sid, userId := u.id }]
      nextSessionId := sid + 1 }
    return sid

/-- F03 ログアウト。AC03-1。
    AC03-2（ブラウザバックで一覧が見えない）は**状態ではなく HTTP キャッシュ制御**の
    要件であり、このモデルでは表現できない（decision 0008）。 -/
def logout (sid : SessionId) : Action Unit := do
  let _ ← requireAuth sid
  modify (fun s => { s with sessions := s.sessions.filter (·.id ≠ sid) })

/-- F04 プロフィール編集。AC04-1 / AC04-3。
    表示名のみ変更可能。D03 方式①のため、投稿側の書き換えは不要
    ＝ AC04-2「過去の投稿にも反映」は**この操作の副作用ではなく
    Query 側の JOIN によって自動的に満たされる**。 -/
def updateDisplayName (sid : SessionId) (newName : String) : Action Unit := do
  let uid ← requireAuth sid
  match Validation.displayNameFailure newName with
  | some v => fail (.validation v)
  | none => Action.pure ()
  let newName := Validation.trim newName   -- decision 0004
  modify (fun s => { s with
    users := s.users.map (fun u => if u.id = uid then { u with displayName := newName } else u) })

/-! ### スレッド (F05, F06) -/

/-- F05 スレッド作成。AC05-1 / AC05-2。 -/
def createThread (sid : SessionId) (title body : String) : Action ThreadId := do
  let uid ← requireAuth sid
  ensure (Validation.nonEmptyText title) (.validation .titleEmpty)
  ensure (Validation.nonEmptyText body) (.validation .bodyEmpty)
  -- decision 0004: 保存はトリム後の値。判定と保存内容を一致させる。
  let title := Validation.trim title
  let body := Validation.trim body
  let now ← tick
  let s ← get
  let tid := s.nextThreadId
  set { s with
    threads := s.threads ++ [{ id := tid, authorId := uid,
                               title := title, body := body, createdAt := now }]
    nextThreadId := tid + 1 }
  return tid

/-- F06 スレッド削除。AC06-1〜AC06-4 / C-06 / C-08。
    条件は「作成者本人 **かつ** コメント0件（削除済みも数える）」。
    削除は物理削除で、復元操作は存在しない。 -/
def deleteThread (sid : SessionId) (tid : ThreadId) : Action Unit := do
  let uid ← requireAuth sid
  let t ← findThread tid
  ensure (t.authorId = uid) .forbidden
  let cs ← commentsOf tid
  ensure cs.isEmpty .threadHasComments
  modify (fun s => { s with threads := s.threads.filter (·.id ≠ tid) })

/-! ### コメント (F07, F08) -/

/-- F07 コメント作成。AC07-1 / AC07-2。
    C-15「最終更新日時はコメント投稿のたびに更新」は、スレッドに
    `lastUpdatedAt` 列を持たせず**コメントの最大 createdAt から導出**する
    （Query.lastUpdatedAt）。冗長な状態を持たないための選択。 -/
def createComment (sid : SessionId) (tid : ThreadId) (body : String) : Action CommentId := do
  let uid ← requireAuth sid
  let _ ← findThread tid
  ensure (Validation.nonEmptyText body) (.validation .bodyEmpty)
  let body := Validation.trim body   -- decision 0004
  let now ← tick
  let s ← get
  let cid := s.nextCommentId
  set { s with
    comments := s.comments ++ [{ id := cid, threadId := tid, authorId := uid,
                                 body := body, createdAt := now, deleted := false }]
    nextCommentId := cid + 1 }
  return cid

/-- F08 コメント削除（論理削除）。AC08-1〜AC08-4 / C-07 / C-08。
    行も本文も消さず `deleted` を立てるだけ。再削除は AC08-4 により拒否する。
    **削除がスレッドの最終更新日時を動かすかは未規定**（decision 0010）。
    本モデルでは `clock` を進めず、最終更新日時に影響させない。 -/
def deleteComment (sid : SessionId) (cid : CommentId) : Action Unit := do
  let uid ← requireAuth sid
  let c ← findComment cid
  ensure (c.authorId = uid) .forbidden
  ensure (!c.deleted) .alreadyDeleted
  modify (fun s => { s with
    comments := s.comments.map (fun x => if x.id = cid then { x with deleted := true } else x) })

end Op
end Bbs
