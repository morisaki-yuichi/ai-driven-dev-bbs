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
  /-- F07/decision 0027: 論理時計は全レコードの作成時刻を支配する。
      `comment_bumps_lastUpdated`（C-15/AC09-4）が要る性質で、当初は同定理の
      局所仮定（`hclockT`/`hclockC`）として持っていたが、この定理を実際に
      証明するF07セッションで`Wf`へ集約した（decision 0027の選択肢(a)、
      同decisionの「決定」節で確定済みの方針）。`tick`が新規レコードに
      `clock + 1`（進めた後の値）を付けるため、既存レコードは常に`clock`以下に
      留まる、という`Db`の構造的な性質。 -/
  clockDominatesThreads : ∀ t ∈ db.threads, t.createdAt ≤ db.clock
  clockDominatesComments : ∀ c ∈ db.comments, c.createdAt ≤ db.clock
  /-- `nextIdsFresh`のユーザー版・コメント版。**F08のレビュー指摘対応セッションで追加した。**
      `userIdsDistinct`/`commentIdsDistinct`は`Wf`の他のフィールドだけからは
      **保存を証明できない** ―― `register`が新規ユーザーに付ける`db.nextUserId`が
      既存のどの`u.id`とも衝突しないことは、「採番カウンタが既存IDを上回る」という
      性質を`Wf`自身が持っていなければ言えない（`createComment`と`nextCommentId`も同様）。
      スレッドについては`nextIdsFresh`が最初からこの役割を果たしていた。
      decision 0027 が `clockDominates*` を追加したのと同じ理由・同じ形の拡張で、
      「`Wf`を帰納的（inductive）にするために必要な強化」。末尾に足してあるのは
      既存の証明（フィールド順に依存する分解・構成）へ波及させないため。 -/
  nextUserIdFresh : ∀ u ∈ db.users, u.id < db.nextUserId
  nextCommentIdFresh : ∀ c ∈ db.comments, c.id < db.nextCommentId

theorem wf_empty : Wf Db.empty := by
  constructor <;> simp [Db.empty]

/-! ### 1.1 `Wf` の保存 ―― 最小連鎖 (register → login → createThread → createComment)

**このセクションが解く問題**: `wf_empty` だけが `Wf` の住人を与えている状態では、
`Wf db` と `t ∈ db.threads` を同時に要求する定理（`comment_bumps_lastUpdated`、C-15/AC09-4）は
**非空虚に適用できない** ―― 唯一の `Wf` の住人 `Db.empty` は `threads = []` だからである。
「証明は通るが適用できない」定理を残さないために、空DBから実際にスレッドのある状態へ
到達する経路の各操作について `Wf` の保存を示し、末尾で具体例に適用してみせる
（`comment_bumps_lastUpdated_is_applicable`、セクション8）。

**どこまでやったか**: `register` / `login` / `createThread` / `createComment` / `deleteThread`
の5操作。うち前4つは「`Db.empty` から出発してコメント付きスレッドのある状態へ至る」ための
最短経路であり、上記の目的を果たすのに過不足ない範囲として選んだ（`Op` の全7操作 × `Wf` の
全13 conjunct を埋めるフルセットは、この目的には不要なので**意図的にスコープ外**とした）。
`deleteThread`（F06）はF06のRust実装が入ったセッションで追加した ――
`commentThreadsExist`（孤児コメントが無い）の維持が実質的な論点で、`deleteThread` は
コメント0件のときしか成功しないので真だが、`ensure cs.isEmpty` から
「除去対象スレッドを参照するコメントが存在しない」を取り出す一手が要った
（`deleteThread_preserves_wf`）。

**何が残っているか**（後続セッションの宿題）:
- `deleteComment`（F08）・`updateDisplayName`（F04）の保存補題。どちらも
  `List.map` による既存要素の書き換えのみでIDを変えないため、
  `runStep_preserves_commentTrackingInvariant` の同名分岐と同じ形で埋まるはずだが、
  本セッションの目的（上記）には不要なため書いていない。
- `Wf` を `runAll`（`Step` 列）へ持ち上げた一般形。上記1点が揃ってから。 -/

/-- `Action.ensure`の2値を具体形に落とす。`if b then .. else ..`(`b : Bool`)は
    `ite (b = true) .. ..`へ脱糖されるため、`htitle : nonEmptyText title = true`を
    `simp`で代入しても`if True then .. else ..`止まりで完全には簡約されない
    (`decide`系の後始末が要る)。あらかじめ`Bool`literal版の等式として用意しておき、
    本体の証明では`nonEmptyText title`を`true`/`false`へ書き換えた直後にこれを
    適用する2段構えにする。
    (配置メモ: 元はセクション4に置いていたが、F08レビュー対応でこのセクション1.1が
     先に使うようになったため前方へ移した。定義内容は変えていない。) -/
theorem ensure_true_eq (e : Error) : Action.ensure true e = Action.pure () := by
  unfold Action.ensure; simp

theorem ensure_false_eq (e : Error) : Action.ensure false e = Action.fail e := by
  unfold Action.ensure; simp

/-- 末尾に1件足しても、その1件のキーが既存のどれとも違えば `Nodup` は保たれる。
    `register`（`users`）・`createThread`（`threads`）・`createComment`（`comments`）の
    3箇所が同じ議論を要求するため、`createComment` の分岐に埋まっていた形
    （`runStep_preserves_commentTrackingInvariant`）を補題として括り出した。 -/
theorem nodup_map_append_singleton {α β : Type} [DecidableEq β] (f : α → β) (l : List α) (a : α)
    (h : (l.map f).Nodup) (hfresh : ∀ x ∈ l, f x ≠ f a) :
    ((l ++ [a]).map f).Nodup := by
  rw [List.map_append, List.map_singleton]
  refine List.nodup_append.mpr ⟨h, List.nodup_cons.mpr ⟨List.not_mem_nil, List.nodup_nil⟩, ?_⟩
  intro x hx y hy
  rw [List.mem_map] at hx
  obtain ⟨z, hz, hzx⟩ := hx
  simp only [List.mem_singleton] at hy
  rw [← hzx, hy]
  exact hfresh z hz

/-- フィルタは`Nodup`を保つ（`filter_sublist`で得る`Sublist`を`Sublist.map`で写した後に
    `Nodup.sublist`へ渡す）。`deleteThread`は`threads`から`filter`で要素を除くだけなので、
    `nodup`(id重複無し)がそのまま保たれることを示すのに使う。`deleteThread_preserves_wf`
    (`threadIdsDistinct`の保存)と、後出の`runStep_preserves_threadTrackingInvariant`の
    `deleteThread`成功分岐(セクション4)の両方が同じ議論を要するため、ここで括り出す。 -/
theorem nodup_map_filter {α β : Type} [DecidableEq β] (f : α → β) (p : α → Bool) (l : List α)
    (h : (l.map f).Nodup) : ((l.filter p).map f).Nodup :=
  List.Nodup.sublist (List.Sublist.map f List.filter_sublist) h

/-- F01: `register` は `Wf` を保つ。失敗する4分岐（形式・強度・表示名・重複）は
    いずれも状態を書き換えないので `h` をそのまま返し、成功分岐だけが議論を要する。
    `userIdsDistinct` に `nextUserIdFresh`、`uniqueIdsDistinct` に重複検査
    （`findUserByUniqueId` が `none`）を使う。 -/
theorem register_preserves_wf (u p d : String) (db : Db) (h : Wf db) :
    Wf ((register u p d) db).2 := by
  unfold register
  cases hwf : Validation.uniqueIdWellFormed u with
  | false =>
    simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
      Action.fail, hwf, ensure_false_eq]
    exact h
  | true =>
    cases hweak : (Validation.passwordWeaknesses p).isEmpty with
    | false =>
      simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
        Action.fail, hwf, hweak, ensure_true_eq, ensure_false_eq]
      exact h
    | true =>
      cases hdn : Validation.displayNameFailure d with
      | some v =>
        simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
          Action.fail, hwf, hweak, hdn, ensure_true_eq, Action.guardNone]
        exact h
      | none =>
        cases hex : db.users.find? (·.uniqueId = u) with
        | some usr =>
          simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
            Action.fail, hwf, hweak, hdn, hex, ensure_true_eq, Action.guardNone,
            findUserByUniqueId]
          exact h
        | none =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get,
            Action.set, Action.pure, hwf, hweak, hdn, hex, ensure_true_eq,
            Action.guardNone, findUserByUniqueId]
          obtain ⟨hu, ht, hc, huq, hta, hca, hsa, hct, hfresh, hclt, hclc, huf, hcf⟩ := h
          -- 新規ユーザーのidは`db.nextUserId`、uniqueIdは`u`（トリムするのは表示名だけ）。
          have hidne : ∀ x ∈ db.users, x.id ≠ db.nextUserId := fun x hx =>
            Nat.ne_of_lt (huf x hx)
          have huqne : ∀ x ∈ db.users, x.uniqueId ≠ u := by
            intro x hx
            have := List.find?_eq_none.mp hex x hx
            simpa using this
          refine ⟨?_, ht, hc, ?_, ?_, ?_, ?_, hct, hfresh, hclt, hclc, ?_, hcf⟩
          · exact nodup_map_append_singleton _ _ _ hu hidne
          · exact nodup_map_append_singleton _ _ _ huq huqne
          · intro t htmem
            obtain ⟨v, hv, hvid⟩ := hta t htmem
            exact ⟨v, List.mem_append_left _ hv, hvid⟩
          · intro c hcmem
            obtain ⟨v, hv, hvid⟩ := hca c hcmem
            exact ⟨v, List.mem_append_left _ hv, hvid⟩
          · intro s hsmem
            obtain ⟨v, hv, hvid⟩ := hsa s hsmem
            exact ⟨v, List.mem_append_left _ hv, hvid⟩
          · intro x hx
            rw [List.mem_append, List.mem_singleton] at hx
            rcases hx with hx | hx
            · exact Nat.lt_of_lt_of_le (huf x hx) (Nat.le_succ _)
            · subst hx; exact Nat.lt_succ_self _

/-- F02: `login` は `Wf` を保つ。`sessions` に1件足すだけなので、実質的な論点は
    `sessionUsersExist`（新しいセッションのユーザーが実在すること）だけで、
    それは認証に成功した `usr` 自身が `db.users` の要素であることから従う。 -/
theorem login_preserves_wf (u p : String) (db : Db) (h : Wf db) :
    Wf ((login u p) db).2 := by
  unfold login
  cases hu : db.users.find? (·.uniqueId = u) with
  | none =>
    simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
      Action.pure, Action.fail, hu]
    exact h
  | some usr =>
    cases hpw : decide (usr.passwordHash = hashPassword p) with
    | false =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, hu, hpw, ensure_false_eq]
      exact h
    | true =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, hu, hpw, ensure_true_eq]
      obtain ⟨hus, ht, hc, huq, hta, hca, hsa, hct, hfresh, hclt, hclc, huf, hcf⟩ := h
      have husr : usr ∈ db.users := List.mem_of_find?_eq_some hu
      refine ⟨hus, ht, hc, huq, hta, hca, ?_, hct, hfresh, hclt, hclc, huf, hcf⟩
      intro s hs
      rw [List.mem_append, List.mem_singleton] at hs
      rcases hs with hs | hs
      · exact hsa s hs
      · subst hs; exact ⟨usr, husr, rfl⟩

/-- F05: `createThread` は `Wf` を保つ。論点は3つ ――
    `threadIdsDistinct`（新規idは `nextIdsFresh` より既存と衝突しない）、
    `threadAuthorsExist`（作成者は認証を通ったセッションのユーザーなので
    `sessionUsersExist` で実在する）、`clockDominatesThreads`（`tick` が付ける
    時刻は進めた後の `clock + 1` そのものなので等号で収まる）。 -/
theorem createThread_preserves_wf (sid : SessionId) (title body : String) (db : Db)
    (h : Wf db) : Wf ((createThread sid title body) db).2 := by
  unfold createThread
  cases hs : db.sessions.find? (·.id = sid) with
  | none =>
    simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
      Action.pure, Action.fail, Action.modify, tick, requireAuth, hs]
    exact h
  | some sess =>
    cases htitle : Validation.nonEmptyText title with
    | false =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, Action.modify, tick, requireAuth, hs, htitle,
        ensure_false_eq]
      exact h
    | true =>
      cases hbody : Validation.nonEmptyText body with
      | false =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, Action.modify, tick, requireAuth, hs, htitle, hbody,
          ensure_true_eq, ensure_false_eq]
        exact h
      | true =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, Action.modify, tick, requireAuth, hs, htitle, hbody,
          ensure_true_eq]
        obtain ⟨hus, ht, hc, huq, hta, hca, hsa, hct, hfresh, hclt, hclc, huf, hcf⟩ := h
        have hsess : sess ∈ db.sessions := List.mem_of_find?_eq_some hs
        have hauthor : ∃ v ∈ db.users, v.id = sess.userId := hsa sess hsess
        have hidne : ∀ x ∈ db.threads, x.id ≠ db.nextThreadId := fun x hx =>
          Nat.ne_of_lt (hfresh x hx)
        refine ⟨hus, ?_, hc, huq, ?_, hca, hsa, ?_, ?_, ?_, ?_, huf, hcf⟩
        · exact nodup_map_append_singleton _ _ _ ht hidne
        · intro t htmem
          rw [List.mem_append, List.mem_singleton] at htmem
          rcases htmem with htmem | htmem
          · exact hta t htmem
          · subst htmem; exact hauthor
        · intro c hcmem
          obtain ⟨v, hv, hvid⟩ := hct c hcmem
          exact ⟨v, List.mem_append_left _ hv, hvid⟩
        · intro x hx
          rw [List.mem_append, List.mem_singleton] at hx
          rcases hx with hx | hx
          · exact Nat.lt_of_lt_of_le (hfresh x hx) (Nat.le_succ _)
          · subst hx; exact Nat.lt_succ_self _
        · intro x hx
          rw [List.mem_append, List.mem_singleton] at hx
          rcases hx with hx | hx
          · exact Nat.le_trans (hclt x hx) (Nat.le_succ _)
          · subst hx; exact Nat.le_refl _
        · intro x hx
          exact Nat.le_trans (hclc x hx) (Nat.le_succ _)

/-- F07: `createComment` は `Wf` を保つ。`createThread` と同じ骨格で、
    加わる論点は `commentThreadsExist`（孤児コメントを作らない）―― これは
    `findThread` が成功した、すなわち対象スレッドが実在することから従う。 -/
theorem createComment_preserves_wf (sid : SessionId) (tid : ThreadId) (body : String) (db : Db)
    (h : Wf db) : Wf ((createComment sid tid body) db).2 := by
  unfold createComment
  cases hs : db.sessions.find? (·.id = sid) with
  | none =>
    simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
      Action.pure, Action.fail, Action.modify, tick, requireAuth, findThread,
      Action.liftOption, hs]
    exact h
  | some sess =>
    cases hthread : db.threads.find? (·.id = tid) with
    | none =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, Action.modify, tick, requireAuth, findThread,
        Action.liftOption, hs, hthread]
      exact h
    | some thr =>
      cases hbody : Validation.nonEmptyText body with
      | false =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, Action.modify, tick, requireAuth, findThread,
          Action.liftOption, hs, hthread, hbody, ensure_false_eq]
        exact h
      | true =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, Action.modify, tick, requireAuth, findThread,
          Action.liftOption, hs, hthread, hbody, ensure_true_eq]
        obtain ⟨hus, ht, hc, huq, hta, hca, hsa, hct, hfresh, hclt, hclc, huf, hcf⟩ := h
        have hsess : sess ∈ db.sessions := List.mem_of_find?_eq_some hs
        have hauthor : ∃ v ∈ db.users, v.id = sess.userId := hsa sess hsess
        have hthrmem : thr ∈ db.threads := List.mem_of_find?_eq_some hthread
        have hthrid : thr.id = tid := by simpa using List.find?_some hthread
        have hidne : ∀ x ∈ db.comments, x.id ≠ db.nextCommentId := fun x hx =>
          Nat.ne_of_lt (hcf x hx)
        refine ⟨hus, ht, ?_, huq, hta, ?_, hsa, ?_, hfresh, ?_, ?_, huf, ?_⟩
        · exact nodup_map_append_singleton _ _ _ hc hidne
        · intro x hx
          rw [List.mem_append, List.mem_singleton] at hx
          rcases hx with hx | hx
          · exact hca x hx
          · subst hx; exact hauthor
        · intro x hx
          rw [List.mem_append, List.mem_singleton] at hx
          rcases hx with hx | hx
          · exact hct x hx
          · subst hx; exact ⟨thr, hthrmem, hthrid⟩
        · intro x hx
          exact Nat.le_trans (hclt x hx) (Nat.le_succ _)
        · intro x hx
          rw [List.mem_append, List.mem_singleton] at hx
          rcases hx with hx | hx
          · exact Nat.le_trans (hclc x hx) (Nat.le_succ _)
          · subst hx; exact Nat.le_refl _
        · intro x hx
          rw [List.mem_append, List.mem_singleton] at hx
          rcases hx with hx | hx
          · exact Nat.lt_of_lt_of_le (hcf x hx) (Nat.le_succ _)
          · subst hx; exact Nat.lt_succ_self _

/-- F06: `deleteThread` は `Wf` を保つ。物理削除で `threads` から要素を1件除くだけなので、
    大半のフィールドは`List.mem_filter`（フィルタ後の要素は元のリストの要素でもある）から
    直接従う。実質的な論点は2つ ―― `threadIdsDistinct`（`nodup_map_filter`、フィルタは
    `Nodup`を保つ）と`commentThreadsExist`（孤児コメントが無いことの維持、本節冒頭の
    「何が残っているか」で予告した論点）。後者は`deleteThread`が
    `ensure cs.isEmpty .threadHasComments`（`cs := commentsOf tid`）を通過したときしか
    成功しないことから従う ―― `hce : (db.comments.filter (·.threadId = tid)).isEmpty = true`
    から「削除対象`tid`を参照するコメントは存在しない」を取り出し、`commentThreadsExist`が
    与える`t'.id = c.threadId`な`t'`が`tid`と一致しないこと（＝フィルタ後も生き残ること）を
    背理法で示す。 -/
theorem deleteThread_preserves_wf (sid : SessionId) (tid : ThreadId) (db : Db)
    (h : Wf db) : Wf ((deleteThread sid tid) db).2 := by
  unfold deleteThread
  cases hs : db.sessions.find? (·.id = sid) with
  | none =>
    simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
      Action.pure, Action.fail, Action.modify, requireAuth, findThread, Action.liftOption,
      Op.commentsOf, hs]
    exact h
  | some sess =>
    cases ht : db.threads.find? (·.id = tid) with
    | none =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, Action.modify, requireAuth, findThread, Action.liftOption,
        Op.commentsOf, hs, ht]
      exact h
    | some thr =>
      cases how : decide (thr.authorId = sess.userId) with
      | false =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, Action.modify, requireAuth, findThread, Action.liftOption,
          Op.commentsOf, hs, ht, how, ensure_false_eq]
        exact h
      | true =>
        cases hce : (db.comments.filter (·.threadId = tid)).isEmpty with
        | false =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, findThread,
            Action.liftOption, Op.commentsOf, hs, ht, how, hce, ensure_true_eq, ensure_false_eq]
          exact h
        | true =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, findThread,
            Action.liftOption, Op.commentsOf, hs, ht, how, hce, ensure_true_eq]
          obtain ⟨hus, htd, hc, huq, hta, hca, hsa, hct, hfresh, hclt, hclc, huf, hcf⟩ := h
          refine ⟨hus, ?_, hc, huq, ?_, hca, hsa, ?_, ?_, ?_, hclc, huf, hcf⟩
          · exact nodup_map_filter _ _ _ htd
          · intro t' ht'mem
            exact hta t' (List.mem_filter.mp ht'mem).1
          · intro c hcmem
            obtain ⟨t', ht'mem, ht'id⟩ := hct c hcmem
            have hne : t'.id ≠ tid := by
              intro heq
              have hceq : c.threadId = tid := by rw [← ht'id]; exact heq
              have hcin : c ∈ db.comments.filter (·.threadId = tid) := by
                rw [List.mem_filter]; exact ⟨hcmem, by simp [hceq]⟩
              cases hf : db.comments.filter (·.threadId = tid) with
              | nil => rw [hf] at hcin; cases hcin
              | cons a as => rw [hf] at hce; simp at hce
            exact ⟨t', by rw [List.mem_filter]; exact ⟨ht'mem, by simp [hne]⟩, ht'id⟩
          · intro t' ht'mem
            exact hfresh t' (List.mem_filter.mp ht'mem).1
          · intro t' ht'mem
            exact hclt t' (List.mem_filter.mp ht'mem).1

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

/-- `liftOption`(`findThread`/`findComment`等が使う早期リターン)は、`some`なら
    `pure`、`none`なら`fail`なので、どちらの分岐でも状態は変わらない。
    `guardNone_noWriteOnSuccess`の逆向き版。 -/
theorem liftOption_noWriteOnSuccess (o : Option α) (e : Error) :
    NoWriteOnSuccess (Action.liftOption o e) := by
  unfold Action.liftOption
  cases o with
  | some a => exact pure_noWriteOnSuccess a
  | none => exact fail_noWriteOnSuccess e

/-- `findThread`(`get`のあとに`liftOption`)は成功時に状態を変えない。
    `createComment_atomic`の証明で`requireAuth`と同じ形で使う。 -/
theorem findThread_noWriteOnSuccess (tid : ThreadId) : NoWriteOnSuccess (findThread tid) := by
  unfold findThread
  exact bind_noWriteOnSuccess get_noWriteOnSuccess fun _ => liftOption_noWriteOnSuccess _ _

/-- `commentsOf`(`get`のあとに`return`)は成功時に状態を変えない。純粋な射影
    (フィルタするだけ)で、`findUserByUniqueId_noWriteOnSuccess`と同じ形。
    `deleteThread_atomic`の証明で使う。 -/
theorem commentsOf_noWriteOnSuccess (tid : ThreadId) : NoWriteOnSuccess (Op.commentsOf tid) := by
  unfold Op.commentsOf
  exact bind_noWriteOnSuccess get_noWriteOnSuccess fun _ => pure_noWriteOnSuccess _

/-- F07 コメント作成の原子性(decision 0002)。`createComment`は`requireAuth`→
    `findThread`(スレッド存在検査)→本文空検査、の順に検査を重ねるだけで、実際に
    `Db`を書き換える`set`(コメント追加)は全検査を通過した後の`tick`と合わせて
    最後に一度だけ実行される。`createThread_atomic`と同じ構造(検査だけが
    失敗しうる分岐で、検査を全て通過した後の末尾は失敗し得ない)。 -/
theorem createComment_atomic (sid : SessionId) (tid : ThreadId) (b : String) :
    NoWriteOnError (createComment sid tid b) := by
  unfold createComment
  apply bind_noWriteOnError (requireAuth_noWriteOnSuccess sid)
  intro _
  apply bind_noWriteOnError (findThread_noWriteOnSuccess tid)
  intro _
  apply bind_noWriteOnError (ensure_noWriteOnSuccess _ _)
  intro _
  -- 残るは`tick`・`get`・`set`・`pure`の末尾で、これは絶対に失敗しない
  -- (NoWriteOnErrorは空虚に真)。createThread_atomicの末尾と同じ理由付け。
  intro s0 e s' h
  simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.set, Action.pure,
    Action.modify, Action.get, tick] at h
  injection h with h1 _
  injection h1

/-- F06 スレッド削除の原子性(decision 0002)。`deleteThread`は`requireAuth`→
    `findThread`(スレッド存在検査)→作成者検査(`forbidden`)→`commentsOf`
    (コメント一覧の取得)→コメント0件検査(`threadHasComments`)、の順に検査を
    重ねるだけで、実際に`Db`を書き換える`modify`(スレッド除去)は全検査を
    通過した後に一度だけ実行される。`deleteComment_atomic`と同じ構造だが、
    `commentsOf`ぶんの`bind`が1段多い。 -/
theorem deleteThread_atomic (sid : SessionId) (tid : ThreadId) :
    NoWriteOnError (deleteThread sid tid) := by
  unfold deleteThread
  apply bind_noWriteOnError (requireAuth_noWriteOnSuccess sid)
  intro _
  apply bind_noWriteOnError (findThread_noWriteOnSuccess tid)
  intro _
  apply bind_noWriteOnError (ensure_noWriteOnSuccess _ _)
  intro _
  apply bind_noWriteOnError (commentsOf_noWriteOnSuccess tid)
  intro _
  apply bind_noWriteOnError (ensure_noWriteOnSuccess _ _)
  intro _
  exact modify_noWriteOnError _

/-- `findComment`(`get`のあとに`liftOption`)は成功時に状態を変えない。
    `findThread_noWriteOnSuccess`と同じ形。`deleteComment_atomic`の証明で使う。 -/
theorem findComment_noWriteOnSuccess (cid : CommentId) : NoWriteOnSuccess (findComment cid) := by
  unfold findComment
  exact bind_noWriteOnSuccess get_noWriteOnSuccess fun _ => liftOption_noWriteOnSuccess _ _

/-- F08 コメント削除の原子性(decision 0002)。`deleteComment`は`requireAuth`→
    `findComment`(コメント存在検査)→作成者検査(`forbidden`)→未削除検査
    (`alreadyDeleted`)、の順に検査を重ねるだけで、実際に`Db`を書き換える`modify`
    (`deleted`を立てる)は全検査を通過した後に一度だけ実行される。`logout_atomic`と
    同じ構造(末尾が`modify`で、`modify`自体は絶対に失敗しないため
    `NoWriteOnError`は空虚に真になる)。 -/
theorem deleteComment_atomic (sid : SessionId) (cid : CommentId) :
    NoWriteOnError (deleteComment sid cid) := by
  unfold deleteComment
  apply bind_noWriteOnError (requireAuth_noWriteOnSuccess sid)
  intro _
  apply bind_noWriteOnError (findComment_noWriteOnSuccess cid)
  intro _
  apply bind_noWriteOnError (ensure_noWriteOnSuccess _ _)
  intro _
  apply bind_noWriteOnError (ensure_noWriteOnSuccess _ _)
  intro _
  exact modify_noWriteOnError _

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

/-- **`db`自体が既にID重複を持たない**（`Wf.threadIdsDistinct`相当）という前提から、
    同じidを持つ2要素は同一であることを引き出す。`thread_immutable`
    （下記、`ThreadTrackingInvariant`経由）と`createThread_does_not_modify_existing_threads`
    の両方が使う。反例: `threads := [⟨0,_,"a",_,_⟩, ⟨0,_,"z",_,_⟩]`のような
    不正な`db`では、2件とも`t.id = 0`だが内容が違うので
    「`t' ∈ db.threads`かつ`t'.id = t.id`なら`t' = t`」自体が最初から成り立たない
    （decision 0025）。`Wf`構造体を丸ごと要求せず、この証明に要る性質だけを
    局所的な仮定として取る。 -/
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

/-! #### F05スコープの補題: `createThread`は既存スレッドを変更しない (C-05 / AC05-4)

下記の`thread_immutable`は`Step`（F01〜F08全種）を跨ぐ一般形（F06セッションで証明済み、
`ThreadTrackingInvariant`経由）。ここでは**`createThread`という単一操作**に絞り、
C-05が要求する「作成後に他のスレッドの内容を書き換えない」を直接証明する。
`thread_immutable`は論理的にはこの補題を包含するが、F05セッション時点の意図
（単一操作への言明）と、実装側がこの形の言明をオラクルとして参照している経緯を
尊重し、`sorry`を含まない既存の証明はそのまま残す（削除しない。`comment_body_immutable`と
`createComment_does_not_modify_existing_comments`の関係と同じ扱い）。 -/

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

/-- `discard`(`runStep`が`register`/`login`/`createThread`/`createComment`を`Action Unit`へ
    包むのに使う)は`Functor.mapConst`の既定実装(`Function.comp map (Function.const _)`、
    `map`はさらに`Monad`の既定実装`fun f x => bind x (pure ∘ f)`)を通じて
    `Action.bind x (fun _ => Action.pure ())`へ definitionally reduce する
    (`instance : Monad Action`が`pure`/`bind`のみ与え、`map`/`mapConst`を上書きしていない
    ため)。`rfl`で閉じる。F08セッションで`runStep`越しの証明(下記)に`discard`が
    絡むようになり、初めて必要になった補題。 -/
theorem discard_eq {α : Type} (x : Action α) :
    discard x = Action.bind x (fun _ => Action.pure ()) := rfl

/-- `thread_immutable`の帰納法の核。「`t`と同じidを持つスレッドは常に`t`と完全に一致する」
    (`Comment`と違い`Thread`には`deleted`のような「同idのまま書き換わる」フィールドが無く、
    `deleteThread`は物理削除なので、生き残っているなら中身も同一 ―― `CommentTrackingInvariant`の
    `bodyMatch`のような3フィールド一致ではなく、レコード全体の一致`t' = t`で言える)を、
    それが成り立つために必要な整形式性(`nodup`/`fresh`)ごと1つの構造体にまとめ、`runStep`
    8種すべてで一括して保存できるようにする。`tidBound`（`t.id`は常に`db.nextThreadId`未満）は
    `CommentTrackingInvariant.cidBound`と同じ理由の追加フィールド ―― `createThread`が
    新規スレッドに`db.nextThreadId`(現在値)を新規idとして採番する際、それが`t.id`と
    衝突しないことを示すには「`t.id`が現在の`nextThreadId`を常に下回り続ける」ことが要る。 -/
structure ThreadTrackingInvariant (db : Db) (t : Thread) : Prop where
  nodup : (db.threads.map (·.id)).Nodup
  fresh : ∀ x ∈ db.threads, x.id < db.nextThreadId
  tidBound : t.id < db.nextThreadId
  /-- `matches`という名前は`matches`(パターン照合の項レベル構文)という予約語と
      衝突するため使えず、`eqOfSameId`とした。 -/
  eqOfSameId : ∀ t' ∈ db.threads, t'.id = t.id → t' = t

/-- 帰納法の基底部。`thread_immutable`の引数(`hnodup`/`hfresh`/`h`)から
    `ThreadTrackingInvariant`を組み立てる。`eqOfSameId`は`nodup_map_eq_of_mem`
    (`t' ∈ db.threads`かつ`t'.id = t.id`なら`t' = t`)から自明に従う。 -/
theorem threadTrackingInvariant_base (db : Db) (t : Thread)
    (hnodup : (db.threads.map (·.id)).Nodup)
    (hfresh : ∀ x ∈ db.threads, x.id < db.nextThreadId)
    (h : t ∈ db.threads) :
    ThreadTrackingInvariant db t where
  nodup := hnodup
  fresh := hfresh
  tidBound := hfresh t h
  eqOfSameId := fun t' ht' heq => nodup_map_eq_of_mem hnodup ht' h heq

/-- 帰納法の1手。`Step`8種のうち`threads`/`nextThreadId`に一切触れない6種
    (`register`/`login`/`logout`/`updateDisplayName`/`createComment`/`deleteComment`)は
    各分岐を`simp`で具体形へ落とし、変わらない`db.threads`/`db.nextThreadId`そのものを
    再利用するだけで閉じる。`createThread`(F05)は末尾への1件追加、`deleteThread`(F06)は
    `List.filter`による除去のみで、いずれも生き残った要素のidや中身を変えない ―― この
    2操作だけが実質的な議論を要する。 -/
theorem runStep_preserves_threadTrackingInvariant (st : Step) (db : Db) (t : Thread)
    (h : ThreadTrackingInvariant db t) :
    ThreadTrackingInvariant (runStep st db).2 t := by
  obtain ⟨hnodup, hfresh, htidBound, hmatches⟩ := h
  cases st with
  | register u p d =>
    refine ⟨?_, ?_, ?_, ?_⟩ <;>
      · simp only [runStep, discard_eq]
        unfold register
        cases hwf : Validation.uniqueIdWellFormed u with
        | false =>
          simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
            Action.fail, hwf, ensure_false_eq]
          first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
        | true =>
          cases hweak : (Validation.passwordWeaknesses p).isEmpty with
          | false =>
            simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
              Action.fail, hwf, hweak, ensure_true_eq, ensure_false_eq]
            first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
          | true =>
            cases hdn : Validation.displayNameFailure d with
            | some v =>
              simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
                Action.fail, hwf, hweak, hdn, ensure_true_eq, Action.guardNone]
              first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
            | none =>
              cases hex : db.users.find? (·.uniqueId = u) with
              | some usr =>
                simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
                  Action.fail, hwf, hweak, hdn, hex, ensure_true_eq, Action.guardNone,
                  findUserByUniqueId]
                first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
              | none =>
                simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get,
                  Action.set, Action.pure, hwf, hweak, hdn, hex, ensure_true_eq,
                  Action.guardNone, findUserByUniqueId]
                first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
  | login u p =>
    refine ⟨?_, ?_, ?_, ?_⟩ <;>
      · simp only [runStep, discard_eq]
        unfold login
        cases hu : db.users.find? (·.uniqueId = u) with
        | none =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, hu]
          first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
        | some usr =>
          cases hpw : decide (usr.passwordHash = hashPassword p) with
          | false =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, hu, hpw, ensure_false_eq]
            first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
          | true =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, hu, hpw, ensure_true_eq]
            first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
  | logout sid =>
    refine ⟨?_, ?_, ?_, ?_⟩ <;>
      · simp only [runStep]
        unfold logout
        cases hs : db.sessions.find? (·.id = sid) with
        | none =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, hs]
          first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
        | some sess =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, hs]
          first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
  | updateDisplayName sid n =>
    refine ⟨?_, ?_, ?_, ?_⟩ <;>
      · simp only [runStep]
        unfold updateDisplayName
        cases hs : db.sessions.find? (·.id = sid) with
        | none =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, hs]
          first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
        | some sess =>
          cases hdn : Validation.displayNameFailure n with
          | some v =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, hs, hdn]
            first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
          | none =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, hs, hdn]
            first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
  | createThread sid title body =>
    simp only [runStep, discard_eq]
    unfold createThread
    cases hs : db.sessions.find? (·.id = sid) with
    | none =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, Action.modify, tick, requireAuth, hs]
      exact ⟨hnodup, hfresh, htidBound, hmatches⟩
    | some sess =>
      cases ht : Validation.nonEmptyText title with
      | false =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, Action.modify, tick, requireAuth, hs, ht, ensure_false_eq]
        exact ⟨hnodup, hfresh, htidBound, hmatches⟩
      | true =>
        cases hb : Validation.nonEmptyText body with
        | false =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, tick, requireAuth, hs, ht, hb,
            ensure_true_eq, ensure_false_eq]
          exact ⟨hnodup, hfresh, htidBound, hmatches⟩
        | true =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, tick, requireAuth, hs, ht, hb,
            ensure_true_eq]
          -- 成功: threads' = db.threads ++ [new]、nextThreadId' = db.nextThreadId + 1、
          -- new.id = db.nextThreadId(旧)。`hfresh`より旧`nextThreadId`は`db.threads`の
          -- 全idより大きいので、新規idは重複しない(nodup維持)。`t`自身は新規スレッドとid一致
          -- しない(`htidBound`)ので、`eqOfSameId`は元のリストへの`hmatches`をそのまま使える。
          refine ⟨?_, ?_, ?_, ?_⟩
          · rw [List.map_append, List.map_singleton]
            refine List.nodup_append.mpr ⟨hnodup, ?_, ?_⟩
            · exact List.nodup_cons.mpr ⟨List.not_mem_nil, List.nodup_nil⟩
            intro a ha b' hb'
            simp only [List.mem_singleton] at hb'
            rw [List.mem_map] at ha
            obtain ⟨x, hx, hxa⟩ := ha
            have hlt : x.id < db.nextThreadId := hfresh x hx
            rw [← hxa, hb']
            exact Nat.ne_of_lt hlt
          · intro x hx
            rw [List.mem_append, List.mem_singleton] at hx
            rcases hx with hx | hx
            · exact Nat.lt_of_lt_of_le (hfresh x hx) (Nat.le_succ _)
            · subst hx; exact Nat.lt_succ_self _
          · exact Nat.lt_of_lt_of_le htidBound (Nat.le_succ _)
          · intro t'' ht'' heq
            rw [List.mem_append, List.mem_singleton] at ht''
            rcases ht'' with ht'' | ht''
            · exact hmatches t'' ht'' heq
            · exfalso
              subst ht''
              exact Nat.lt_irrefl _ (heq ▸ htidBound)
  | deleteThread sid tid =>
    simp only [runStep]
    unfold deleteThread
    cases hs : db.sessions.find? (·.id = sid) with
    | none =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, Action.modify, requireAuth, findThread, Action.liftOption,
        Op.commentsOf, hs]
      exact ⟨hnodup, hfresh, htidBound, hmatches⟩
    | some sess =>
      cases ht : db.threads.find? (·.id = tid) with
      | none =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, Action.modify, requireAuth, findThread, Action.liftOption,
          Op.commentsOf, hs, ht]
        exact ⟨hnodup, hfresh, htidBound, hmatches⟩
      | some thr =>
        cases how : decide (thr.authorId = sess.userId) with
        | false =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, findThread, Action.liftOption,
            Op.commentsOf, hs, ht, how, ensure_false_eq]
          exact ⟨hnodup, hfresh, htidBound, hmatches⟩
        | true =>
          cases hce : (db.comments.filter (·.threadId = tid)).isEmpty with
          | false =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, findThread,
              Action.liftOption, Op.commentsOf, hs, ht, how, hce, ensure_true_eq, ensure_false_eq]
            exact ⟨hnodup, hfresh, htidBound, hmatches⟩
          | true =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, findThread,
              Action.liftOption, Op.commentsOf, hs, ht, how, hce, ensure_true_eq]
            -- 成功: threads' = db.threads.filter (·.id ≠ tid)。フィルタは要素を除くだけなので、
            -- `nodup`は`nodup_map_filter`、`fresh`/`eqOfSameId`は「フィルタ後のリストの要素は
            -- 元のリストの要素でもある」(`List.mem_filter`)ことから従う。`tidBound`は`t`自身は
            -- 触れていないのでそのまま。
            refine ⟨?_, ?_, htidBound, ?_⟩
            · exact nodup_map_filter _ _ _ hnodup
            · intro x hx
              exact hfresh x (List.mem_filter.mp hx).1
            · intro t' ht' heq
              exact hmatches t' (List.mem_filter.mp ht').1 heq
  | createComment sid tid b =>
    refine ⟨?_, ?_, ?_, ?_⟩ <;>
      · simp only [runStep, discard_eq]
        unfold createComment
        cases hs : db.sessions.find? (·.id = sid) with
        | none =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, tick, requireAuth, findThread, Action.liftOption, hs]
          first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
        | some sess =>
          cases ht : db.threads.find? (·.id = tid) with
          | none =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, tick, requireAuth, findThread, Action.liftOption, hs, ht]
            first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
          | some thr =>
            cases hb : Validation.nonEmptyText b with
            | false =>
              simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
                Action.pure, Action.fail, tick, requireAuth, findThread, Action.liftOption, hs,
                ht, hb, ensure_false_eq]
              first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
            | true =>
              simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
                Action.pure, Action.fail, Action.modify, tick, requireAuth, findThread,
                Action.liftOption, hs, ht, hb, ensure_true_eq]
              first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
  | deleteComment sid dcid =>
    refine ⟨?_, ?_, ?_, ?_⟩ <;>
      · simp only [runStep]
        unfold deleteComment
        cases hs : db.sessions.find? (·.id = sid) with
        | none =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, findComment,
            Action.liftOption, hs]
          first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
        | some sess =>
          cases hf : db.comments.find? (·.id = dcid) with
          | none =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, findComment,
              Action.liftOption, hs, hf]
            first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
          | some target =>
            cases how : decide (target.authorId = sess.userId) with
            | false =>
              simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
                Action.pure, Action.fail, Action.modify, requireAuth, findComment,
                Action.liftOption, hs, hf, how, ensure_false_eq]
              first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
            | true =>
              cases hdel : !target.deleted with
              | false =>
                simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get,
                  Action.set, Action.pure, Action.fail, Action.modify, requireAuth, findComment,
                  Action.liftOption, hs, hf, how, hdel, ensure_true_eq, ensure_false_eq]
                first | exact hnodup | exact hfresh | exact htidBound | exact hmatches
              | true =>
                simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get,
                  Action.set, Action.pure, Action.fail, Action.modify, requireAuth, findComment,
                  Action.liftOption, hs, hf, how, hdel, ensure_true_eq]
                first | exact hnodup | exact hfresh | exact htidBound | exact hmatches

/-- 帰納法本体。`comment_body_immutable`の`runAll_preserves_commentTrackingInvariant`と
    同形。 -/
theorem runAll_preserves_threadTrackingInvariant (steps : List Step) (db : Db) (t : Thread)
    (h : ThreadTrackingInvariant db t) :
    ThreadTrackingInvariant (runAll steps db) t := by
  induction steps generalizing db with
  | nil => exact h
  | cons st rest ih =>
    unfold runAll
    exact ih (runStep st db).2 (runStep_preserves_threadTrackingInvariant st db t h)

/-- C-05: スレッド本体は不変。残っているなら中身は同一。

    **`hnodup`/`hfresh`は必須の仮定**であり、外すと言明そのものが**偽**になる
    (decision 0025)。反例: `threads := [⟨0,_,"a",_,_⟩, ⟨0,_,"z",_,_⟩]`のように
    idが重複した整形式でない`db`では、`steps = []`だけで
    「`t' ∈ db.threads`かつ`t'.id = t.id`なら`t' = t`」が成り立たない。
    当初この2仮定を欠いた形で書かれていたが、それは「後で証明する」ことが
    原理的に不可能な偽の命題だったため、`createThread_does_not_modify_existing_threads`
    (F05スコープの単一操作版)が採ったのと同じ局所仮定を付けて**真の命題**へ直した。
    `Wf`構造体を丸ごと要求しないのも同じ理由 ―― `Wf`全体の保存はF01〜F03の時点でも
    未証明であり、この言明が実際に要るのは`threadIdsDistinct`/`nextIdsFresh`相当の
    2性質だけである。

    **F06セッションでこの一般形を証明した。** `threads`/`nextThreadId`フィールドに
    触れる`Step`は`createThread`(F05)と`deleteThread`(F06)のみで、両方のRust実装が
    揃ったため decision 0025のスコープ限定(実装未着手の操作を理由に一般形を`sorry`のまま
    残す)がもう適用されない。戦略は`comment_body_immutable`(F08セッション)と全く同じ ――
    `ThreadTrackingInvariant`（上記）を`Step`8種すべてについて保存されることを示し、
    `runAll`（リストの畳み込み）へ帰納法で持ち上げる。`Comment`版との違いは、`Thread`には
    `deleted`のような「同idのまま書き換わる」フィールドが無い(`deleteThread`は物理削除)ため、
    `eqOfSameId`が(`bodyMatch`のような3フィールド一致でなく)完全なレコード一致(`t' = t`)で
    言えること。 -/
theorem thread_immutable (db : Db) (steps : List Step) (t : Thread)
    (hnodup : (db.threads.map (·.id)).Nodup)
    (hfresh : ∀ t ∈ db.threads, t.id < db.nextThreadId)
    (h : t ∈ db.threads) :
    ∀ t' ∈ (runAll steps db).threads, t'.id = t.id → t' = t :=
  (runAll_preserves_threadTrackingInvariant steps db t
    (threadTrackingInvariant_base db t hnodup hfresh h)).eqOfSameId

/-- `comment_body_immutable`の帰納法の核。「`c`と同じidを持つコメントは常に`c`と
    body/authorId/createdAtが一致する」を、それが成り立つために必要な整形式性
    (`nodup`/`fresh`)ごと1つの構造体にまとめ、`runStep`8種すべてで一括して
    保存できるようにする。`cidBound`（`c.id`は常に`db.nextCommentId`未満）は
    `hfresh`単体からは導けない追加のフィールド ―― `createComment`が新規コメントに
    `db.nextCommentId`(現在値)を新規idとして採番する際、それが`c.id`と衝突しないことを
    示すには「`c`自身が(過去のどこかの時点の)`db.comments`に属していた」ことではなく
    「`c.id`が現在の`nextCommentId`を常に下回り続ける」ことが要る。`nextCommentId`は
    単調増加なので、一度`cidBound`が成り立てば`createComment`後も成り立ち続ける
    （帰納法のこの一手のために持たせている）。 -/
structure CommentTrackingInvariant (db : Db) (c : Comment) : Prop where
  nodup : (db.comments.map (·.id)).Nodup
  fresh : ∀ x ∈ db.comments, x.id < db.nextCommentId
  cidBound : c.id < db.nextCommentId
  bodyMatch : ∀ c' ∈ db.comments, c'.id = c.id →
    c'.body = c.body ∧ c'.authorId = c.authorId ∧ c'.createdAt = c.createdAt

/-- 帰納法の基底部。`comment_body_immutable`の引数(`hnodup`/`hfresh`/`h`)から
    `CommentTrackingInvariant`を組み立てる。`bodyMatch`は`nodup_map_eq_of_mem`
    (`c' ∈ db.comments`かつ`c'.id = c.id`なら`c' = c`)から自明に従う。 -/
theorem commentTrackingInvariant_base (db : Db) (c : Comment)
    (hnodup : (db.comments.map (·.id)).Nodup)
    (hfresh : ∀ x ∈ db.comments, x.id < db.nextCommentId)
    (h : c ∈ db.comments) :
    CommentTrackingInvariant db c where
  nodup := hnodup
  fresh := hfresh
  cidBound := hfresh c h
  bodyMatch := by
    intro c' hc' heq
    have : c' = c := nodup_map_eq_of_mem hnodup hc' h heq
    subst this
    exact ⟨rfl, rfl, rfl⟩

/-- 帰納法の1手。`Step`8種のうち`comments`/`nextCommentId`に一切触れない6種
    (`register`/`login`/`logout`/`updateDisplayName`/`createThread`/`deleteThread`)は
    各分岐を`simp`で具体形へ落とし、変わらない`db.comments`/`db.nextCommentId`
    そのものを再利用するだけで閉じる。`createComment`(F07)は末尾への1件追加、
    `deleteComment`(F08)は`List.map`による`deleted`のみの書き換えで、いずれも
    既存要素のidを変えない ―― この2操作だけが実質的な議論を要する。 -/
theorem runStep_preserves_commentTrackingInvariant (st : Step) (db : Db) (c : Comment)
    (h : CommentTrackingInvariant db c) :
    CommentTrackingInvariant (runStep st db).2 c := by
  obtain ⟨hnodup, hfresh, hcidBound, hmatches⟩ := h
  cases st with
  | register u p d =>
    refine ⟨?_, ?_, ?_, ?_⟩ <;>
      · simp only [runStep, discard_eq]
        unfold register
        cases hwf : Validation.uniqueIdWellFormed u with
        | false =>
          simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
            Action.fail, hwf, ensure_false_eq]
          first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
        | true =>
          cases hweak : (Validation.passwordWeaknesses p).isEmpty with
          | false =>
            simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
              Action.fail, hwf, hweak, ensure_true_eq, ensure_false_eq]
            first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
          | true =>
            cases hdn : Validation.displayNameFailure d with
            | some v =>
              simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
                Action.fail, hwf, hweak, hdn, ensure_true_eq, Action.guardNone]
              first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
            | none =>
              cases hex : db.users.find? (·.uniqueId = u) with
              | some usr =>
                simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
                  Action.fail, hwf, hweak, hdn, hex, ensure_true_eq, Action.guardNone,
                  findUserByUniqueId]
                first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
              | none =>
                simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get,
                  Action.set, Action.pure, hwf, hweak, hdn, hex, ensure_true_eq,
                  Action.guardNone, findUserByUniqueId]
                first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
  | login u p =>
    refine ⟨?_, ?_, ?_, ?_⟩ <;>
      · simp only [runStep, discard_eq]
        unfold login
        cases hu : db.users.find? (·.uniqueId = u) with
        | none =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, hu]
          first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
        | some usr =>
          cases hpw : decide (usr.passwordHash = hashPassword p) with
          | false =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, hu, hpw, ensure_false_eq]
            first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
          | true =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, hu, hpw, ensure_true_eq]
            first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
  | logout sid =>
    refine ⟨?_, ?_, ?_, ?_⟩ <;>
      · simp only [runStep]
        unfold logout
        cases hs : db.sessions.find? (·.id = sid) with
        | none =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, hs]
          first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
        | some sess =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, hs]
          first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
  | updateDisplayName sid n =>
    refine ⟨?_, ?_, ?_, ?_⟩ <;>
      · simp only [runStep]
        unfold updateDisplayName
        cases hs : db.sessions.find? (·.id = sid) with
        | none =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, hs]
          first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
        | some sess =>
          cases hdn : Validation.displayNameFailure n with
          | some v =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, hs, hdn]
            first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
          | none =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, hs, hdn]
            first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
  | createThread sid t b =>
    refine ⟨?_, ?_, ?_, ?_⟩ <;>
      · simp only [runStep, discard_eq]
        unfold createThread
        cases hs : db.sessions.find? (·.id = sid) with
        | none =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, tick, requireAuth, hs]
          first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
        | some sess =>
          cases ht : Validation.nonEmptyText t with
          | false =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, tick, requireAuth, hs, ht,
              ensure_false_eq]
            first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
          | true =>
            cases hb : Validation.nonEmptyText b with
            | false =>
              simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
                Action.pure, Action.fail, Action.modify, tick, requireAuth, hs, ht, hb,
                ensure_true_eq, ensure_false_eq]
              first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
            | true =>
              simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
                Action.pure, Action.fail, Action.modify, tick, requireAuth, hs, ht, hb,
                ensure_true_eq]
              first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
  | deleteThread sid tid =>
    refine ⟨?_, ?_, ?_, ?_⟩ <;>
      · simp only [runStep]
        unfold deleteThread
        cases hs : db.sessions.find? (·.id = sid) with
        | none =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, findThread, Action.liftOption,
            Op.commentsOf, hs]
          first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
        | some sess =>
          cases ht : db.threads.find? (·.id = tid) with
          | none =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, findThread,
              Action.liftOption, Op.commentsOf, hs, ht]
            first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
          | some thr =>
            cases how : decide (thr.authorId = sess.userId) with
            | false =>
              simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
                Action.pure, Action.fail, Action.modify, requireAuth, findThread,
                Action.liftOption, Op.commentsOf, hs, ht, how, ensure_false_eq]
              first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
            | true =>
              cases hce : (db.comments.filter (·.threadId = tid)).isEmpty with
              | false =>
                simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get,
                  Action.set, Action.pure, Action.fail, Action.modify, requireAuth, findThread,
                  Action.liftOption, Op.commentsOf, hs, ht, how, hce, ensure_true_eq,
                  ensure_false_eq]
                first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
              | true =>
                simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get,
                  Action.set, Action.pure, Action.fail, Action.modify, requireAuth, findThread,
                  Action.liftOption, Op.commentsOf, hs, ht, how, hce, ensure_true_eq]
                first | exact hnodup | exact hfresh | exact hcidBound | exact hmatches
  | createComment sid tid b =>
    simp only [runStep, discard_eq]
    unfold createComment
    cases hs : db.sessions.find? (·.id = sid) with
    | none =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, tick, requireAuth, findThread, Action.liftOption, hs]
      exact ⟨hnodup, hfresh, hcidBound, hmatches⟩
    | some sess =>
      cases ht : db.threads.find? (·.id = tid) with
      | none =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, tick, requireAuth, findThread, Action.liftOption, hs, ht]
        exact ⟨hnodup, hfresh, hcidBound, hmatches⟩
      | some thr =>
        cases hb : Validation.nonEmptyText b with
        | false =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, tick, requireAuth, findThread, Action.liftOption, hs, ht,
            hb, ensure_false_eq]
          exact ⟨hnodup, hfresh, hcidBound, hmatches⟩
        | true =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, tick, requireAuth, findThread,
            Action.liftOption, hs, ht, hb, ensure_true_eq]
          -- 成功: comments' = db.comments ++ [new]、nextCommentId' = db.nextCommentId + 1、
          -- new.id = db.nextCommentId(旧)。`hfresh`より旧`nextCommentId`は
          -- `db.comments`の全idより大きいので、新規idは重複しない(nodup維持)。
          -- `omega`はこのファイルの`abbrev`ベースのID型(`CommentId`等)を
          -- `Nat`として認識できない既知の制約があるため、`Nat.ne_of_lt`等の
          -- 具体的な補題を直接使う(omegaに頼らない)。
          refine ⟨?_, ?_, ?_, ?_⟩
          · rw [List.map_append, List.map_singleton]
            refine List.nodup_append.mpr ⟨hnodup, ?_, ?_⟩
            · exact List.nodup_cons.mpr ⟨List.not_mem_nil, List.nodup_nil⟩
            intro a ha b' hb'
            simp only [List.mem_singleton] at hb'
            rw [List.mem_map] at ha
            obtain ⟨x, hx, hxa⟩ := ha
            have hlt : x.id < db.nextCommentId := hfresh x hx
            rw [← hxa, hb']
            exact Nat.ne_of_lt hlt
          · intro x hx
            rw [List.mem_append, List.mem_singleton] at hx
            rcases hx with hx | hx
            · exact Nat.lt_of_lt_of_le (hfresh x hx) (Nat.le_succ _)
            · subst hx; exact Nat.lt_succ_self _
          · exact Nat.lt_of_lt_of_le hcidBound (Nat.le_succ _)
          · intro c' hc' heq
            rw [List.mem_append, List.mem_singleton] at hc'
            rcases hc' with hc' | hc'
            · exact hmatches c' hc' heq
            · exfalso
              subst hc'
              exact Nat.lt_irrefl _ (heq ▸ hcidBound)
  | deleteComment sid dcid =>
    simp only [runStep]
    unfold deleteComment
    cases hs : db.sessions.find? (·.id = sid) with
    | none =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, Action.modify, requireAuth, findComment, Action.liftOption, hs]
      exact ⟨hnodup, hfresh, hcidBound, hmatches⟩
    | some sess =>
      cases hf : db.comments.find? (·.id = dcid) with
      | none =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, Action.modify, requireAuth, findComment, Action.liftOption,
          hs, hf]
        exact ⟨hnodup, hfresh, hcidBound, hmatches⟩
      | some target =>
        cases how : decide (target.authorId = sess.userId) with
        | false =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, findComment, Action.liftOption,
            hs, hf, how, ensure_false_eq]
          exact ⟨hnodup, hfresh, hcidBound, hmatches⟩
        | true =>
          cases hdel : !target.deleted with
          | false =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, findComment,
              Action.liftOption, hs, hf, how, hdel, ensure_true_eq, ensure_false_eq]
            exact ⟨hnodup, hfresh, hcidBound, hmatches⟩
          | true =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, findComment,
              Action.liftOption, hs, hf, how, hdel, ensure_true_eq]
            -- 成功: comments' = db.comments.map(該当idのみdeleted:=trueに書き換え)。
            -- `List.map`はidを変えないので`nodup`/`fresh`はそのまま流用できる。
            refine ⟨?_, ?_, hcidBound, ?_⟩
            · rw [List.map_map]
              have heq : (Comment.id ∘ (fun x => if x.id = dcid then { x with deleted := true } else x))
                  = (Comment.id ·) := by
                funext x; simp only [Function.comp]; split <;> rfl
              rw [heq]
              exact hnodup
            · intro x hx
              rw [List.mem_map] at hx
              obtain ⟨y, hy, hxy⟩ := hx
              have hlt : y.id < db.nextCommentId := hfresh y hy
              split at hxy <;> (rw [← hxy]; simpa using hlt)
            · intro c' hc' heq
              rw [List.mem_map] at hc'
              obtain ⟨y, hy, hxy⟩ := hc'
              split at hxy
              · rw [← hxy] at heq
                have hm := hmatches y hy heq
                rw [← hxy]
                exact hm
              · rw [← hxy] at heq ⊢
                exact hmatches y hy heq

/-- 帰納法本体。`List.foldl`型の`runAll`を`Step`のリストについて帰納する。 -/
theorem runAll_preserves_commentTrackingInvariant (steps : List Step) (db : Db) (c : Comment)
    (h : CommentTrackingInvariant db c) :
    CommentTrackingInvariant (runAll steps db) c := by
  induction steps generalizing db with
  | nil => exact h
  | cons st rest ih =>
    unfold runAll
    exact ih (runStep st db).2 (runStep_preserves_commentTrackingInvariant st db c h)

/-- C-05: コメント本文と作成者・作成日時は不変（`deleted` のみ変化しうる）。

    **`hnodup`/`hfresh`は必須の仮定**であり、外すと言明そのものが**偽**になる
    (`thread_immutable`/decision 0025と全く同じ構造)。反例:
    `comments := [⟨0,_,_,"a",_,false⟩, ⟨0,_,_,"z",_,false⟩]`のようにidが重複した
    整形式でない`db`では、`steps = []`だけで
    「`c' ∈ db.comments`かつ`c'.id = c.id`なら本文が一致する」が成り立たない
    (F07実装セッションでこの反例を`native_decide`で実際に構成し確認した)。
    当初この2仮定を欠いた形で書かれていたが、それは「後で証明する」ことが
    原理的に不可能な偽の命題だったため、`createComment_does_not_modify_existing_comments`
    (下記、F07スコープの単一操作版)が採ったのと同じ局所仮定を付けて**真の命題**へ直した。

    **F08セッションでこの一般形を証明した。** `comments`フィールドに触れる`Step`は
    `createComment`(F07)と`deleteComment`(F08)のみで、両方のRust実装が揃ったため
    decision 0025のスコープ限定(実装未着手の操作を理由に一般形を`sorry`のまま残す)が
    もう適用されない。戦略は`CommentTrackingInvariant`（上記）を`Step`8種すべてに
    ついて保存されることを示し、`runAll`（リストの畳み込み）へ帰納法で持ち上げる。 -/
theorem comment_body_immutable (db : Db) (steps : List Step) (c : Comment)
    (hnodup : (db.comments.map (·.id)).Nodup)
    (hfresh : ∀ c ∈ db.comments, c.id < db.nextCommentId)
    (h : c ∈ db.comments) :
    ∀ c' ∈ (runAll steps db).comments, c'.id = c.id →
      c'.body = c.body ∧ c'.authorId = c.authorId ∧ c'.createdAt = c.createdAt :=
  (runAll_preserves_commentTrackingInvariant steps db c
    (commentTrackingInvariant_base db c hnodup hfresh h)).bodyMatch

/-! #### F07スコープの補題: `createComment`は既存コメントを変更しない (C-05 / AC07-4)

上の`comment_body_immutable`はF08セッションで`Step`全種にわたる一般形として証明済みに
なったため、以下の`createComment_does_not_modify_existing_comments`は論理的には
その特殊ケース(`steps := [.createComment sid tid body]`)で置き換えられる。ただし
F07セッション時点の意図(単一操作への言明)をそのまま残す記録的価値と、実装側の
`createComment_atomic`/このテストが直接この形の言明をオラクルとして参照している
経緯を尊重し、`sorry`を含まない既存の証明はそのまま残す(削除しない)。 -/

/-- `comment_body_immutable`と同様に、**`db`自体が既にID重複を持たない**
    （`Wf.commentIdsDistinct`相当）という前提が要る。`Wf`構造体を丸ごと要求せず、
    この証明に要る2性質だけを局所的な仮定として取る(`nodup_map_eq_of_mem`と同じ方針)。 -/
theorem createComment_does_not_modify_existing_comments
    (sid : SessionId) (tid : ThreadId) (body : String) (db : Db)
    (hnodup : (db.comments.map (·.id)).Nodup)
    (hfresh : ∀ c ∈ db.comments, c.id < db.nextCommentId)
    (c : Comment) (h : c ∈ db.comments) :
    ∀ c' ∈ ((createComment sid tid body) db).2.comments, c'.id = c.id →
      c'.body = c.body ∧ c'.authorId = c.authorId ∧ c'.createdAt = c.createdAt := by
  intro c' hc' hid
  unfold createComment at hc'
  -- `requireAuth sid db`で場合分け。未認証なら状態は書き変わらず`db`のまま。
  cases hsess : db.sessions.find? (·.id = sid) with
  | none =>
    simp only [bind, Bind.bind, Action.bind, Action.get, Action.fail, requireAuth, hsess] at hc'
    have heq := nodup_map_eq_of_mem hnodup hc' h hid; subst heq; exact ⟨rfl, rfl, rfl⟩
  | some sess =>
    -- `findThread tid db`で場合分け。スレッドが無ければ状態は書き変わらず`db`のまま。
    cases hthread : db.threads.find? (·.id = tid) with
    | none =>
      simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
        Action.fail, requireAuth, hsess, findThread, Action.liftOption, hthread] at hc'
      have heq := nodup_map_eq_of_mem hnodup hc' h hid; subst heq; exact ⟨rfl, rfl, rfl⟩
    | some thr =>
      -- 本文の空検査で場合分け。空なら状態は書き変わらず`db`のまま。
      cases hbody : Validation.nonEmptyText body with
      | false =>
        simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
          Action.fail, requireAuth, hsess, findThread, Action.liftOption, hthread, hbody,
          ensure_false_eq] at hc'
        have heq := nodup_map_eq_of_mem hnodup hc' h hid; subst heq; exact ⟨rfl, rfl, rfl⟩
      | true =>
        -- ここまで来れば必ず成功し、`db.comments ++ [newComment]`が書き込まれる。
        -- `c'`は元のリストにあったか、新規追加分かのいずれか。
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.modify, tick, requireAuth, hsess, findThread, Action.liftOption,
          hthread, hbody, ensure_true_eq] at hc'
        rw [List.mem_append, List.mem_singleton] at hc'
        rcases hc' with hc' | hc'
        · have heq := nodup_map_eq_of_mem hnodup hc' h hid; subst heq; exact ⟨rfl, rfl, rfl⟩
        · -- 新規コメントは`id := db.nextCommentId`。`hfresh`よりこれは`c.id`と一致しない。
          exfalso
          have hlt : c.id < db.nextCommentId := hfresh c h
          have heq : db.nextCommentId = c.id := by rw [hc'] at hid; exact hid
          exact Nat.lt_irrefl c.id (heq ▸ hlt)

/-- C-07 / C-08: 論理削除は不可逆かつ非破壊。一度立った `deleted` は下りず、
    行そのものも消えない。

    `deleteComment`(F08)の実装により、前提`c.deleted = true`を満たす具体例が
    実際に作れるようになったため、このセッションで証明する。`comment_body_immutable`と
    同じ帰納法の骨格 ―― 「`cid`のコメントが削除済みとして存在する」という性質
    (`CommentDeletedExists`)を`Step`8種すべてで保存されることを示し、`runAll`へ
    帰納法で持ち上げる ―― だが、こちらは存在命題(∃)なので`Wf`相当の局所仮定
    (`nodup`/`fresh`)が要らない分`comment_body_immutable`より単純。 -/
def CommentDeletedExists (db : Db) (cid : CommentId) : Prop :=
  ∃ c ∈ db.comments, c.id = cid ∧ c.deleted = true

/-- 帰納法の1手。6種の「`comments`に触れない`Step`」は目的の comment が
    同じリストに残ることを再利用するだけ。`createComment`は末尾への追加
    (`List.mem_append_left`で残存を示す)、`deleteComment`は`List.map`
    (該当id以外は不変、該当idなら`deleted`が`true`のまま/`true`になる
    ―― どちらの分岐でも目的の性質は保たれる)。 -/
theorem runStep_preserves_commentDeletedExists (st : Step) (db : Db) (cid : CommentId)
    (h : CommentDeletedExists db cid) :
    CommentDeletedExists (runStep st db).2 cid := by
  obtain ⟨c, hc, hcid, hcd⟩ := h
  cases st with
  | register u p d =>
    refine ⟨c, ?_, hcid, hcd⟩
    simp only [runStep, discard_eq]
    unfold register
    cases hwf : Validation.uniqueIdWellFormed u with
    | false =>
      simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
        Action.fail, hwf, ensure_false_eq]
      exact hc
    | true =>
      cases hweak : (Validation.passwordWeaknesses p).isEmpty with
      | false =>
        simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
          Action.fail, hwf, hweak, ensure_true_eq, ensure_false_eq]
        exact hc
      | true =>
        cases hdn : Validation.displayNameFailure d with
        | some v =>
          simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
            Action.fail, hwf, hweak, hdn, ensure_true_eq, Action.guardNone]
          exact hc
        | none =>
          cases hex : db.users.find? (·.uniqueId = u) with
          | some usr =>
            simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
              Action.fail, hwf, hweak, hdn, hex, ensure_true_eq, Action.guardNone,
              findUserByUniqueId]
            exact hc
          | none =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, hwf, hweak, hdn, hex, ensure_true_eq, Action.guardNone,
              findUserByUniqueId]
            exact hc
  | login u p =>
    refine ⟨c, ?_, hcid, hcd⟩
    simp only [runStep, discard_eq]
    unfold login
    cases hu : db.users.find? (·.uniqueId = u) with
    | none =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, hu]
      exact hc
    | some usr =>
      cases hpw : decide (usr.passwordHash = hashPassword p) with
      | false =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, hu, hpw, ensure_false_eq]
        exact hc
      | true =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, hu, hpw, ensure_true_eq]
        exact hc
  | logout sid =>
    refine ⟨c, ?_, hcid, hcd⟩
    simp only [runStep]
    unfold logout
    cases hs : db.sessions.find? (·.id = sid) with
    | none =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, Action.modify, requireAuth, hs]
      exact hc
    | some sess =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, Action.modify, requireAuth, hs]
      exact hc
  | updateDisplayName sid n =>
    refine ⟨c, ?_, hcid, hcd⟩
    simp only [runStep]
    unfold updateDisplayName
    cases hs : db.sessions.find? (·.id = sid) with
    | none =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, Action.modify, requireAuth, hs]
      exact hc
    | some sess =>
      cases hdn : Validation.displayNameFailure n with
      | some v =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, Action.modify, requireAuth, hs, hdn]
        exact hc
      | none =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, Action.modify, requireAuth, hs, hdn]
        exact hc
  | createThread sid t b =>
    refine ⟨c, ?_, hcid, hcd⟩
    simp only [runStep, discard_eq]
    unfold createThread
    cases hs : db.sessions.find? (·.id = sid) with
    | none =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, Action.modify, tick, requireAuth, hs]
      exact hc
    | some sess =>
      cases ht : Validation.nonEmptyText t with
      | false =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, Action.modify, tick, requireAuth, hs, ht, ensure_false_eq]
        exact hc
      | true =>
        cases hb : Validation.nonEmptyText b with
        | false =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, tick, requireAuth, hs, ht, hb,
            ensure_true_eq, ensure_false_eq]
          exact hc
        | true =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, tick, requireAuth, hs, ht, hb,
            ensure_true_eq]
          exact hc
  | deleteThread sid tid =>
    refine ⟨c, ?_, hcid, hcd⟩
    simp only [runStep]
    unfold deleteThread
    cases hs : db.sessions.find? (·.id = sid) with
    | none =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, Action.modify, requireAuth, findThread, Action.liftOption,
        Op.commentsOf, hs]
      exact hc
    | some sess =>
      cases ht : db.threads.find? (·.id = tid) with
      | none =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, Action.modify, requireAuth, findThread, Action.liftOption,
          Op.commentsOf, hs, ht]
        exact hc
      | some thr =>
        cases how : decide (thr.authorId = sess.userId) with
        | false =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, findThread, Action.liftOption,
            Op.commentsOf, hs, ht, how, ensure_false_eq]
          exact hc
        | true =>
          cases hce : (db.comments.filter (·.threadId = tid)).isEmpty with
          | false =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, findThread,
              Action.liftOption, Op.commentsOf, hs, ht, how, hce, ensure_true_eq, ensure_false_eq]
            exact hc
          | true =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, findThread,
              Action.liftOption, Op.commentsOf, hs, ht, how, hce, ensure_true_eq]
            exact hc
  | createComment sid tid b =>
    simp only [runStep, discard_eq]
    unfold createComment
    cases hs : db.sessions.find? (·.id = sid) with
    | none =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, tick, requireAuth, findThread, Action.liftOption, hs]
      exact ⟨c, hc, hcid, hcd⟩
    | some sess =>
      cases ht : db.threads.find? (·.id = tid) with
      | none =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, tick, requireAuth, findThread, Action.liftOption, hs, ht]
        exact ⟨c, hc, hcid, hcd⟩
      | some thr =>
        cases hb : Validation.nonEmptyText b with
        | false =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, tick, requireAuth, findThread, Action.liftOption, hs, ht,
            hb, ensure_false_eq]
          exact ⟨c, hc, hcid, hcd⟩
        | true =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, tick, requireAuth, findThread, Action.liftOption, hs, ht,
            hb, ensure_true_eq]
          exact ⟨c, List.mem_append_left _ hc, hcid, hcd⟩
  | deleteComment sid dcid =>
    simp only [runStep]
    unfold deleteComment
    cases hs : db.sessions.find? (·.id = sid) with
    | none =>
      simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
        Action.pure, Action.fail, Action.modify, requireAuth, findComment, Action.liftOption, hs]
      exact ⟨c, hc, hcid, hcd⟩
    | some sess =>
      cases hf : db.comments.find? (·.id = dcid) with
      | none =>
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.fail, Action.modify, requireAuth, findComment, Action.liftOption,
          hs, hf]
        exact ⟨c, hc, hcid, hcd⟩
      | some target =>
        cases how : decide (target.authorId = sess.userId) with
        | false =>
          simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
            Action.pure, Action.fail, Action.modify, requireAuth, findComment, Action.liftOption,
            hs, hf, how, ensure_false_eq]
          exact ⟨c, hc, hcid, hcd⟩
        | true =>
          cases hdel : !target.deleted with
          | false =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, findComment,
              Action.liftOption, hs, hf, how, hdel, ensure_true_eq, ensure_false_eq]
            exact ⟨c, hc, hcid, hcd⟩
          | true =>
            simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
              Action.pure, Action.fail, Action.modify, requireAuth, findComment,
              Action.liftOption, hs, hf, how, hdel, ensure_true_eq]
            -- 削除対象自身が目的のcommentなら、`deleted`は(既にtrueだったか、
            -- ここで新たにtrueになるかのいずれかで)trueのまま。対象でなければ不変。
            refine ⟨if c.id = dcid then { c with deleted := true } else c, ?_, ?_, ?_⟩
            · exact List.mem_map_of_mem hc
            · split <;> exact hcid
            · split <;> first | rfl | exact hcd

/-- 帰納法本体。`comment_body_immutable`の`runAll_preserves_commentTrackingInvariant`と
    同形。 -/
theorem runAll_preserves_commentDeletedExists (steps : List Step) (db : Db) (cid : CommentId)
    (h : CommentDeletedExists db cid) :
    CommentDeletedExists (runAll steps db) cid := by
  induction steps generalizing db with
  | nil => exact h
  | cons st rest ih =>
    unfold runAll
    exact ih (runStep st db).2 (runStep_preserves_commentDeletedExists st db cid h)

theorem deletion_irreversible (db : Db) (steps : List Step) (c : Comment)
    (h : c ∈ db.comments) (hd : c.deleted = true) :
    ∃ c' ∈ (runAll steps db).comments, c'.id = c.id ∧ c'.deleted = true :=
  runAll_preserves_commentDeletedExists steps db c.id ⟨c, h, rfl, hd⟩

/-! ### 5. スレッド削除の二重条件 (C-06 / AC06-1〜3) -/

/-- C-06: 作成者本人でなければ、コメント件数によらずスレッド削除は拒否される。
    `deleteThread_blocked_by_any_comment`の姉妹補題で、`ensure`の分岐が
    コメント0件検査ではなく作成者検査で先に失敗する側。`hown'`を`false`側で
    立てる点だけが唯一の違い(`ensure_false_eq`を使う)。 -/
theorem deleteThread_needs_owner (db : Db) (sid : SessionId) (tid : ThreadId)
    (uid : UserId) (t : Thread)
    (hs : db.sessions.find? (·.id = sid) = some ⟨sid, uid⟩)
    (ht : db.threads.find? (·.id = tid) = some t)
    (hne : t.authorId ≠ uid) :
    (deleteThread sid tid) db = (.error .forbidden, db) := by
  have hown' : decide (t.authorId = uid) = false := by simp [hne]
  unfold deleteThread
  simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
    Action.pure, Action.fail, Action.modify, requireAuth, findThread, Action.liftOption,
    Op.commentsOf, hs, ht, hown', ensure_false_eq]

/-- C-06: コメントが1件でも紐づくスレッドは削除できない。認可（作成者本人）を
    通った上でなお `.threadHasComments` で失敗し、状態は変わらない。
    `deleteThread_blocked_by_deleted_comment`（AC06-2）はこの補題の
    `c.deleted = true` への特殊化として得る。

    restate後の言明が真であることの確認としてF06を待たずに証明した（当初は
    証明本体をF06送りにする想定だったが、`ensure cs.isEmpty`が失敗する経路を
    辿るだけで、既存の補題群で閉じた）。要点は`commentsOf`（`Op.lean`）が
    `deleted`で絞らず全件を`filter`することで、削除済み・未削除のどちらでも
    同じように`isEmpty`が偽になる。`Bbs.Scenario`の「AC06-2 コメント有り
    (削除済み1+通常1)スレッドは削除不可」が同じことを具体例で確認している。 -/
theorem deleteThread_blocked_by_any_comment (db : Db) (sid : SessionId) (tid : ThreadId)
    (uid : UserId) (t : Thread) (c : Comment)
    (hs : db.sessions.find? (·.id = sid) = some ⟨sid, uid⟩)
    (ht : db.threads.find? (·.id = tid) = some t)
    (hown : t.authorId = uid)
    (hc : c ∈ db.comments) (hct : c.threadId = tid) :
    (deleteThread sid tid) db = (.error .threadHasComments, db) := by
  have hmem : c ∈ db.comments.filter (·.threadId = tid) := by
    rw [List.mem_filter]
    exact ⟨hc, by simp [hct]⟩
  have hne : (db.comments.filter (·.threadId = tid)).isEmpty = false := by
    cases hf : db.comments.filter (·.threadId = tid) with
    | nil => rw [hf] at hmem; cases hmem
    | cons a as => rfl
  have hown' : decide (t.authorId = uid) = true := by simp [hown]
  unfold deleteThread
  simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
    Action.pure, Action.fail, Action.modify, requireAuth, findThread, Action.liftOption,
    Op.commentsOf, hs, ht, hown', hne, ensure_true_eq, ensure_false_eq]

/-- AC06-2: 削除済みコメントも件数に数える。

    **F08のレビュー指摘を受けてrestateした。** 旧言明は
    `∀ e s', (deleteThread sid tid) db = (.error e, s') → s' = db`
    という形で、仮定`hc`/`hct`/`hcd`（「削除済みコメントがこのスレッドに存在する」
    というAC06-2の状況設定そのもの）を**1つも使わなかった**。中身は
    `deleteThread_atomic`（`NoWriteOnError`）の特殊化にすぎず、命題自体は真だが
    **AC06-2について何も述べていない** ―― 証明しても「削除済みコメントが
    スレッド削除を阻む」ことの保証にはならない（「失敗したなら状態が変わらない」と
    「削除済みコメントがあるなら失敗する」は別の主張であり、前者は後者を含まない）。

    AC06-2 が要求しているのは後者、すなわち**失敗すること自体**とその理由なので、
    結論を「`.threadHasComments`で失敗する」へ改めた。認可（セッション・作成者本人）を
    仮定に加えたのは、それが無いと`.notAuthenticated`/`.forbidden`での失敗と
    区別できず、「コメントの存在が理由で拒否された」ことを言えないため。

    `hcd`（削除済みであること）は証明には要らない ―― `commentsOf`（`Op.lean`）が
    `deleted`で絞らないので、削除済みでも未削除でも同じ経路で拒否される。
    **その「要らなさ」こそがAC06-2の内容**（削除済みコメントも件数に数える）なので、
    一般形を`deleteThread_blocked_by_any_comment`として立て、AC06-2はその
    削除済みケースへの特殊化として明示的に残す。 -/
theorem deleteThread_blocked_by_deleted_comment (db : Db) (sid : SessionId) (tid : ThreadId)
    (uid : UserId) (t : Thread) (c : Comment)
    (hs : db.sessions.find? (·.id = sid) = some ⟨sid, uid⟩)
    (ht : db.threads.find? (·.id = tid) = some t)
    (hown : t.authorId = uid)
    (hc : c ∈ db.comments) (hct : c.threadId = tid) (hcd : c.deleted = true) :
    (deleteThread sid tid) db = (.error .threadHasComments, db) :=
  deleteThread_blocked_by_any_comment db sid tid uid t c hs ht hown hc hct

/-! ### 6. 表示名の全投稿反映 (AC04-2)

D03 方式①（JOIN 解決）の正しさ。表示名を変えた直後、
その利用者の過去のスレッド・コメントはすべて新しい表示名で表示される。 -/

/-- AC04-2: 表示名を変えると、その利用者の過去のスレッドの表示名も新しい値になる。

    **結論は`some n`ではなく`some (Validation.trim n)`でなければならない**
    （F06セッションのレビューで訂正）。旧言明は結論を`some n`（トリム前の生の入力）と
    書いており、**偽**だった ―― `updateDisplayName`（`Op.lean`）は decision 0004 に従い
    `Validation.trim newName`を保存し、`displayNameFailure`もトリム後の値で
    検証する。したがって`n`が前後に空白を含むと、仮定`hv`を満たしたまま結論が破れる。

    反例（`decide`で確認済み）: `n = " A "`。`displayNameValid " A " = true`
    （トリム後"A"は1文字）だが、保存されるのは`"A"`なので
    `authorDisplayName = some " A "`は成り立たない。

    **さらに`Wf db`が要る**（F11セッションのレビューで訂正、2度目の修正）。
    トリムを直した後の言明も、`Wf`を仮定しないままでは**まだ偽**だった ――
    `hs`はセッションの存在しか言わず、その`userId`に対応する`User`が
    `db.users`に実在することを保証しない。`updateDisplayName`の更新は
    `s.users.map (fun u => if u.id = uid then … else u)`（`Op.lean`）なので、
    該当ユーザーが居なければ**黙って何も起きない**。一方`toRow`の
    `authorDisplayName`は`displayNameOf db' t.authorId`
    ＝`(db'.users.find? (·.id = uid)).map (·.displayName)`であり、`none`になる。

    反例（`decide`で確認済み）: `db.users = []`、`db.sessions = [⟨1, 1⟩]`、
    `db.threads = [{id := 1, authorId := 1, …}]`、`n = "A"`。仮定`hs`・`hv`は
    ともに成立するが、更新後の`authorDisplayName`は`none`であり
    `some "A"`にならない。

    `Wf`の`sessionUsersExist`（∀ s ∈ db.sessions, ∃ u ∈ db.users, u.id = s.userId）が
    ちょうどこの穴を塞ぐ。`userIdsDistinct`と併せて、`find?`が拾う唯一の該当ユーザーが
    更新後の`Validation.trim n`を持つことが言えるようになる。この`db`は`Wf`を
    破っている（`sessionUsersExist`・`threadAuthorsExist`の両方に違反）ので、
    `Wf`を仮定すれば反例は消える。

    `sorry`のまま残す言明が偽だと、後続セッションがこれを補題として使った時点で
    そこから導かれるものがすべて無価値になる（このファイル冒頭の方針、および
    `thread_immutable`が同じ理由でF05時点に仮定を補われた経緯を参照）。証明を
    後回しにすること自体は許すが、**言明は真の形で置く**。F04実装のサイクルで証明する。 -/
theorem displayName_propagates (db : Db) (sid : SessionId) (uid : UserId) (n : String)
    (hwf : Wf db)
    (hs : db.sessions.find? (·.id = sid) = some ⟨sid, uid⟩)
    (hv : Validation.displayNameValid n = true) :
    let db' := (updateDisplayName sid n db).2
    ∀ t ∈ db'.threads, t.authorId = uid →
      (toRow db' t).authorDisplayName = some (Validation.trim n) := by
  sorry

/-! ### 7. 一覧・ソート・ページネーション (F09, F12, F13)

F09（初期表示は作成日時降順のみ、decision 0009）実装に伴い、このセクションのうち
`page_size_bound`・`first_page_no_prev`・`hasNext_iff_more`・`pagination_preserves_order`・
`sortThreads_perm`の5件を証明した。いずれも`sortThreads`/`paginate`/`threadList`という
**既に全`SortKey`について定義済みの純関数**（F12のソート切替UI自体は未実装だが、
モデル・`app/src/domain/query.rs`の対応する関数はdecision 0009/0013の時点で
`SortKey`全体に対して汎用に書かれている）についての言明であり、F06〜F08のような
未実装`Step`操作（`runAll`・`Step`を経由する横断的invariant）に依存しない。
decision 0025のスコープ限定（実装未着手の操作を含む言明を対象操作へ絞る）は
この5件には該当しないため、`SortKey`を`.createdDesc`へ絞らず一般形のまま証明する。
`comment_bumps_lastUpdated`はF07（コメント作成）で証明した（下記セクション8）。

ただし**`sorry`は「未証明」であって「真」ではない**。F05の`thread_immutable`が
仮定不足で偽だった前例に倣い、当時この3件も反例の有無を個別に検査していた:

- `sorted_by_commentCount`・`createdAsc_head_is_oldest`: 真。`leOf`が
  （コメント数降順, id昇順）／（作成日時昇順, id昇順）の辞書式で**全順序**であり、
  挿入ソートが整列列を返すことから従う。`db`の健全性には依存しない。
- `comment_bumps_lastUpdated`: **偽だった**（decision 0027）。論理時計の単調性を
  仮定に持たないため反例が構成できる。F07で`Wf`に`clockDominatesThreads`/
  `clockDominatesComments`を追加して真の形に直し、証明した。詳細は同定理のdocコメント。

F12（ソート切替UI）実装に伴い、`sorted_by_commentCount`・`createdAsc_head_is_oldest`の
2件を証明した。証明の下ごしらえとして`insertBy_pairwise`/`sortBy_pairwise`
（「`le`が連結的かつ推移的なら挿入ソートは整列列を返す」という一般補題。汎用の
「全順序なら挿入ソートは整列列を返す」というライブラリ補題が無いため自前で用意した）と、
`leOf`の該当2キーぶんの連結性・推移性補題（`leOf_commentCountDesc_total`等）を追加した。
上記で「真であることを検査済み」としていた通り、いずれも反例なく証明できた
（`db`の健全性`Wf`に依存しない）。 -/

/-- 挿入は要素を1つ増やすだけ（除去も重複もしない）。`sortBy_length`の土台。 -/
theorem insertBy_length {α : Type} (le : α → α → Bool) (a : α) (l : List α) :
    (insertBy le a l).length = l.length + 1 := by
  induction l with
  | nil => rfl
  | cons b bs ih =>
    unfold insertBy
    split
    · rfl
    · simp [ih]

/-- 挿入ソート（`sortBy`）は並べ替えである（件数を変えない）。`sortThreads_perm`の土台。 -/
theorem sortBy_length {α : Type} (le : α → α → Bool) (l : List α) :
    (sortBy le l).length = l.length := by
  induction l with
  | nil => rfl
  | cons a as ih =>
    unfold sortBy
    simp [insertBy_length, ih]

/-- ソートは並べ替えである（件数も要素も変わらない）。 -/
theorem sortThreads_perm (db : Db) (k : SortKey) (ts : List Thread) :
    (sortThreads db k ts).length = ts.length := by
  unfold sortThreads
  exact sortBy_length _ ts

/-- 挿入は要素の集合を変えない（挿入対象自身か、元のリストの要素）。
    `sortBy_mem`の土台。`deleted_comment_keeps_metadata`（F10）で、コメントが
    `threadDetail`の並べ替え後も消えず残ることを示すのに使う。 -/
theorem insertBy_mem {α : Type} (le : α → α → Bool) (x e : α) (l : List α) :
    e ∈ insertBy le x l ↔ e = x ∨ e ∈ l := by
  induction l with
  | nil => simp [insertBy]
  | cons b bs ih =>
    unfold insertBy
    split
    · simp
    · simp only [List.mem_cons, ih]
      exact or_left_comm

/-- 挿入ソート（`sortBy`）は要素の集合を変えない。並べ替えでは要素が
    増減しないことの、件数版（`sortBy_length`）に対する会員版。 -/
theorem sortBy_mem {α : Type} (le : α → α → Bool) (e : α) (l : List α) :
    e ∈ sortBy le l ↔ e ∈ l := by
  induction l with
  | nil => simp [sortBy]
  | cons a as ih =>
    unfold sortBy
    rw [insertBy_mem, ih]
    simp

/-- C-12: ページは常に10件以下。 -/
theorem page_size_bound (db : Db) (k : SortKey) (p : Nat) :
    (threadList db k p).items.length ≤ pageSize := by
  unfold threadList paginate
  exact List.length_take_le _ _

/-- C-12: 1ページ目に「前に戻る」は出ない。 -/
theorem first_page_no_prev (db : Db) (k : SortKey) :
    (threadList db k 1).hasPrev = false := by
  unfold threadList paginate
  rfl

/-- C-12: 「次に進む」が出るのは、実際に次のページに項目があるときだけ。

    **仮定`1 ≤ p`が要る**（decision 0026）。`paginate`は`n = 0`を1ページ目として
    丸めるため（decision 0013。実装側は`domain::query::paginate`の
    `if page == 0 { 1 } else { page }`に対応）、`p = 0`のときだけ言明が偽になる。
    反例: `db.threads.length = 5`（1ページに収まりページ2は無い）のとき`p = 0`を渡すと、
    `paginate`は1ページ目として扱い`hasNext = false`だが、右辺は`5 > 0 * pageSize = 0`
    で真になる（`lake build`で実際にこの反例を構成して確認した）。

    Why-not: 丸め規約そのもの（`if p = 0 then 1 else p`）を右辺に転記する形は採らない。
    それは`paginate`の実装をそのまま言明へ写すことになり、不変条件が実装を**拘束する**
    のではなく**追認する**だけになる（p=0の1点で、実装が何を返してもそれが正しいと
    言えてしまう）。ページ番号は1始まりというのが原典C-12の前提であり、`1 ≤ p`を
    仮定に置いて意味のある定義域だけを語るほうが、不変条件としての拘束力を保てる。
    呼び出し側の`ListParams::parse`（`app/src/web/params.rs`）がpを1以上に丸めてから
    `paginate`へ渡すため、この仮定は実運用で常に満たされる。 -/
theorem hasNext_iff_more (db : Db) (k : SortKey) (p : Nat) (hp : 1 ≤ p) :
    (threadList db k p).hasNext = true ↔
      db.threads.length > p * pageSize := by
  have hlen : ((sortThreads db k db.threads).map (toRow db)).length = db.threads.length := by
    rw [List.length_map, sortThreads_perm]
  unfold threadList paginate pageSize
  simp only [decide_eq_true_iff, List.length_drop, hlen]
  split <;> omega

/-- C-13: ページをまたいでもソート順が保たれる ＝
    全ページを連結すると、ソート済み全体列と一致する。

    `p = 0`のときも成り立つ: `paginate`内部の丸め後ページ番号は`1`だが
    `(1 - 1) = 0 = (0 - 1)`（Natの切り捨て減算）なので、丸めの有無で`drop`の
    引数が一致し、`hasNext_iff_more`と異なり式の書き換えは不要だった。 -/
theorem pagination_preserves_order (db : Db) (k : SortKey) (p : Nat) :
    (threadList db k p).items =
      (((sortThreads db k db.threads).map (toRow db)).drop ((p - 1) * pageSize)).take pageSize
    := by
  unfold threadList paginate
  split
  · next hp0 => subst hp0; rfl
  · rfl

/-- 挿入ソートの核心補題（下ごしらえ）。`le`が連結的（`htotal`。両方向のうち
    少なくとも一方が必ず成り立つ、全順序の強さまでは要求しない）かつ推移的（`htrans`）
    なら、既に整列済みのリストへ挿入しても整列は保たれる。`sortBy_pairwise`の土台。
    汎用の「全順序なら挿入ソートは整列列を返す」というライブラリ補題が
    （mathlib等の外部依存を持たないこの`formal/`には）無いため自前で用意した。 -/
theorem insertBy_pairwise {α : Type} (le : α → α → Bool)
    (htotal : ∀ a b, le a b = true ∨ le b a = true)
    (htrans : ∀ a b c, le a b = true → le b c = true → le a c = true)
    (a : α) (l : List α) (hl : List.Pairwise (fun x y => le x y = true) l) :
    List.Pairwise (fun x y => le x y = true) (insertBy le a l) := by
  induction l with
  | nil => simpa [insertBy] using List.pairwise_singleton (fun x y => le x y = true) a
  | cons b bs ih =>
    rw [List.pairwise_cons] at hl
    obtain ⟨hb, hbs⟩ := hl
    unfold insertBy
    split
    · next hab =>
      rw [List.pairwise_cons]
      refine ⟨fun x hx => ?_, List.pairwise_cons.2 ⟨hb, hbs⟩⟩
      rcases List.mem_cons.mp hx with rfl | hx
      · exact hab
      · exact htrans a b x hab (hb x hx)
    · next hab =>
      have hba : le b a = true := (htotal a b).resolve_left hab
      rw [List.pairwise_cons]
      refine ⟨fun x hx => ?_, ih hbs⟩
      rcases (insertBy_mem le a x bs).mp hx with rfl | hx
      · exact hba
      · exact hb x hx

/-- 挿入ソート（`sortBy`）は`le`が連結的かつ推移的なら整列列を返す。
    `sorted_by_commentCount`・`createdAsc_head_is_oldest`の土台。 -/
theorem sortBy_pairwise {α : Type} (le : α → α → Bool)
    (htotal : ∀ a b, le a b = true ∨ le b a = true)
    (htrans : ∀ a b c, le a b = true → le b c = true → le a c = true) :
    ∀ l : List α, List.Pairwise (fun x y => le x y = true) (sortBy le l)
  | [] => by simp [sortBy]
  | a :: as =>
      insertBy_pairwise le htotal htrans a (sortBy le as) (sortBy_pairwise le htotal htrans as)

/-- `leOf db .commentCountDesc`は連結的（コメント数の`Nat`三分律 + idの`Nat`全順序）。
    `unfold ThreadId`が要る: `ThreadId`は`Nat`の`abbrev`だが、`omega`は型名が
    構文的に`Nat`と一致しないと同じ数値領域として扱わない（`Bbs.ThreadId`のままだと
    「制約が見つからない」で失敗する。`formal/`で実測して確認した挙動）。 -/
theorem leOf_commentCountDesc_total (db : Db) (a b : Thread) :
    leOf db .commentCountDesc a b = true ∨ leOf db .commentCountDesc b a = true := by
  simp only [leOf, Bool.or_eq_true, Bool.and_eq_true, decide_eq_true_iff]
  unfold ThreadId
  omega

/-- `leOf db .commentCountDesc`は推移的。 -/
theorem leOf_commentCountDesc_trans (db : Db) (a b c : Thread) :
    leOf db .commentCountDesc a b = true → leOf db .commentCountDesc b c = true →
      leOf db .commentCountDesc a c = true := by
  simp only [leOf, Bool.or_eq_true, Bool.and_eq_true, decide_eq_true_iff]
  unfold ThreadId
  omega

/-- `leOf db .createdAsc`は連結的（作成日時の`Nat`三分律 + idの`Nat`全順序）。 -/
theorem leOf_createdAsc_total (db : Db) (a b : Thread) :
    leOf db .createdAsc a b = true ∨ leOf db .createdAsc b a = true := by
  simp only [leOf, Bool.or_eq_true, Bool.and_eq_true, decide_eq_true_iff]
  unfold ThreadId Time
  omega

/-- `leOf db .createdAsc`は推移的。 -/
theorem leOf_createdAsc_trans (db : Db) (a b c : Thread) :
    leOf db .createdAsc a b = true → leOf db .createdAsc b c = true →
      leOf db .createdAsc a c = true := by
  simp only [leOf, Bool.or_eq_true, Bool.and_eq_true, decide_eq_true_iff]
  unfold ThreadId Time
  omega

/-- AC12-3 / C-16: コメント数順は削除済みを含む件数の降順。 -/
theorem sorted_by_commentCount (db : Db) (ts : List Thread) :
    List.Pairwise (fun a b => commentCount db a.id ≥ commentCount db b.id)
      (sortThreads db .commentCountDesc ts) := by
  have h := sortBy_pairwise (leOf db .commentCountDesc)
    (leOf_commentCountDesc_total db) (leOf_commentCountDesc_trans db) ts
  unfold sortThreads
  refine h.imp fun hab => ?_
  simp only [leOf, Bool.or_eq_true, Bool.and_eq_true, decide_eq_true_iff] at hab
  unfold ThreadId at hab
  omega

/-- シナリオ04-2-2: 作成日時昇順で先頭が最古。 -/
theorem createdAsc_head_is_oldest (db : Db) (ts : List Thread) (t : Thread)
    (h : (sortThreads db .createdAsc ts).head? = some t) :
    ∀ t' ∈ ts, t.createdAt ≤ t'.createdAt := by
  have hsorted : List.Pairwise (fun x y => leOf db .createdAsc x y = true)
      (sortThreads db .createdAsc ts) :=
    sortBy_pairwise (leOf db .createdAsc) (leOf_createdAsc_total db) (leOf_createdAsc_trans db) ts
  obtain ⟨rest, heq⟩ : ∃ rest, sortThreads db .createdAsc ts = t :: rest := by
    -- `rcases hlist : e with pat`は`e`の出現をゴールにも代入するため、
    -- ここでの目標は既に`hd :: tl = t :: rest`の形になっている(`hlist`自体の
    -- 書き換えは不要)。
    rcases hlist : sortThreads db .createdAsc ts with _ | ⟨hd, tl⟩
    · simp [hlist] at h
    · simp only [hlist, List.head?_cons, Option.some.injEq] at h
      exact ⟨tl, by rw [h]⟩
  rw [heq] at hsorted
  rw [List.pairwise_cons] at hsorted
  intro t' ht'
  have hmem : t' ∈ sortThreads db .createdAsc ts := by
    unfold sortThreads
    exact (sortBy_mem (leOf db .createdAsc) t' ts).mpr ht'
  rw [heq, List.mem_cons] at hmem
  unfold Time
  rcases hmem with rfl | hin
  · exact Nat.le_refl _
  · have hrel := hsorted.1 t' hin
    simp only [leOf, Bool.or_eq_true, Bool.and_eq_true, decide_eq_true_iff] at hrel
    unfold ThreadId Time at hrel
    omega

/-! ### 8. 最終更新日時 (C-15 / AC09-4)

コメント投稿はそのスレッドの最終更新日時を厳密に進める。 -/

/-- `Nat.max`は結合的。コアに具体名の補題が見当たらないため、`Nat.max_def`
    (`if`への展開)経由で`omega`に落として自前で示す。 -/
theorem nat_max_assoc (a b c : Nat) :
    Nat.max a (Nat.max b c) = Nat.max (Nat.max a b) c := by
  simp only [Nat.max_def]
  repeat' split
  all_goals omega

/-- 挿入ソートと違い、`maxTime`は末尾への1件追加でどう動くかを直接述べる補題。
    `comment_bumps_lastUpdated`で、`commentsOf db' t.id`が`commentsOf db t.id`に
    新規コメント1件を追加した形になることと組み合わせて使う。 -/
theorem maxTime_append_singleton (l : List Time) (x : Time) :
    maxTime (l ++ [x]) = Nat.max (maxTime l) x := by
  induction l with
  | nil => simp [maxTime]
  | cons a as ih =>
    simp only [List.cons_append, maxTime]
    rw [ih, nat_max_assoc]

/-- リスト全要素が`b`以下なら、`maxTime`も`b`以下（`clockDominatesComments`から
    `lastUpdatedAt db t ≤ db.clock`を導くのに使う）。 -/
theorem maxTime_le {l : List Time} {b : Time} (h : ∀ x ∈ l, x ≤ b) : maxTime l ≤ b := by
  induction l with
  | nil => simp [maxTime]
  | cons a as ih =>
    simp only [maxTime]
    exact Nat.max_le.mpr ⟨h a List.mem_cons_self,
      ih (fun x hx => h x (List.mem_cons_of_mem a hx))⟩

/-- **論理時計の単調性を仮定に持たないと偽になる**（decision 0027）。`Db`構造体にも
    当初の`Wf`にも「既存レコードの`createdAt`は`clock`以下」という制約が無かったため、
    `clock`より進んだ`createdAt`を持つ`db`を自由に構成できた。反例:
    `clock = 0`・コメント1件が`createdAt = 100`のとき、`createComment`が付ける
    時刻は`tick`により`clock + 1 = 1`にしかならず、`lastUpdatedAt`は前後とも
    `100`のままで**厳密に増えない**（`formal/`で実際に構成し`native_decide`で
    確認した、decision 0027）。

    F07（コメント作成）実装セッションで、`Wf`に`clockDominatesThreads`/
    `clockDominatesComments`を追加した（decision 0027の選択肢(a)、同decisionの
    「決定」節で確定済みの方針。当初の局所仮定`hclockT`/`hclockC`はこの2フィールドの
    導出に置き換えた）。`wf_empty`とF01〜F06の`Wf`保存補題はこのセッション時点では
    まだ無い（`Wf`保存補題自体が本セッションより前に存在しない。`wf_empty`は
    フィールド追加後も`Db.empty`が空リストのみを持つことから自明に成り立つ）ため、
    波及の心配なく追加できた。 -/
theorem comment_bumps_lastUpdated (db : Db) (sid : SessionId) (tid : ThreadId) (b : String)
    (t : Thread) (ht : t ∈ db.threads) (ht' : t.id = tid) (cid : CommentId)
    (hwf : Wf db)
    (hok : (createComment sid tid b) db = (.ok cid, (createComment sid tid b db).2)) :
    let db' := (createComment sid tid b db).2
    lastUpdatedAt db t < lastUpdatedAt db' t := by
  have hclockT : t.createdAt ≤ db.clock := hwf.clockDominatesThreads t ht
  have hclockC : ∀ c ∈ db.comments, c.createdAt ≤ db.clock := hwf.clockDominatesComments
  have hbound : lastUpdatedAt db t ≤ db.clock := by
    unfold lastUpdatedAt
    apply Nat.max_le.mpr
    refine ⟨hclockT, maxTime_le ?_⟩
    intro x hx
    rw [List.mem_map] at hx
    obtain ⟨c, hc, hcx⟩ := hx
    unfold Query.commentsOf at hc
    rw [List.mem_filter] at hc
    exact hcx ▸ hclockC c hc.1
  -- `tid`を`t.id`に統一しておく(以降の場合分けで`newComment.threadId = t.id`が
  -- 定義上そのまま成り立つようにするため)。
  subst ht'
  -- `hok`の実質的な中身は「成功して`cid`を返す」ことだけ(第2成分の等式は
  -- 自己言及で無内容)。これで検査失敗の分岐を`hok`と矛盾させて潰す。
  have hcid : ((createComment sid t.id b) db).1 = Except.ok cid := congrArg Prod.fst hok
  show lastUpdatedAt db t < lastUpdatedAt ((createComment sid t.id b) db).2 t
  unfold createComment at hcid ⊢
  cases hsess : db.sessions.find? (·.id = sid) with
  | none =>
    simp only [bind, Bind.bind, Action.bind, Action.get, Action.fail, requireAuth, hsess] at hcid
    injection hcid
  | some sess =>
    cases hthread : db.threads.find? (·.id = t.id) with
    | none =>
      simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
        Action.fail, requireAuth, hsess, findThread, Action.liftOption, hthread] at hcid
      injection hcid
    | some thr =>
      cases hbody : Validation.nonEmptyText b with
      | false =>
        simp only [bind, Bind.bind, Pure.pure, Action.bind, Action.get, Action.pure,
          Action.fail, requireAuth, hsess, findThread, Action.liftOption, hthread, hbody,
          ensure_false_eq] at hcid
        injection hcid
      | true =>
        -- ここまでで残る分岐は成功一択: 新規コメント1件が`db.clock + 1`の時刻で
        -- 追加される。ゴール側をこの具体形まで簡約する(`hcid`はもう使わない ―
        -- `cid`の値そのものはこの先の数値の議論に登場しない)。
        simp only [bind, pure, Bind.bind, Pure.pure, Action.bind, Action.get, Action.set,
          Action.pure, Action.modify, tick, requireAuth, hsess, findThread, Action.liftOption,
          hthread, ensure_true_eq]
        unfold lastUpdatedAt Query.commentsOf
        simp only [List.filter_append, List.map_append, List.filter_cons, List.filter_nil,
          List.map_cons, List.map_nil, decide_eq_true_eq, if_true, maxTime_append_singleton]
        -- 残る目標は`Nat.max t.createdAt M < Nat.max t.createdAt (Nat.max M (db.clock+1))`
        -- (`M`はコメント時刻列の`maxTime`)。`hbound : lastUpdatedAt db t ≤ db.clock`から従う。
        unfold lastUpdatedAt Query.commentsOf at hbound
        generalize hM : maxTime (List.map (fun x => x.createdAt)
          (List.filter (fun x => decide (x.threadId = t.id)) db.comments)) = M at hbound ⊢
        have h1 : db.clock < db.clock + 1 := Nat.lt_succ_self _
        have h2 : db.clock + 1 ≤ Nat.max M (db.clock + 1) := Nat.le_max_right _ _
        have h3 : Nat.max M (db.clock + 1) ≤ Nat.max t.createdAt (Nat.max M (db.clock + 1)) :=
          Nat.le_max_right _ _
        exact Nat.lt_of_le_of_lt hbound (Nat.lt_of_lt_of_le h1 (Nat.le_trans h2 h3))

/-! #### 8.1 `comment_bumps_lastUpdated` が非空虚に適用できることの確認

セクション1.1（`Wf`の保存・最小連鎖）の目的地。`comment_bumps_lastUpdated`は
`Wf db`と`t ∈ db.threads`を**同時に**要求するため、`Wf`の住人が`wf_empty`
（`threads = []`）しか無い間は前提を満たす`db`が1つも作れず、証明済みでも
**適用できない定理**だった（F08のレビュー指摘）。ここでは`Db.empty`から
register → login → createThread と辿った具体的な状態を作り、
(1) その状態が`Wf`であることを保存補題の**連鎖だけ**から導き、
(2) そこにスレッドが実在することを示し、
(3) 実際に`comment_bumps_lastUpdated`を適用する、
の3点を機械的に確認する。`decide`/`rfl`で閉じる具体例（テスト定理）であり、
一般性は無い ―― 一般形の主張は各保存補題そのものが担う。 -/

/-- 空DBに利用者を1人登録した状態。 -/
def chainDb1 : Db := ((register "testuser_01" "TestPassword123!" "テストユーザー01") Db.empty).2

/-- さらにログインした状態（採番されるセッションIDは`0`）。 -/
def chainDb2 : Db := ((login "testuser_01" "TestPassword123!") chainDb1).2

/-- さらにスレッドを1件立てた状態（スレッドID`0`・`createdAt = 1`）。
    `comment_bumps_lastUpdated`の前提を満たす最小の`db`。 -/
def chainDb3 : Db := ((createThread 0 "スレッドタイトル" "スレッド本文") chainDb2).2

/-- (1) 到達状態の整合性。**保存補題の連鎖だけ**で導いており、`decide`等による
    具体計算に頼っていない（連鎖が実際に繋がっていることの確認になる）。 -/
theorem chainDb3_wf : Wf chainDb3 :=
  createThread_preserves_wf _ _ _ _
    (login_preserves_wf _ _ _
      (register_preserves_wf _ _ _ _ wf_empty))

/-- (2) 到達状態にはスレッドが実在する（`Db.empty`と違い`threads`が空でない）。
    これと`chainDb3_wf`が両立することが、`comment_bumps_lastUpdated`の
    前提が非空虚であることの内実。 -/
theorem chainDb3_has_thread :
    (⟨0, 0, "スレッドタイトル", "スレッド本文", 1⟩ : Thread) ∈ chainDb3.threads := by
  decide

/-- (3) 実適用。`comment_bumps_lastUpdated`を上の具体例に適用し、コメント投稿が
    実際に最終更新日時を進める（`1 < 2`）ことを得る。これが通ることで
    「証明は通るが適用できない」状態が解消されたと言える。 -/
theorem comment_bumps_lastUpdated_is_applicable :
    lastUpdatedAt chainDb3 ⟨0, 0, "スレッドタイトル", "スレッド本文", 1⟩ <
      lastUpdatedAt ((createComment 0 0 "コメント本文") chainDb3).2
        ⟨0, 0, "スレッドタイトル", "スレッド本文", 1⟩ :=
  comment_bumps_lastUpdated chainDb3 0 0 "コメント本文"
    ⟨0, 0, "スレッドタイトル", "スレッド本文", 1⟩ chainDb3_has_thread rfl 0 chainDb3_wf rfl

/-! ### 9. 検索 (F11 / AC11-2, AC11-4) -/

/-- AC11-2: 本文またはコメント本文にキーワードを含むスレッドがヒットする。
    `hitIn`は本文優先(`if containsSubstr t.body kw then some .inBody else ...`)なので、
    本文がヒットする`t`は無条件に`.inBody`を返す ―― コメント側の場合分けを見る必要がない。 -/
theorem search_finds_body (db : Db) (kw : String)
    (t : Thread) (h : t ∈ db.threads) (hc : containsSubstr t.body kw = true) :
    ∃ r ∈ search db kw, r.thread = t := by
  refine ⟨{ thread := t, hit := .inBody }, ?_, rfl⟩
  unfold search
  rw [List.mem_filterMap]
  refine ⟨t, h, ?_⟩
  unfold hitIn
  simp [hc]

/-- AC11-4 の充足（decision 0012 で確定した方式）: 削除済みコメントを理由に
    ヒットすることはない ＝ 返る `Hit` が指すコメントは必ず未削除。 -/
theorem no_deleted_hit (db : Db) (kw : String) (r : SearchResult)
    (h : r ∈ search db kw) (cid : CommentId) (hh : r.hit = .inComment cid) :
    ∃ c ∈ db.comments, c.id = cid ∧ c.deleted = false := by
  unfold search at h
  rw [List.mem_filterMap] at h
  obtain ⟨t, ht, hf⟩ := h
  unfold hitIn at hf
  dsimp only at hf
  split at hf
  · -- 本文ヒット(`.inBody`)分岐: `r.hit = .inComment cid`という仮定と矛盾する。
    simp only [Option.map_some] at hf
    have hr : r = { thread := t, hit := Hit.inBody } := Option.some.inj hf.symm
    rw [hr] at hh
    simp at hh
  · -- コメントヒット分岐。`searchableComments`(=`!c.deleted`でフィルタ済み)から
    -- 見つかった要素のidが`cid`。
    cases hfind :
        (searchableComments (commentsOf db t.id)).find? (fun c => containsSubstr c.body kw) with
    | none => rw [hfind] at hf; simp at hf
    | some c =>
      rw [hfind] at hf
      simp only [Option.map_some] at hf
      have hr : r = { thread := t, hit := Hit.inComment c.id } := Option.some.inj hf.symm
      rw [hr] at hh
      simp only [Hit.inComment.injEq] at hh
      have hcmem : c ∈ searchableComments (commentsOf db t.id) :=
        List.mem_of_find?_eq_some hfind
      unfold searchableComments at hcmem
      rw [List.mem_filter] at hcmem
      obtain ⟨hcmem', hcnotdel⟩ := hcmem
      unfold Query.commentsOf at hcmem'
      rw [List.mem_filter] at hcmem'
      exact ⟨c, hcmem'.1, hh, by simpa using hcnotdel⟩

-- `hit_is_reachable`（AC11-3のスクロール先が実在すること、D19）は本セクションの
-- 末尾ではなく§10末尾に置く。証明が`deleted_comment_keeps_metadata`（§10）に
-- 依存し、Leanは前方参照を許さないため。

/-! ### 10. 固定文言 (C-01 / AC08-2) -/

theorem deleted_comment_renders_fixed_text (c : Comment) (h : c.deleted = true) :
    renderCommentBody c = deletedCommentText := by
  unfold renderCommentBody
  simp [h]

/-- AC10-3 の[解釈]: 削除済みでも作成者・日時は維持される。
    `threadDetail`は`c.deleted`で場合分けせず全コメントを`CommentView`に写すので、
    削除済みかどうかに関わらず`id`・`createdAt`・`authorDisplayName`は元のコメントと
    一致する（本文だけが`renderCommentBody`で固定文言に差し替わる）。 -/
theorem deleted_comment_keeps_metadata (db : Db) (tid : ThreadId) (d : ThreadDetail)
    (h : threadDetail db tid = some d) (c : Comment) (hc : c ∈ db.comments)
    (hct : c.threadId = tid) :
    ∃ cv ∈ d.comments, cv.id = c.id ∧ cv.createdAt = c.createdAt ∧
      cv.authorDisplayName = displayNameOf db c.authorId := by
  unfold threadDetail at h
  rw [Option.map_eq_some_iff] at h
  obtain ⟨t, hfind, hd⟩ := h
  have htid : t.id = tid := by simpa using List.find?_some hfind
  have hcmt : c ∈ Query.commentsOf db t.id := by
    unfold Query.commentsOf
    rw [List.mem_filter]
    refine ⟨hc, ?_⟩
    simp [htid, hct]
  refine ⟨{ id := c.id, authorDisplayName := displayNameOf db c.authorId,
            body := renderCommentBody c, createdAt := c.createdAt, deleted := c.deleted },
    ?_, rfl, rfl, rfl⟩
  rw [← hd]
  simp only [List.mem_map]
  exact ⟨c, (sortBy_mem _ c (Query.commentsOf db t.id)).mpr hcmt, rfl⟩

/-- AC11-3 のスクロール先が実在すること（D19）。
    ヒット箇所は必ず詳細画面に描画される要素を指す。`no_deleted_hit`（§9）と
    同じ場合分けで`Hit.inComment`に対応する未削除コメント`c`を取り出し、
    `deleted_comment_keeps_metadata`（本セクション）を使って`c`が
    `threadDetail`の`comments`に現れることを示す。 -/
theorem hit_is_reachable (db : Db) (kw : String)
    (r : SearchResult) (h : r ∈ search db kw) (cid : CommentId)
    (hh : r.hit = .inComment cid) :
    ∃ d, threadDetail db r.thread.id = some d ∧ ∃ cv ∈ d.comments, cv.id = cid := by
  unfold search at h
  rw [List.mem_filterMap] at h
  obtain ⟨t, ht, hf⟩ := h
  unfold hitIn at hf
  dsimp only at hf
  split at hf
  · -- 本文ヒット分岐は`.inComment`という仮定と矛盾する(no_deleted_hitと同型)。
    simp only [Option.map_some] at hf
    have hr : r = { thread := t, hit := Hit.inBody } := Option.some.inj hf.symm
    rw [hr] at hh
    simp at hh
  · cases hfind :
        (searchableComments (commentsOf db t.id)).find? (fun c => containsSubstr c.body kw) with
    | none => rw [hfind] at hf; simp at hf
    | some c =>
      rw [hfind] at hf
      simp only [Option.map_some] at hf
      have hr : r = { thread := t, hit := Hit.inComment c.id } := Option.some.inj hf.symm
      have hrthread : r.thread = t := by rw [hr]
      have hh' : c.id = cid := by rw [hr] at hh; simpa using hh
      -- `c`は未削除コメント(`searchableComments`)かつ`t`に属する(`commentsOf`)。
      have hcmem : c ∈ searchableComments (commentsOf db t.id) :=
        List.mem_of_find?_eq_some hfind
      unfold searchableComments at hcmem
      rw [List.mem_filter] at hcmem
      obtain ⟨hcmem', -⟩ := hcmem
      unfold Query.commentsOf at hcmem'
      rw [List.mem_filter] at hcmem'
      obtain ⟨hcin, hcthread⟩ := hcmem'
      have hcthread' : c.threadId = t.id := by simpa using hcthread
      -- `t ∈ db.threads`なので、詳細画面(`threadDetail`)は必ず`some`になる
      -- (存在しないスレッドの検索結果は`search`が返さないので404には落ちない、
      -- decision 0012の裏付け)。
      rw [hrthread]
      cases hd : threadDetail db t.id with
      | none =>
        exfalso
        unfold threadDetail at hd
        cases hfc : db.threads.find? (fun t' => t'.id = t.id) with
        | none => exact (List.find?_eq_none.mp hfc) t ht (by simp)
        | some t' => rw [hfc] at hd; simp at hd
      | some d =>
        refine ⟨d, rfl, ?_⟩
        obtain ⟨cv, hcvmem, hcvid, -, -⟩ :=
          deleted_comment_keeps_metadata db t.id d hd c hcin hcthread'
        exact ⟨cv, hcvmem, by rw [hcvid, hh']⟩

end Invariant
end Bbs
