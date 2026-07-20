/-
  Bbs.Invariant — 証明すべき性質の言明と、実装済み機能ぶんの証明

  ここに並ぶのは「原典の AC が状態機械の性質として何を意味するか」の翻訳。
  当初は全件を `sorry` で置いた言明集だったが、以降は**機能の実装に合わせて
  対応する定理を証明していく**方針に移した。現在は F01（ユーザー登録）・
  F02（ログイン）・F03（ログアウト）が触れる範囲 ―― 作用モナドの原子性補題
  （`bind_*` / `pure_*` / `fail_*` / `ensure_*` / `guardNone_*` など）、
  `register_atomic`、`login_atomic`、`logout_atomic`、認証ガード
  （`requireAuth_fails_without_session` / `requireAuth_succeeds_with_session` ほか）、
  および `logout_requires_auth` / `logout_effect` / `logout_removes_only_target_session`
  （ログアウトは対象セッションだけを消し、同一利用者の別セッションには影響しない。
  decision 0007 の多重セッション許可と整合する）―― が証明済み。

  F05（スレッド作成）で新たに証明したのは `createThread_atomic`（decision 0002）と
  `createThread_does_not_modify_existing_threads`（C-05/AC05-4。`thread_immutable`の
  一般形の代わりに`createThread`単体へ絞った版 ―― decision 0025 参照）の2件。
  `createThread_requires_auth`（C-09）もF05に対応する定理だが、こちらは
  `requireAuth_fails_without_session` を土台にF02の時点で既に証明済みであり、
  このセッションの成果ではない。

  未実装機能（スレッド削除・コメント・検索・一覧のページングとソート）に対応する
  定理はまだ `sorry` のままで、`lake build` はそのぶんの警告
  （declaration uses 'sorry'）を出すが成功する。証明を試みる過程でさらに
  未規定点が出ることを想定しており、残りは各機能の実装時に順次埋める。
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

/-! #### 補助: `bind` の合成則（F01 の証明で使う）

`register` は「検査を重ねる間は状態を一切書き換えず、最後に一度だけ `set` する」
という構造をしている。この構造から `NoWriteOnError` を導くには、
**成功時にも状態を変えない**性質（`NoWriteOnSuccess`）を補助的に立て、
`Action.bind` がこの2性質をどう合成するかを先に示しておくのが素直。
`Action.bind` の定義（失敗時は束縛前の `s` へ戻す）そのものを使う。 -/

def NoWriteOnSuccess (x : Action α) : Prop :=
  ∀ s a s', x s = (.ok a, s') → s' = s

theorem bind_noWriteOnError {x : Action α} {f : α → Action β}
    (hx : NoWriteOnSuccess x) (hf : ∀ a, NoWriteOnError (f a)) :
    NoWriteOnError (Action.bind x f) := by
  intro s e s' h
  unfold Action.bind at h
  cases hxs : x s with
  | mk r s1 =>
    rw [hxs] at h
    cases r with
    | error e0 => exact (congrArg Prod.snd h).symm
    | ok a =>
      have hs1 : s1 = s := hx s a s1 hxs
      rw [hs1] at h
      exact hf a s e s' h

theorem bind_noWriteOnSuccess {x : Action α} {f : α → Action β}
    (hx : NoWriteOnSuccess x) (hf : ∀ a, NoWriteOnSuccess (f a)) :
    NoWriteOnSuccess (Action.bind x f) := by
  intro s b s' h
  unfold Action.bind at h
  cases hxs : x s with
  | mk r s1 =>
    rw [hxs] at h
    cases r with
    | error e0 => injection h with h1 _; injection h1
    | ok a =>
      have hs1 : s1 = s := hx s a s1 hxs
      rw [hs1] at h
      exact hf a s b s' h

theorem pure_noWriteOnError (a : α) : NoWriteOnError (Action.pure a) := by
  intro s e s' h; exact (congrArg Prod.snd h).symm

theorem pure_noWriteOnSuccess (a : α) : NoWriteOnSuccess (Action.pure a) := by
  intro s a' s' h; exact (congrArg Prod.snd h).symm

theorem fail_noWriteOnError (e : Error) :
    NoWriteOnError (α := α) (Action.fail e) := by
  intro s e' s' h; exact (congrArg Prod.snd h).symm

theorem fail_noWriteOnSuccess (e : Error) :
    NoWriteOnSuccess (α := α) (Action.fail e) := by
  intro s a s' h
  unfold Action.fail at h
  injection h with h1 _
  injection h1

theorem ensure_noWriteOnError (b : Bool) (e : Error) :
    NoWriteOnError (Action.ensure b e) := by
  cases b
  · simp only [Action.ensure]; exact fail_noWriteOnError e
  · simp only [Action.ensure]; exact pure_noWriteOnError ()

theorem ensure_noWriteOnSuccess (b : Bool) (e : Error) :
    NoWriteOnSuccess (Action.ensure b e) := by
  cases b
  · simp only [Action.ensure]; exact fail_noWriteOnSuccess e
  · simp only [Action.ensure]; exact pure_noWriteOnSuccess ()

theorem get_noWriteOnSuccess : NoWriteOnSuccess (α := Db) Action.get := by
  intro s a s' h; exact (congrArg Prod.snd h).symm

theorem get_noWriteOnError : NoWriteOnError (α := Db) Action.get := by
  intro s e s' h
  unfold Action.get at h
  injection h with h1 _
  injection h1

theorem findUserByUniqueId_noWriteOnSuccess (u : String) :
    NoWriteOnSuccess (findUserByUniqueId u) := by
  unfold findUserByUniqueId
  exact bind_noWriteOnSuccess get_noWriteOnSuccess fun s => pure_noWriteOnSuccess _

/-- `guardNone`(`register`の表示名検査・重複検査で使う早期リターン)は、
    `some`なら`fail`、`none`なら何もしないので、どちらの分岐でも状態は変わらない。 -/
theorem guardNone_noWriteOnSuccess {γ : Type} (o : Option γ) (mk : γ → Error) :
    NoWriteOnSuccess (Action.guardNone o mk) := by
  unfold Action.guardNone
  cases o with
  | some v => exact fail_noWriteOnSuccess (mk v)
  | none => exact pure_noWriteOnSuccess ()

/-- F01 登録の原子性（decision 0002）。`register` は
    形式検査 → 強度検査 → 表示名検査 → 重複検査、の順に検査を重ねるだけで、
    どの検査でも実際に `Db` を書き換えるのは検査を全て通過した最後の `set` の
    一度きりである。ゆえに途中で失敗すれば `Action.bind` の巻き戻しにより
    呼び出し前の状態がそのまま返る。
    (実装メモ: 表示名検査・重複検査をdo記法内に直接`match`で書くと、Leanの
     do記法コンパイラが2つの早期リターン用matchの継続を共有する「join point」
     形式(`have __do_jp := ...`)にelaborateされ、`Action.bind x f`という単純形に
     ならず`apply`ベースの合成証明が成立しない。`register`の定義を`guardNone`
     (`Bbs.Db`)経由に書き換えることでこの問題を避けている。) -/
theorem register_atomic (u p d : String) :
    NoWriteOnError (register u p d) := by
  unfold register
  apply bind_noWriteOnError (ensure_noWriteOnSuccess _ _)
  intro _
  apply bind_noWriteOnError (ensure_noWriteOnSuccess _ _)
  intro _
  apply bind_noWriteOnError (guardNone_noWriteOnSuccess _ _)
  intro _
  apply bind_noWriteOnError (findUserByUniqueId_noWriteOnSuccess u)
  intro _
  apply bind_noWriteOnError (guardNone_noWriteOnSuccess _ _)
  intro _
  apply bind_noWriteOnError get_noWriteOnSuccess
  intro s
  -- 残るは `set` してから `pure` するだけの末尾で、これは絶対に失敗しない
  -- (NoWriteOnErrorは空虚に真)。
  intro s0 e s' h
  simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.set, Action.pure] at h
  injection h with h1 _
  injection h1

/-- F02 ログインの原子性（decision 0002。F02のスコープ）。`login` は
    「ID不存在 → invalidCredentials」「パスワード不一致 → invalidCredentials」の
    いずれで失敗しても、実際に `Db` を書き換える `set`（セッション追加）には
    到達しない。AC02-3（誤ったID/パスワードでログイン失敗）で
    セッションが作られてしまわないことを保証する。 -/
theorem login_atomic (u p : String) :
    NoWriteOnError (login u p) := by
  unfold login
  apply bind_noWriteOnError get_noWriteOnSuccess
  intro s
  cases h : s.users.find? (·.uniqueId = u) with
  | none =>
    exact fail_noWriteOnError _
  | some usr =>
    apply bind_noWriteOnError (ensure_noWriteOnSuccess _ _)
    intro _
    intro s0 e s' hh
    simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.set, Action.pure] at hh
    injection hh with h1 _
    injection h1

/-- `Action.modify`は失敗しない(`Except.ok`の一定値を返す)ので、
    `NoWriteOnError`は空虚に真になる。`logout`(F03)の末尾がこの形をしている。 -/
theorem modify_noWriteOnError (f : Db → Db) :
    NoWriteOnError (α := Unit) (Action.modify f) := by
  intro s e s' h
  unfold Action.modify at h
  injection h with h1 _
  injection h1

/-- `requireAuth`は成功時に状態を変えない(`get`のあとは`pure`/`fail`のみ)。
    `logout_atomic`の証明で`requireAuth`を`bind`の左側として使うために要る。 -/
theorem requireAuth_noWriteOnSuccess (sid : SessionId) :
    NoWriteOnSuccess (requireAuth sid) := by
  unfold requireAuth
  apply bind_noWriteOnSuccess get_noWriteOnSuccess
  intro s
  cases s.sessions.find? (·.id = sid) with
  | some sess => exact pure_noWriteOnSuccess _
  | none => exact fail_noWriteOnSuccess _

/-- F03 ログアウトの原子性(decision 0002)。`logout`が失敗しうる唯一の経路は
    `requireAuth`(未認証)であり、それ自体が状態を変えない。認証を通った後の
    `modify`(セッション除去)は失敗しないので、失敗時に部分書き込みが残る余地が
    そもそも無い。 -/
theorem logout_atomic (sid : SessionId) : NoWriteOnError (logout sid) := by
  unfold logout
  apply bind_noWriteOnError (requireAuth_noWriteOnSuccess sid)
  intro _
  exact modify_noWriteOnError _

/-- F05 スレッド作成の原子性(decision 0002)。`createThread`は`requireAuth`→
    タイトル空検査→本文空検査、の順に検査を重ねるだけで、実際に`Db`を書き換える
    `set`（スレッド追加）は全検査を通過した後の`tick`(clockを1進める)と合わせて
    最後に一度だけ実行される。`register_atomic`と同じ構造(検査だけが失敗しうる
    分岐で、検査を全て通過した後の末尾は失敗し得ない)。 -/
theorem createThread_atomic (sid : SessionId) (t b : String) :
    NoWriteOnError (createThread sid t b) := by
  unfold createThread
  apply bind_noWriteOnError (requireAuth_noWriteOnSuccess sid)
  intro _
  apply bind_noWriteOnError (ensure_noWriteOnSuccess _ _)
  intro _
  apply bind_noWriteOnError (ensure_noWriteOnSuccess _ _)
  intro _
  -- 残るは`tick`(clockを進める)・`get`・`set`・`pure`の末尾で、これは絶対に失敗しない
  -- (NoWriteOnErrorは空虚に真)。register_atomicの末尾と同じ理由付けだが、
  -- `tick`自体がAction.modify;Action.getという2段のdo記法なのでその分もsimpで展開する。
  intro s0 e s' h
  simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.set, Action.pure,
    Action.modify, Action.get, tick] at h
  injection h with h1 _
  injection h1

theorem createComment_atomic (sid : SessionId) (tid : ThreadId) (b : String) :
    NoWriteOnError (createComment sid tid b) := by sorry

theorem deleteThread_atomic (sid : SessionId) (tid : ThreadId) :
    NoWriteOnError (deleteThread sid tid) := by sorry

/-! ### 3. 認証ガード (C-09 / AC02-1, AC09-1, AC10-1, AC11-1, AC12-1)

有効なセッションが無ければ、いかなる認証必須操作も
`notAuthenticated` で失敗し、状態は変わらない。 -/

def NoSession (db : Db) (sid : SessionId) : Prop :=
  db.sessions.find? (·.id = sid) = none

/-- AC02-1 の核心（F02のスコープ）: 有効なセッションが無ければ `requireAuth` は
    必ず `notAuthenticated` で失敗し、状態も変えない。C-09 のガードそのものの
    正しさを保証し、これを土台に `createThread_requires_auth` 等を導く。 -/
theorem requireAuth_fails_without_session (db : Db) (sid : SessionId)
    (h : NoSession db sid) :
    (requireAuth sid) db = (.error .notAuthenticated, db) := by
  unfold NoSession at h
  unfold requireAuth
  -- register_atomic/login_atomicの末尾と同じ組(`bind`/`pure`/`Bind.bind`/`Pure.pure`/
  -- `Action.*`)を挙げないと、do記法の`Bind.bind`がAction.bindへ展開しきらない
  -- (`Action.bind`だけを挙げた版は構文不一致で`simp made no progress`になった)。
  simp only [bind, Bind.bind, Action.bind, Action.get, Action.fail, h]

/-- `Query.guarded`（`viewThreadList`/`viewSearch`/`viewThreadDetail`が使う認証ガード）
    も同じ性質を持つ。`Op.requireAuth`とは別実装だが、モデル上「有効なセッションが
    無ければ`notAuthenticated`で状態を変えず失敗する」という同じ契約を果たす。 -/
theorem guarded_fails_without_session (db : Db) (sid : SessionId) (f : Db → α)
    (h : NoSession db sid) :
    (guarded sid f) db = (.error .notAuthenticated, db) := by
  unfold guarded NoSession at *
  simp only [h]

/-- `x`が状態`s`上で失敗するなら、それに何を継いでも(`Action.bind x f`)同じ
    エラー・同じ状態で失敗する(`Action.bind`の定義そのもの)。`exact`/`apply`は
    defeqで単一化するため、do記法が`Bind.bind`経由で展開されていても
    (`simp`と違い)ここを経由すれば素通りできる。 -/
theorem bind_fails_with {x : Action α} {f : α → Action β} {e : Error} {s : Db}
    (hx : x s = (.error e, s)) :
    (Action.bind x f) s = (.error e, s) := by
  simp only [Action.bind, hx]

theorem createThread_requires_auth (db : Db) (sid : SessionId) (t b : String)
    (h : NoSession db sid) :
    (createThread sid t b) db = (.error .notAuthenticated, db) := by
  have hr := requireAuth_fails_without_session db sid h
  unfold createThread
  exact bind_fails_with hr

/-- F03 ログアウト(AC03-1/AC03-3の前提): 有効なセッションが無ければ
    `logout`自体が`notAuthenticated`で失敗し、状態も変えない。実装側では
    `POST /logout`を`require_auth`配下に置くことでこれと同じ契約にする
    (未ログインでのPOSTはログイン画面へリダイレクトし、`sessions`テーブルへは
    一切触れない)。 -/
theorem logout_requires_auth (db : Db) (sid : SessionId)
    (h : NoSession db sid) :
    (logout sid) db = (.error .notAuthenticated, db) := by
  have hr := requireAuth_fails_without_session db sid h
  unfold logout
  exact bind_fails_with hr

theorem viewThreadList_requires_auth (db : Db) (sid : SessionId) (k : SortKey) (p : Nat)
    (h : NoSession db sid) :
    (viewThreadList sid k p) db = (.error .notAuthenticated, db) := by
  unfold viewThreadList
  exact guarded_fails_without_session db sid _ h

theorem viewSearch_requires_auth (db : Db) (sid : SessionId)
    (kw : String) (h : NoSession db sid) :
    (viewSearch sid kw) db = (.error .notAuthenticated, db) := by
  unfold viewSearch
  exact guarded_fails_without_session db sid _ h

/-! ### 3.1 F03 ログアウトの効果 (AC03-1 / decision 0007)

有効なセッションでの `logout` が「対象セッションだけを消し、他は残す」ことを示す。
decision 0007（多重セッション許可）と組み合わせると、同じ利用者の**別セッション**は
ログアウト後も有効なままであることが従う。 -/

/-- `bind_fails_with`の成功版: `x`が状態`s`上で値`a`・状態`s'`へ成功するなら、
    それに継いだ`Action.bind x f`は`f a s'`と同じ結果になる(`Action.bind`の定義)。 -/
theorem bind_succeeds_with {x : Action α} {f : α → Action β} {a : α} {s s' : Db}
    (hx : x s = (.ok a, s')) :
    (Action.bind x f) s = f a s' := by
  simp only [Action.bind, hx]

/-- 有効なセッションがあれば`requireAuth`はそのユーザーIDで成功し、状態を変えない。
    `requireAuth_fails_without_session`の成功版。 -/
theorem requireAuth_succeeds_with_session (db : Db) (sid : SessionId) (uid : UserId)
    (h : db.sessions.find? (·.id = sid) = some ⟨sid, uid⟩) :
    (requireAuth sid) db = (.ok uid, db) := by
  unfold requireAuth
  simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure, h]

/-- F03 ログアウトの効果本体。`logout_removes_only_target_session`はこれと
    `requireAuth_succeeds_with_session`を合成して得る。 -/
theorem logout_effect (db : Db) (sid : SessionId) (uid : UserId)
    (h : (requireAuth sid) db = (.ok uid, db)) :
    (logout sid) db = (.ok (), { db with sessions := db.sessions.filter (·.id ≠ sid) }) := by
  unfold logout
  exact bind_succeeds_with h

/-- ログアウトは対象セッション`sid`だけを`sessions`から消し、他のセッション
    (同一利用者の別セッションを含む)は変更しない。実装の `db::sessions::delete`
    (`delete from sessions where id = $1`)が1行だけを対象にすることの対応物。 -/
theorem logout_removes_only_target_session (db : Db) (sid : SessionId) (uid : UserId)
    (h : db.sessions.find? (·.id = sid) = some ⟨sid, uid⟩) :
    (logout sid db).2.sessions = db.sessions.filter (·.id ≠ sid) := by
  have hr := requireAuth_succeeds_with_session db sid uid h
  rw [logout_effect db sid uid hr]

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

/-- C-05: スレッド本体は不変。残っているなら中身は同一。

    **`hnodup`/`hfresh`は必須の仮定**であり、外すと言明そのものが**偽**になる
    (decision 0025)。反例: `threads := [⟨0,_,"a",_,_⟩, ⟨0,_,"z",_,_⟩]`のように
    idが重複した整形式でない`db`では、`steps = []`だけで
    「`t' ∈ db.threads`かつ`t'.id = t.id`なら`t' = t`」が成り立たない。
    当初この2仮定を欠いた形で書かれていたが、それは「後で証明する」ことが
    原理的に不可能な偽の命題だったため、`createThread_does_not_modify_existing_threads`
    (下記、F05スコープの単一操作版)が採ったのと同じ局所仮定を付けて**真の命題**へ直した。
    `Wf`構造体を丸ごと要求しないのも同じ理由 ―― `Wf`全体の保存はF01〜F03の時点でも
    未証明であり、この言明が実際に要るのは`threadIdsDistinct`/`nextIdsFresh`相当の
    2性質だけである。

    **未証明(`sorry`)のまま残すが、これは「偽の命題を証明しようとしている」のではなく
    「真だが未証明」である。** 証明には`Step`(F01〜F08全種)の各操作が
    `threads`/`nextThreadId`についてこの2性質を保つことを示す必要があり、
    F06〜F08(`deleteThread`/`createComment`/`deleteComment`)はRust側が未実装。
    F05単体のセッションでそこまで踏み込むのは過剰スコープと判断し、この機能に
    対応する単一操作版(`createThread_does_not_modify_existing_threads`)に絞った
    (decision 0025)。F06〜F08の実装時に、`runAll`についてこの2性質が保存されることを
    示して本体を埋める。 -/
theorem thread_immutable (db : Db) (steps : List Step) (t : Thread)
    (hnodup : (db.threads.map (·.id)).Nodup)
    (hfresh : ∀ t ∈ db.threads, t.id < db.nextThreadId)
    (h : t ∈ db.threads) :
    ∀ t' ∈ (runAll steps db).threads, t'.id = t.id → t' = t := by sorry

/-! #### F05スコープの補題: `createThread`は既存スレッドを変更しない (C-05 / AC05-4)

上の`thread_immutable`は`Step`（F01〜F08全種）を跨ぐ一般形で、F06〜F08の`Op`が未実装の
このセッションでは過剰スコープ（上のコメント参照）。ここでは**`createThread`という
単一操作**に絞り、C-05が要求する「作成後に他のスレッドの内容を書き換えない」を
直接証明する。 -/

/-- `thread_immutable`と同様に、**`db`自体が既にID重複を持たない**（`Wf.threadIdsDistinct`
    相当）という前提が要る。反例: `threads := [⟨0,_,"a",_,_⟩, ⟨0,_,"z",_,_⟩]`
    のような不正な`db`では、2件とも`t.id = 0`だが内容が違うので
    「`t' ∈ db.threads`かつ`t'.id = t.id`なら`t' = t`」自体が最初から成り立たない。
    `Wf`構造体を丸ごと要求せず、この証明に要る2性質だけを局所的な仮定として取る
    （`Wf`全体の保存はF01〜F03の時点でも証明されておらず、本セッションの対象外）。 -/
theorem nodup_map_eq_of_mem {α β : Type} [DecidableEq β] {f : α → β} {l : List α}
    (h : (l.map f).Nodup) {a b : α} (ha : a ∈ l) (hb : b ∈ l) (hab : f a = f b) : a = b := by
  induction l with
  | nil => cases ha
  | cons x xs ih =>
    rw [List.map_cons, List.nodup_cons] at h
    obtain ⟨hx, hxs⟩ := h
    rcases List.mem_cons.mp ha with ha' | ha'
    · rcases List.mem_cons.mp hb with hb' | hb'
      · rw [ha', hb']
      · exfalso; apply hx; rw [ha'] at hab; rw [hab]; exact List.mem_map_of_mem hb'
    · rcases List.mem_cons.mp hb with hb' | hb'
      · exfalso; apply hx; rw [hb'] at hab; rw [← hab]; exact List.mem_map_of_mem ha'
      · exact ih hxs ha' hb'

/-- `Action.ensure`の2値を具体形に落とす。`if b then .. else ..`(`b : Bool`)は
    `ite (b = true) .. ..`へ脱糖されるため、`htitle : nonEmptyText title = true`を
    `simp`で代入しても`if True then .. else ..`止まりで完全には簡約されない
    (`decide`系の後始末が要る)。あらかじめ`Bool`literal版の等式として用意しておき、
    本体の証明では`nonEmptyText title`を`true`/`false`へ書き換えた直後にこれを
    適用する2段構えにする。 -/
theorem ensure_true_eq (e : Error) : Action.ensure true e = Action.pure () := by
  unfold Action.ensure; simp

theorem ensure_false_eq (e : Error) : Action.ensure false e = Action.fail e := by
  unfold Action.ensure; simp

/-- F05 / C-05 / AC05-4: `createThread`は、既に`db`に存在するどのスレッドの内容も
    書き換えない。追加(`append`)だけを行う操作なので、生き残ったIDは元の中身のまま。
    `hnodup`/`hfresh`は`thread_immutable`と同じ理由で必要な局所仮定（コメント参照）。 -/
theorem createThread_does_not_modify_existing_threads
    (sid : SessionId) (title body : String) (db : Db)
    (hnodup : (db.threads.map (·.id)).Nodup)
    (hfresh : ∀ t ∈ db.threads, t.id < db.nextThreadId)
    (t : Thread) (h : t ∈ db.threads) :
    ∀ t' ∈ ((createThread sid title body) db).2.threads, t'.id = t.id → t' = t := by
  intro t' ht' hid
  unfold createThread at ht'
  -- `requireAuth sid db`で場合分けする。未認証なら状態は書き変わらず`db`のまま。
  cases hsess : db.sessions.find? (·.id = sid) with
  | none =>
    simp only [bind, Bind.bind, Action.bind, Action.get, Action.fail, requireAuth, hsess] at ht'
    exact nodup_map_eq_of_mem hnodup ht' h hid
  | some sess =>
    -- タイトル・本文の空検査で場合分け。どちらかが空なら状態は書き変わらず`db`のまま。
    cases htitle : Validation.nonEmptyText title with
    | false =>
      simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
        Action.fail, requireAuth, hsess, htitle, ensure_false_eq] at ht'
      exact nodup_map_eq_of_mem hnodup ht' h hid
    | true =>
      cases hbody : Validation.nonEmptyText body with
      | false =>
        simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
          Action.fail, requireAuth, hsess, htitle, hbody, ensure_true_eq, ensure_false_eq] at ht'
        exact nodup_map_eq_of_mem hnodup ht' h hid
      | true =>
        -- ここまで来れば必ず成功し、`db.threads ++ [newThread]`が書き込まれる。
        -- `t'`は元のリストにあったか、新規追加分かのいずれか。
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.modify, tick, requireAuth, hsess,
          htitle, hbody, ensure_true_eq] at ht'
        rw [List.mem_append, List.mem_singleton] at ht'
        rcases ht' with ht' | ht'
        · exact nodup_map_eq_of_mem hnodup ht' h hid
        · -- 新規スレッドは`id := db.nextThreadId`。`hfresh`よりこれは`t.id`と一致しない。
          exfalso
          have hlt : t.id < db.nextThreadId := hfresh t h
          have heq : db.nextThreadId = t.id := by rw [ht'] at hid; exact hid
          exact Nat.lt_irrefl t.id (heq ▸ hlt)

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
