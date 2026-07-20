---
id: 0025
title: 機能セッションごとのLean証明スコープ(横断的invariantは実装済み範囲に絞る)
date: 2026-07-20
importance: minor
decided_by: ai+user
status: 決定済
---

# 0025 機能セッションごとのLean証明スコープ(横断的invariantは実装済み範囲に絞る)

- 関連論点: なし(新規。F05実装セッションでの形式化作業中に判明)。
- 関連原典: C-05(スレッド作成後の編集不可)、AC05-4。`formal/Bbs/Invariant.lean` の `thread_immutable`。
- 影響範囲: `formal/Bbs/Invariant.lean`(`thread_immutable`に`Wf`相当の局所仮定を追加したうえで`sorry`のまま維持、代わりに`createThread_does_not_modify_existing_threads`を新設)。今後のF06〜F08実装セッションでの証明範囲の判断基準。

## 背景（原典が何を言い、何を言っていないか）

**[事実]** `CLAUDE.md`/セッション開始時の指示は「実装に入る前に、この機能に対応する不変条件（`formal/README.md` の一覧を参照）を証明する」ことをMustとしている。

**[事実]** `formal/Bbs/Invariant.lean` の `thread_immutable` は次のように定義されている。

```
theorem thread_immutable (db : Db) (steps : List Step) (t : Thread)
    (h : t ∈ db.threads) :
    ∀ t' ∈ (runAll steps db).threads, t'.id = t.id → t' = t := by sorry
```

`Step`は F01〜F08 の操作全種（`register`/`login`/`logout`/`updateDisplayName`/`createThread`/`deleteThread`/`createComment`/`deleteComment`）を含む帰納型であり、`thread_immutable`はこの**任意の長さ・任意の順序の操作列**にわたる一般形。`Bbs/Op.lean`はF06〜F08(`deleteThread`/`createComment`/`deleteComment`)の操作自体はすでに定義済みだが、Rust側の実装(`app/`)はF01〜F04のみが完了しており、F05(スレッド作成)が本セッションの対象。

**[事実(形式化作業中に判明した誤り)]** `thread_immutable`は`db`が整形式(スレッドIDが重複しない・`nextThreadId`が既存の全IDより大きい)であることを暗黙に仮定しないと成立しない。反例: `db.threads := [{id:=0,...,title:="a",...}]`、`db.nextThreadId := 0`という(`Wf.nextIdsFresh`に違反する)不正な`db`に対し`createThread`を1回実行すると、新規スレッドが同じ`id:=0`で追加され、内容の異なる2件が同じidを持つ。`lake build`の`#eval`で実際に反例を再現し確認した(作業ログ用に一時追加し、確認後削除)。

**[空白]** `thread_immutable`はこの前提(`Wf`の一部)を明示的な仮定として持っていない。`Wf`が全操作について保存されることの証明(`formal/README.md`「未起票の宿題」)自体もまだ着手されていない。

## 選択肢

### (a) `thread_immutable`をF05セッション内で一般形のまま証明する

- 利点: 既存の言明をそのまま満たせる。F06〜F08実装時に再証明が要らない。
- 欠点: `Step`の8コンストラクタ全てについて「`threads`/`nextThreadId`フィールドを変更しない(または`createThread`/`deleteThread`のみが変更し、かつその変更が`Wf`の必要部分を保つ)」ことを示す必要があり、F06〜F08(Rust側は未実装)の`Op`定義の詳細にまで証明が踏み込むことになる。F05単体のセッションに対して過大なスコープ。

### (b) `thread_immutable`は`sorry`のまま残し、`createThread`という単一操作に絞った代替の定理を新設する

- 利点: F05が実際に導入する操作(`createThread`)のみを扱えばよく、C-05/AC05-4の核心(「作成は既存スレッドを書き換えない」)をこのセッションのスコープ内で証明できる。`Wf`の必要部分(`threadIdsDistinct`/`nextIdsFresh`相当)を局所的な仮定として要求するだけで済み、`Wf`全体の保存証明を前倒しで背負わずに済む。
- 欠点: `thread_immutable`という一般形の言明は未証明のまま残る。F06〜F08実装時に再度証明作業が必要になる。

## 提案（および理由）

**(b) を採る。**

`thread_immutable`をこのセッションで無理に一般形のまま片付けようとすると、まだRust側に存在しないF06〜F08の`Op`定義の内部（`deleteThread`/`createComment`/`deleteComment`が`threads`/`nextThreadId`フィールドに触れないこと）まで証明範囲に含める必要があり、CLAUDE.mdが定める「1機能ずつ」の原則（TDD・段階的実装）とスコープの単位が食い違う。一方、`createThread_does_not_modify_existing_threads`はF05が導入する唯一の新規操作に絞っており、C-05/AC05-4が要求する「作成後の編集不可」の核心を過不足なくカバーする。

`Wf`全体ではなく、この証明に必要な2性質（`(db.threads.map (·.id)).Nodup`と`∀ t ∈ db.threads, t.id < db.nextThreadId`）のみを局所仮定として要求する設計にした。`Wf`構造体自体の保存証明はF01〜F03の時点でも未着手であり、それを本セッションで肩代わりする理由がない。

反例の発見自体は形式モデルの技術的な精緻化であり、`docs/`原典の解釈が変わるものではないため、大きな決定というより方針の記録として`minor`とした。

## 決定（2026-07-20 ユーザー判断）

- **`thread_immutable`（`formal/Bbs/Invariant.lean`）の言明そのものを修正する。** 仮定の無い当初の形は**偽**であり（上記の反例。id が重複した整形式でない`db`と`steps = []`だけで反証でき、レビューではこの反証が`sorry`を含まない形でLeanで実際に構成された）、**偽の命題は将来も証明できない**。`createThread_does_not_modify_existing_threads`が採ったのと同じ局所仮定 ―― `(db.threads.map (·.id)).Nodup` と `∀ t ∈ db.threads, t.id < db.nextThreadId` ―― を追加し、**真だが未証明**の言明へ直す。
- 修正後の`thread_immutable`は`sorry`のまま残す。証明には`Step`全種（F06〜F08はRust側未実装）がこの2性質を保つことを示す必要があり、F05単体のセッションには過大なため。**「未証明」であって「誤り」ではない**ことがdocコメントから読み取れる状態にする。
- F05のC-05/AC05-4は、`createThread`単体に絞った`createThread_does_not_modify_existing_threads`でカバーする。
- 今後F06〜F08を実装するセッションでは、その時点で追加される単一操作について同様のスコープを取るか、あるいはその時点で`Step`全種が実装済みになるため`thread_immutable`本体を証明するかを、そのセッションで判断する。
- 汎用的な方針として、**`formal/`に既存の横断的invariant（複数機能にまたがる`Step`/`runAll`ベースの言明）が実装未着手の操作を含む場合、そのセッションでは「今回追加する操作」に絞った代替の定理で不変条件Mustを満たしてよい**。既存の一般形の言明は`sorry`のまま残し、対応するコメントで理由と代替定理名を明記する。**ただし残す言明は真でなければならない** ―― 反例が見つかった言明は、その場で必要な仮定を補って真の形に直してから`sorry`を残すこと。仮定を欠いた偽の言明を「後で証明する」として放置しない。

## 影響

- `formal/Bbs/Invariant.lean`: `createThread_atomic`(decision 0002対応)・`createThread_does_not_modify_existing_threads`(本決定)・補助補題`nodup_map_eq_of_mem`/`ensure_true_eq`/`ensure_false_eq`を追加。`thread_immutable`は`hnodup`/`hfresh`の2仮定を追加したうえで`sorry`のまま残し、docコメントに「仮定を外すと偽になること」「現状は真だが未証明であること」を明記した。
- `thread_immutable`に依存する証明は無い（参照はドキュメントとコメントのみ）ため、仮定の追加による既存証明への影響は無い。
- 今後のF06(スレッド削除)・F07(コメント作成)・F08(コメント削除)実装セッションは、着手時に`thread_immutable`/`comment_body_immutable`/`deletion_irreversible`をこのセッション同様「スコープを絞るか、その時点で一般形を証明できるか」を判断すること。あわせて`comment_body_immutable`/`deletion_irreversible`も同種の`Wf`仮定を欠いている可能性があり、着手時に反例の有無を確認すること。

## 変更履歴

- 2026-07-20: ユーザーが内容を確認し承認（`decided_by: ai` → `ai+user`、`status: 提案` → `決定済`）。あわせて結論部を訂正した。当初は「`thread_immutable`は`sorry`のまま維持し、F06〜F08実装時にこの一般形へ拡張する想定」としていたが、仮定の無い当初の言明は偽であり将来も証明できないため、この記述は論理的に誤りだった。言明に`Wf`相当の局所仮定を追加して真の形へ直したうえで`sorry`を残す、という決定に改めた。
