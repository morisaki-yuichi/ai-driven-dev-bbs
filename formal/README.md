# formal — BBS 仕様の Lean 4 形式モデル

`docs/` の要件を Lean 4 の状態機械として書き下したもの。**実装ではない**。
目的は「原典の受け入れ基準が状態の性質として何を意味するか」を精密化し、
その過程で**原典が答えていない点を機械的にあぶり出す**こと。

## ビルド

```
cd formal
lake build
```

Lean 4.32.0（`lean-toolchain` で固定）。mathlib 等の外部依存は無い。
`Bbs/Invariant.lean` の定理・補題は**すべて証明済み**で、`sorry` はゼロ
（`declaration uses 'sorry'` 警告は出ない）。当初は全件が `sorry` の言明集だったが、
機能の実装に合わせて順次証明し、F12 のレビュー対応で最後の1件
（`displayName_propagates`）が埋まった。

## 構成

| ファイル | 内容 |
| :--- | :--- |
| `Bbs/Basic.lean` | ID・時刻・`Error`・バリデーション違反の型 |
| `Bbs/Db.lean` | 状態 `Db` と作用モナド `Action = StateM Db (Except Error _)` |
| `Bbs/Validation.lean` | パスワード強度・表示名・空チェックの述語（C-02〜C-04） |
| `Bbs/Op.lean` | F01〜F08 の状態遷移操作 |
| `Bbs/Query.lean` | F09〜F13（一覧・詳細・検索・ソート・ページネーション） |
| `Bbs/Invariant.lean` | 証明すべき性質の言明と証明（定理・補題あわせて 96 件、`sorry` ゼロ） |
| `Bbs/Scenario.lean` | 評価シナリオ 01〜05 をモデル上で再生する煙試験（`#eval`、36 チェック） |

## 設計上の要点

- **書けない操作は仕様上あってはならない操作。** スレッドのタイトル・本文やコメント本文を
  更新する操作は `Bbs/Op.lean` に**存在しない**。C-05（作成後編集不可）を型レベルで表現している。
- **失敗時は状態を巻き戻す。** `Action.bind` が明示的にそう定義してある（decision 0002）。
  `StateM Db (Except Error _)` は型としてはこれを強制しないので、証明対象（`NoWriteOnError`）にした。
- **表示名は投稿に持たせない。** `authorId` だけを保持し表示時に解決するので、
  AC04-2（表示名変更が過去の投稿に反映）はデータ構造から自動的に従う（decision 0015）。
- **削除済みコメントの扱いは「数える」で統一。** コメント数・最終更新日時・スレッド削除可否の
  すべてで削除済みを勘定に入れる（C-06 / C-16 / D13）。検索だけは方式が未確定なので
  `DeletedCommentSearchPolicy` として**両方式を残してある**（decision 0012）。
- **モデルの外にある要件がある。** AC03-2（ブラウザバック）は HTTP キャッシュ制御の問題で、
  サーバ状態の性質としては表現できない（decision 0008）。モデルの証明が全部通っても保証されない。

## 煙試験

`Bbs/Scenario.lean` は `docs/evaluation/scenarios/` の手順をモデル上で再生する。
H-08〜H-10 に合わせて空DBから始め、シナリオ間で状態をクリアしない。
`lake build` の出力に結果が出る（現在 36/36 パス）。

これは証明ではなく実行による確認だが、実際にモデルのバグを1件見つけている
（do 記法の `return` が早期リターンとして働き、表示名の更新が丸ごと飛んでいた）。

## 未起票の宿題

- `Wf`（DB 整合性）の保存は `register` / `login` / `createThread` /
  `createComment` / `deleteThread` の5操作ぶんのみ。`updateDisplayName` /
  `deleteComment` の保存補題と、`runAll`（`Step` 列）へ持ち上げた一般形が残っている
  （`Bbs/Invariant.lean` セクション1.1 の「何が残っているか」）。
  これが揃うと、実装のスキーマ制約（外部キー・ユニーク制約）の設計根拠になる。
- **Lean の証明は Rust 実装を機械的に拘束しない。** 両者の対応は人手で維持している
  （F12 の変異テストで、Rust 側の比較関数を壊しても `lake build` が成功することを確認済み）。
