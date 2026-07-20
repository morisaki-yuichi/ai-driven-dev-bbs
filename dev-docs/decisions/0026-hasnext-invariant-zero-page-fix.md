---
id: 0026
title: hasNext_iff_more（Leanモデル）のp=0反例修正
date: 2026-07-20
importance: minor
decided_by: ai+user
status: 決定済
---

# 0026 hasNext_iff_more（Leanモデル）のp=0反例修正

- 関連論点: なし（新規。F09実装セッションでの形式化作業中に判明）。
- 関連原典: C-12（ページネーションの前後ボタン制御）、decision 0013（範囲外・境界ページの扱い）。
- 影響範囲: `formal/Bbs/Invariant.lean` の `hasNext_iff_more`（言明そのものを修正）。`page_size_bound`・`first_page_no_prev`・`pagination_preserves_order`・`sortThreads_perm` と合わせてF09対応でこのセッションに証明した5件のうちの1件。

## 背景（原典が何を言い、何を言っていないか）

**[事実]** `formal/Bbs/Query.lean` の `paginate` は、decision 0013 の決定（`?page=0`等の範囲外値は1ページ目として丸める）に従い、`n = 0` を `p := 1` に丸めてからページを切り出す。この丸めは `pageNumber`・`hasPrev`・`hasNext` の全フィールドに反映される。

**[事実(形式化作業中に判明した誤り)]** `hasNext_iff_more` は当初、丸め前の生の `p` を使って次のように言明されていた。

```
theorem hasNext_iff_more (db : Db) (k : SortKey) (p : Nat) :
    (threadList db k p).hasNext = true ↔
      db.threads.length > p * pageSize := by sorry
```

この言明は **`p = 0` のとき偽である**。反例: `db.threads.length = 5`（1ページに収まり2ページ目は無い）のとき `p = 0` を渡すと、`paginate` は丸めた `p' = 1` で処理するため `hasNext = false`。しかし言明の右辺は丸め前の `p = 0` を使うため `5 > 0 * pageSize = 5 > 0` で真になり、`hasNext = true ↔ 真` は成り立たない。`lake build` 上でこの反例を実際に構成して確認した。

**[空白]** 原典・decision 0013 とも「範囲外ページの丸め」自体は決めているが、その丸めを「一覧の次ページ有無判定を表す不変条件」としてどう言明するかは形式化固有の論点であり、原典は当然この粒度には言及していない。

## 選択肢

### (a) 言明はそのまま（`p * pageSize`）にして、仮定 `1 ≤ p` を追加する

- 利点: 証明が単純。実際のHTTPハンドラは `web/params.rs` の `ListParams::parse` が `page=0`/負数/非数値をパース層で既に1に丸めてから `paginate` を呼ぶため、実運用では `p = 0` はこの関数に渡らない。
- 欠点: `domain::query::paginate`（および Lean の `paginate`）自身も独立に `n = 0` を丸める「二重の安全策」を持っている（`app/src/domain/query.rs` のコメント「HTTPパース層での丸めと二重に安全側へ倒す」）。この二重防御そのものの正しさを不変条件がカバーしなくなる。

### (b) 言明の右辺を `paginate` 自身の丸めロジックへ合わせる（`(if p = 0 then 1 else p) * pageSize`）

- 利点: `paginate`（Lean・Rust実装とも）が実際に行っている丸めを過不足なく写し取れる。`p = 0` を含む**任意の `Nat`** について不変条件が成り立ち、HTTP層の丸めに依存しない、より強い（防御的な）性質になる。
- 欠点: 証明の中で `if` の場合分け（`split`）が1回余分に必要。

## 提案（および理由）

**(a) を採る。**

decision 0025 が確立した方針「反例が見つかった言明は、必要な仮定・書き換えを補って真の形に直してから証明または `sorry` を残す。仮定を欠いた偽の言明を放置しない」を満たすのは (a)・(b) のどちらでも同じであり、選択の基準は「どちらが不変条件として意味を持つか」になる。

**(b) は不変条件が実装を追認する形になる。** 右辺に `paginate` の丸め規約（`if p = 0 then 1 else p`）をそのまま転記すると、`p = 0` の1点において「実装がそう書かれているから正しい」以上のことを言わなくなる。丸め規約を後で変えれば言明も追随して書き換わり、両者が食い違うことが原理的に起きない ―― つまりこの1点で、不変条件が実装を**拘束する**役割を失う。形式モデルを置く目的（実装から独立に、原典の要求を述べて実装を縛る）に照らすと、これは避けたい形である。

**(a) は語る定義域を意味のある範囲に限る形。** 原典 C-12 はページ番号が1始まりであることを前提にしており、`1 ≤ p` はその前提を仮定として明示したものにすぎない。右辺は原典どおりの `p * pageSize` のままなので、実装がどう丸めようと言明は変わらず、実装を拘束する力を保てる。

(a) の欠点として挙げた「`paginate` 自身の二重防御を不変条件がカバーしなくなる」点は、実際には損失が小さい。`p = 0` を1ページ目に丸める挙動そのものは、Lean 側は `pagination_preserves_order` の doc コメント（`p = 0` でも成り立つことを明記）、Rust 側は `domain::query::paginate` の単体テスト `paginate_page_zero_is_treated_as_page_1` と結合テスト `thread_list_page_zero_is_treated_as_first_page` が押さえており、`hasNext_iff_more` がそこまで担う必要はない。加えて呼び出し側の `ListParams::parse`（`app/src/web/params.rs`）が `page` を1以上に丸めてから渡すため、この仮定は実運用で常に満たされる。

## 決定（2026-07-20 ユーザー判断）

`hasNext_iff_more` に仮定 `1 ≤ p` を追加し、右辺は原典どおりの `p * pageSize` のままとして、`sorry` を外して証明を完了した。ユーザーが承認した。

```lean
theorem hasNext_iff_more (db : Db) (k : SortKey) (p : Nat) (hp : 1 ≤ p) :
    (threadList db k p).hasNext = true ↔
      db.threads.length > p * pageSize := by
  ...
```

## 影響

- `formal/Bbs/Invariant.lean`: `hasNext_iff_more` の言明・証明を修正。同じセクションに `insertBy_length`・`sortBy_length`（`sortThreads_perm` の土台）を新設し、`page_size_bound`・`first_page_no_prev`・`pagination_preserves_order`・`sortThreads_perm` も合わせて証明した（これらは反例が無く、言明の書き換えは不要だった）。
- `hasNext_iff_more` に依存する既存証明は無い（`Bbs/Scenario.lean` の煙試験は `threadList`/`paginate` を直接叩いており、この定理を経由しない）ため、既存証明への影響は無い。
- 実装側（`app/src/domain/query.rs` の `paginate`）は元から丸め込み込みで実装されており、コード変更は不要。この決定は形式モデルの言明を実装の実際の挙動に合わせて修正したものであり、実装の挙動そのものを変えるものではない。
- ユーザー承認済み（`decided_by: ai+user`）のため、未承認決定に要求される `// decision 0026` の暫定参照コメントは不要。今回のF09実装では `paginate` のRust単体テストは既存のまま（`p=0` を1ページ目に丸める挙動は `paginate_page_zero_is_treated_as_page_1` で既にカバー済み）で、この決定を直接参照するコード変更は発生していない。

## 変更履歴

- **2026-07-20**: 初版は (b)（右辺に `paginate` の丸め規約 `(if p = 0 then 1 else p) * pageSize` を転記する）を AI 単独判断で採用していた。ユーザーのレビューで「その形は `p = 0` の1点で不変条件が実装を拘束するのではなく追認するものになっている」と指摘され、裁定により **(a)（仮定 `1 ≤ p` を置き、右辺は原典どおり `p * pageSize` のままとする）へ変更**した。`formal/Bbs/Invariant.lean` の `hasNext_iff_more` の言明・doc コメント・証明（`split <;> omega` は仮定 `hp` を文脈に持つため変更不要だった）と、本文書の「選択肢」「提案」「決定」「影響」の各節を、この裁定に合わせて更新した。併せて `decided_by` を `ai+user`、`status` を `決定済` に更新した。
