/-
  Bbs.Invariant — 証明すべき性質の**言明のみ**

  本フェーズでは証明を完遂しない。すべて `sorry` を置いてあり、
  `lake build` は警告（declaration uses 'sorry'）を出すが成功する。
  ここに並ぶのは「原典の AC が状態機械の性質として何を意味するか」の翻訳であり、
  証明を試みる過程でさらに未規定点が出ることを想定している。
-/
import Bbs.Op
import Bbs.Query

namespace Bbs
namespace Invariant

open Op Query

/-! ### 1. 構造的な健全性（Db の整合性） -/

/-- Db の整合性述語。空DBで成り立ち、全操作で保存されることを示したい。 -/
structure Wf (db : Db) : Prop where
  /-- ID は一意 -/
  userIdsDistinct : db.users.map (·.id) |>.Nodup
  threadIdsDistinct : db.threads.map (·.id) |>.Nodup
  commentIdsDistinct : db.comments.map (·.id) |>.Nodup
  /-- C-04: ユニークIDは重複しない -/
  uniqueIdsDistinct : db.users.map (·.uniqueId) |>.Nodup
  /-- 参照整合性: 投稿者・セッションのユーザーは実在する -/
  threadAuthorsExist : ∀ t ∈ db.threads, ∃ u ∈ db.users, u.id = t.authorId
  commentAuthorsExist : ∀ c ∈ db.comments, ∃ u ∈ db.users, u.id = c.authorId
  sessionUsersExist : ∀ s ∈ db.sessions, ∃ u ∈ db.users, u.id = s.userId
  /-- 孤児コメントが無い（スレッド削除はコメント0件時のみなので保てるはず） -/
  commentThreadsExist : ∀ c ∈ db.comments, ∃ t ∈ db.threads, t.id = c.threadId
  /-- ID 採番カウンタは既存IDより大きい -/
  nextIdsFresh : ∀ t ∈ db.threads, t.id < db.nextThreadId

theorem wf_empty : Wf Db.empty := by sorry

/-! ### 2. 作用モナドの原子性（decision 0002）

失敗した操作は状態を一切変えない。`StateM Db (Except Error _)` を選んだ以上、
これは型からは保証されず**証明すべき性質**になる。 -/

def NoWriteOnError (x : Action α) : Prop :=
  ∀ s e s', x s = (.error e, s') → s' = s

theorem register_atomic (u p d : String) :
    NoWriteOnError (register u p d) := by sorry

theorem createThread_atomic (sid : SessionId) (t b : String) :
    NoWriteOnError (createThread sid t b) := by sorry

theorem createComment_atomic (sid : SessionId) (tid : ThreadId) (b : String) :
    NoWriteOnError (createComment sid tid b) := by sorry

theorem deleteThread_atomic (sid : SessionId) (tid : ThreadId) :
    NoWriteOnError (deleteThread sid tid) := by sorry

/-! ### 3. 認証ガード (C-09 / AC02-1, AC09-1, AC10-1, AC11-1, AC12-1)

有効なセッションが無ければ、いかなる認証必須操作も
`notAuthenticated` で失敗し、状態は変わらない。 -/

def NoSession (db : Db) (sid : SessionId) : Prop :=
  db.sessions.find? (·.id = sid) = none

theorem createThread_requires_auth (db : Db) (sid : SessionId) (t b : String)
    (h : NoSession db sid) :
    (createThread sid t b) db = (.error .notAuthenticated, db) := by sorry

theorem viewThreadList_requires_auth (db : Db) (sid : SessionId) (k : SortKey) (p : Nat)
    (h : NoSession db sid) :
    (viewThreadList sid k p) db = (.error .notAuthenticated, db) := by sorry

theorem viewSearch_requires_auth (db : Db) (sid : SessionId)
    (kw : String) (h : NoSession db sid) :
    (viewSearch sid kw) db = (.error .notAuthenticated, db) := by sorry

/-! ### 4. 不変性 (C-05 / AC05-4, AC07-4)

スレッドのタイトル・本文、コメントの本文は作成後に変わらない。
「変える操作が無い」ことは Lean では直接書けないので、
「作成済みのスレッドは、その後どんな操作列を実行しても内容が変わらないか、
 消えているかのいずれか」として言明する。 -/

/-- 操作列を抽象化した型。実装は省く（証明段階で列挙する）。 -/
inductive Step where
  | register (u p d : String)
  | login (u p : String)
  | logout (sid : SessionId)
  | updateDisplayName (sid : SessionId) (n : String)
  | createThread (sid : SessionId) (t b : String)
  | deleteThread (sid : SessionId) (tid : ThreadId)
  | createComment (sid : SessionId) (tid : ThreadId) (b : String)
  | deleteComment (sid : SessionId) (cid : CommentId)

def runStep : Step → Action Unit
  | .register u p d => discard <| register u p d
  | .login u p => discard <| login u p
  | .logout sid => logout sid
  | .updateDisplayName sid n => updateDisplayName sid n
  | .createThread sid t b => discard <| createThread sid t b
  | .deleteThread sid tid => deleteThread sid tid
  | .createComment sid tid b => discard <| createComment sid tid b
  | .deleteComment sid cid => deleteComment sid cid

/-- 失敗も許容して先へ進む実行（UI 上のエラーは操作列を止めない）。 -/
def runAll : List Step → Db → Db
  | [], db => db
  | st :: rest, db => runAll rest (runStep st db).2

/-- C-05: スレッド本体は不変。残っているなら中身は同一。 -/
theorem thread_immutable (db : Db) (steps : List Step) (t : Thread)
    (h : t ∈ db.threads) :
    ∀ t' ∈ (runAll steps db).threads, t'.id = t.id → t' = t := by sorry

/-- C-05: コメント本文と作成者・作成日時は不変（`deleted` のみ変化しうる）。 -/
theorem comment_body_immutable (db : Db) (steps : List Step) (c : Comment)
    (h : c ∈ db.comments) :
    ∀ c' ∈ (runAll steps db).comments, c'.id = c.id →
      c'.body = c.body ∧ c'.authorId = c.authorId ∧ c'.createdAt = c.createdAt := by sorry

/-- C-07 / C-08: 論理削除は不可逆かつ非破壊。一度立った `deleted` は下りず、
    行そのものも消えない。 -/
theorem deletion_irreversible (db : Db) (steps : List Step) (c : Comment)
    (h : c ∈ db.comments) (hd : c.deleted = true) :
    ∃ c' ∈ (runAll steps db).comments, c'.id = c.id ∧ c'.deleted = true := by sorry

/-! ### 5. スレッド削除の二重条件 (C-06 / AC06-1〜3) -/

theorem deleteThread_needs_owner (db : Db) (sid : SessionId) (tid : ThreadId)
    (uid : UserId) (t : Thread)
    (hs : db.sessions.find? (·.id = sid) = some ⟨sid, uid⟩)
    (ht : db.threads.find? (·.id = tid) = some t)
    (hne : t.authorId ≠ uid) :
    (deleteThread sid tid) db = (.error .forbidden, db) := by sorry

/-- AC06-2: 削除済みコメントも件数に数える。 -/
theorem deleteThread_blocked_by_deleted_comment (db : Db) (sid : SessionId) (tid : ThreadId)
    (c : Comment) (hc : c ∈ db.comments) (hct : c.threadId = tid) (hcd : c.deleted = true) :
    ∀ e s', (deleteThread sid tid) db = (.error e, s') → s' = db := by sorry

/-! ### 6. 表示名の全投稿反映 (AC04-2)

D03 方式①（JOIN 解決）の正しさ。表示名を変えた直後、
その利用者の過去のスレッド・コメントはすべて新しい表示名で表示される。 -/

theorem displayName_propagates (db : Db) (sid : SessionId) (uid : UserId) (n : String)
    (hs : db.sessions.find? (·.id = sid) = some ⟨sid, uid⟩)
    (hv : Validation.displayNameValid n = true) :
    let db' := (updateDisplayName sid n db).2
    ∀ t ∈ db'.threads, t.authorId = uid → (toRow db' t).authorDisplayName = some n := by
  sorry

/-! ### 7. 一覧・ソート・ページネーション (F09, F12, F13) -/

/-- C-12: ページは常に10件以下。 -/
theorem page_size_bound (db : Db) (k : SortKey) (p : Nat) :
    (threadList db k p).items.length ≤ pageSize := by sorry

/-- C-12: 1ページ目に「前に戻る」は出ない。 -/
theorem first_page_no_prev (db : Db) (k : SortKey) :
    (threadList db k 1).hasPrev = false := by sorry

/-- C-12: 「次に進む」が出るのは、実際に次のページに項目があるときだけ。 -/
theorem hasNext_iff_more (db : Db) (k : SortKey) (p : Nat) :
    (threadList db k p).hasNext = true ↔
      db.threads.length > p * pageSize := by sorry

/-- C-13: ページをまたいでもソート順が保たれる ＝
    全ページを連結すると、ソート済み全体列と一致する。 -/
theorem pagination_preserves_order (db : Db) (k : SortKey) (p : Nat) :
    (threadList db k p).items =
      (((sortThreads db k db.threads).map (toRow db)).drop ((p - 1) * pageSize)).take pageSize
    := by sorry

/-- ソートは並べ替えである（件数も要素も変わらない）。 -/
theorem sortThreads_perm (db : Db) (k : SortKey) (ts : List Thread) :
    (sortThreads db k ts).length = ts.length := by sorry

/-- AC12-3 / C-16: コメント数順は削除済みを含む件数の降順。 -/
theorem sorted_by_commentCount (db : Db) (ts : List Thread) :
    List.Pairwise (fun a b => commentCount db a.id ≥ commentCount db b.id)
      (sortThreads db .commentCountDesc ts) := by sorry

/-- シナリオ04-2-2: 作成日時昇順で先頭が最古。 -/
theorem createdAsc_head_is_oldest (db : Db) (ts : List Thread) (t : Thread)
    (h : (sortThreads db .createdAsc ts).head? = some t) :
    ∀ t' ∈ ts, t.createdAt ≤ t'.createdAt := by sorry

/-! ### 8. 最終更新日時 (C-15 / AC09-4)

コメント投稿はそのスレッドの最終更新日時を厳密に進める。 -/

theorem comment_bumps_lastUpdated (db : Db) (sid : SessionId) (tid : ThreadId) (b : String)
    (t : Thread) (ht : t ∈ db.threads) (ht' : t.id = tid) (cid : CommentId)
    (hok : (createComment sid tid b) db = (.ok cid, (createComment sid tid b db).2)) :
    let db' := (createComment sid tid b db).2
    lastUpdatedAt db t < lastUpdatedAt db' t := by sorry

/-! ### 9. 検索 (F11 / AC11-2, AC11-4) -/

/-- AC11-2: 本文またはコメント本文にキーワードを含むスレッドがヒットする。 -/
theorem search_finds_body (db : Db) (kw : String)
    (t : Thread) (h : t ∈ db.threads) (hc : containsSubstr t.body kw = true) :
    ∃ r ∈ search db kw, r.thread = t := by sorry

/-- AC11-4 の充足（decision 0012 で確定した方式）: 削除済みコメントを理由に
    ヒットすることはない ＝ 返る `Hit` が指すコメントは必ず未削除。 -/
theorem no_deleted_hit (db : Db) (kw : String) (r : SearchResult)
    (h : r ∈ search db kw) (cid : CommentId) (hh : r.hit = .inComment cid) :
    ∃ c ∈ db.comments, c.id = cid ∧ c.deleted = false := by sorry

/-- AC11-3 のスクロール先が実在すること（D19）。
    ヒット箇所は必ず詳細画面に描画される要素を指す。 -/
theorem hit_is_reachable (db : Db) (kw : String)
    (r : SearchResult) (h : r ∈ search db kw) (cid : CommentId)
    (hh : r.hit = .inComment cid) :
    ∃ d, threadDetail db r.thread.id = some d ∧ ∃ cv ∈ d.comments, cv.id = cid := by sorry

/-! ### 10. 固定文言 (C-01 / AC08-2) -/

theorem deleted_comment_renders_fixed_text (c : Comment) (h : c.deleted = true) :
    renderCommentBody c = deletedCommentText := by sorry

/-- AC10-3 の[解釈]: 削除済みでも作成者・日時は維持される。 -/
theorem deleted_comment_keeps_metadata (db : Db) (tid : ThreadId) (d : ThreadDetail)
    (h : threadDetail db tid = some d) (c : Comment) (hc : c ∈ db.comments)
    (hct : c.threadId = tid) :
    ∃ cv ∈ d.comments, cv.id = c.id ∧ cv.createdAt = c.createdAt ∧
      cv.authorDisplayName = displayNameOf db c.authorId := by sorry

end Invariant
end Bbs
