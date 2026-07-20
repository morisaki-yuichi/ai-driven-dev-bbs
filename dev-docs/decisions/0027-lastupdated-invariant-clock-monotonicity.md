---
id: 0027
title: comment_bumps_lastUpdated（Leanモデル）に論理時計の単調性を仮定として追加する
date: 2026-07-20
importance: minor
decided_by: ai+user
status: 決定済
---

# 0027 comment_bumps_lastUpdated（Leanモデル）に論理時計の単調性を仮定として追加する

- 関連論点: なし（新規。F09実装差分のレビュー中に判明）。
- 関連原典: C-15（最終更新日時＝スレッド作成時刻または最新コメント投稿時刻）、AC09-4（最終更新日時はコメント投稿のたびに更新される）。
- 影響範囲: `formal/Bbs/Invariant.lean` の `comment_bumps_lastUpdated`（言明そのものを修正）。将来的には `Invariant.Wf` および F07（コメント作成）の実装セッションに波及する。

## 背景（原典が何を言い、何を言っていないか）

**[事実]** 原典 C-15 / AC09-4 は「コメントが投稿されるたびに最終更新日時が更新される」ことを要求する。`formal/Bbs/Query.lean` の `lastUpdatedAt` は `Nat.max t.createdAt (maxTime (コメントのcreatedAt))` として、この要求をモデル化している。

**[事実]** `formal/Bbs/Db.lean` の `tick` は `clock := clock + 1` としてから新しい値を返す。したがって `createComment` が新しいコメントに付ける時刻は、必ず**その時点の `db.clock + 1`** である。

**[事実（レビュー中に判明した誤り）]** `comment_bumps_lastUpdated` は当初、次のように言明されていた（`sorry` のまま未証明）。

```lean
theorem comment_bumps_lastUpdated (db : Db) (sid : SessionId) (tid : ThreadId) (b : String)
    (t : Thread) (ht : t ∈ db.threads) (ht' : t.id = tid) (cid : CommentId)
    (hok : (createComment sid tid b) db = (.ok cid, (createComment sid tid b db).2)) :
    let db' := (createComment sid tid b db).2
    lastUpdatedAt db t < lastUpdatedAt db' t := by sorry
```

この言明は **偽である**。`db` は任意の `Db` 値であり、`Db` 構造体にも `Invariant.Wf` にも「既存レコードの `createdAt` は `db.clock` 以下である」という制約が無い。そのため、論理時計より未来の `createdAt` を持つレコードを含む `db` を自由に構成できる。

反例（`formal/` 上で実際に構成し、`native_decide` で機械的に確認した）:

- `db.clock = 0`
- スレッド `t`（`id = 0`, `createdAt = 0`）
- そのスレッドへのコメント1件（`createdAt = 100`）

このとき `lastUpdatedAt db t = 100`。`createComment` は成功し（`hok` は満たされる）、新しいコメントに付く時刻は `tick` により `clock + 1 = 1` にしかならないため、`lastUpdatedAt db' t = max 0 (max 100 1) = 100`。したがって `100 < 100` は偽で、結論が成り立たない。

**この反例は `Wf` を満たす**（ID一意性・参照整合性・採番カウンタの新鮮さをすべて充足する）。つまり `Wf db` を仮定に足しても言明は救われない。

**[空白]** 原典は論理時計・単調性という概念を持たない（実時刻での運用を暗黙に想定している）。「モデル上の `clock` と各レコードの `createdAt` の関係をどう不変条件として言明するか」は形式化固有の論点であり、原典はこの粒度に言及していない。

## 選択肢

### (a) `Wf` に「時計が全レコードの `createdAt` を支配する」フィールドを追加する

- 利点: 単調性はモデル全体で成り立つべき構造的性質であり、`Wf`（Db の整合性述語）に置くのが本来の居場所。他の時刻に関する言明でも再利用できる。
- 欠点: `wf_empty` および F01〜F05 で既に証明済みの `Wf` 保存補題すべてに波及し、それぞれ「この操作は時計支配を保つ」ことの再証明が要る。F09（一覧表示）の差分としてはスコープが大きすぎる。

### (b) この言明に必要な単調性2つだけを局所的な仮定として取る

- 利点: 差分が当該定理1件に閉じる。既存の証明済み補題に影響しない。`nodup_map_eq_of_mem`（C-05補題）が「`Wf` 構造体を丸ごと要求せず、この証明に要る2性質だけを局所的な仮定として取る」という先例を既に確立しており、それと同じ形。
- 欠点: 同種の仮定が今後の時刻関連の言明で重複しうる（`Wf` へ集約する機会を先送りする）。

### (c) 偽と分かったうえで `sorry` のまま放置する

- 利点: 差分ゼロ。
- 欠点: **採らない。** `sorry` は「未証明」であって「真」ではないため、偽の言明を `sorry` で残すと、後続セッションが「証明すればよい」と誤解して到達不能な証明に時間を溶かす。F05 の `thread_immutable` が仮定不足で偽だった前例と同じ罠を再生産する。decision 0025・0026 が確立した方針（反例が見つかった言明は真の形に直してから証明または `sorry` を残す）にも反する。

## 提案（および理由）

**(b) を採る。**

(a) が構造的には正しい置き場所だが、F09 の差分でF01〜F05の証明済み補題に波及させるのはスコープ超過であり、decision 0025 のスコープ限定方針（実装済み範囲に絞る）と噛み合わない。(b) なら差分が1定理に閉じ、かつ既存の先例（`nodup_map_eq_of_mem`）と同じ形になる。

`Wf` への集約（(a)）は、この定理を実際に証明する F07（コメント作成）の実装セッションで、`createComment` の `Wf` 保存証明と合わせて検討する。

## 決定（2026-07-20 ユーザー判断）

`comment_bumps_lastUpdated` に次の2つの局所仮定を追加し、言明を真の形に直した。証明自体は F07 の範囲であり `sorry` のまま残す。ユーザーが承認した。

**`Wf` への集約（選択肢 (a)）を F07 で行うことも、同じ裁定で確定した。** 本決定は「(b) を恒久の形として採る」ものではなく、**F09 のサイクルではスコープを (b) に留める**というものである。F07（コメント作成）の実装セッションで、`Wf` に「既存レコードの `createdAt` は `db.clock` 以下」（スレッド・コメント・および将来の時刻付きレコード）を表すフィールドを追加し、`wf_empty` と F01〜F05 の `Wf` 保存補題を更新したうえで、本定理の局所仮定 `hclockT` / `hclockC` を `Wf db` からの導出で置き換える。`comment_bumps_lastUpdated` の証明自体も同じセッションで埋まるため、局所仮定の除去と証明の完了を1回の差分でまとめて行える。

```lean
    (hclockT : t.createdAt ≤ db.clock)
    (hclockC : ∀ c ∈ db.comments, c.createdAt ≤ db.clock)
```

この2仮定のもとでは `lastUpdatedAt db t ≤ db.clock` であり、新コメントの時刻は `db.clock + 1` なので `lastUpdatedAt db' t ≥ db.clock + 1 > lastUpdatedAt db t` となり、結論が従う。

## 影響

- `formal/Bbs/Invariant.lean`: `comment_bumps_lastUpdated` の言明に仮定2つを追加。反例と選択理由を doc コメントに記載。セクション7冒頭のコメントも修正した（当初「`sorry` で残した3件は真であることを変更していない」と書かれていたが、この定理については誤りだったため、3件それぞれの真偽検査結果を明記する形に改めた）。
- 併せて `sorted_by_commentCount`・`createdAsc_head_is_oldest` の2件も反例の有無を検査し、**いずれも真**であることを確認した（`leOf` が辞書式の全順序であり、挿入ソートが整列列を返すことから従う。`db` の健全性に依存しない）。言明の変更は不要。
- `Invariant.Wf`: 時計支配フィールドの追加は F09 のサイクルでは**行わない**（ユーザー裁定）。F07 の実装セッションで選択肢 (a) を実施し、`hclockT` / `hclockC` の2つの局所仮定を `Wf` からの導出に置き換える。したがって本決定の (b) は暫定形であり、F07 でこの決定に (a) への移行を変更履歴として追記すること。
- 実装側（`app/`）への影響は無い。この決定は形式モデルの言明のみを対象とし、実装の挙動を変えない。また `createComment`（F07）は未実装のため、対応する実装コードがまだ存在しない。
- ユーザー承認済み（`decided_by: ai+user`）のため、未承認決定に要求される暫定参照コメントは不要。現時点では対応する実装コード（`createComment`）がまだ存在しない。

## 変更履歴

- **2026-07-20**: 起票時は AI 単独判断（`decided_by: ai` / `status: 提案`）だった。ユーザーがレビューのうえ承認し、`ai+user` / `決定済` に更新した。併せて、`Wf` への時計支配フィールドの集約（選択肢 (a)）を F07 のサイクルで行うという方針が同じ裁定で確定したため、「決定」節と「影響」節にその想定を明記した。
