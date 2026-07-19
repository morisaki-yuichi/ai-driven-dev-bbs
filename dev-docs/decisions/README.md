# 決定記録（decisions）

仕様が規定していない点について、AI が置いた前提・提案と、人間の判断を記録する場所。

## ファイル名規約

```
dev-docs/decisions/<4桁連番>-<スラッグ>.md
```

- `<4桁連番>`: プロジェクト通番。欠番を作らず、取り下げた決定も番号を再利用しない。
- `<スラッグ>`: 内容を表す英小文字ケバブケース。

## ステータス

| 値 | 意味 |
| :--- | :--- |
| `決定済` | 人間の判断が下りた。実装はこれに従う。 |
| `提案` | AI が前提を置いて先へ進めている。人間の承認・棄却待ち。**覆る前提で扱う。** |
| `保留` | 判断材料が足りず、後続フェーズへ持ち越す。 |
| `取下げ` | 不要になった。理由を残す。 |

## importance（影響の大きさ）

覆ったときに何がやり直しになるかで決める。**「議論が難しかったか」ではない。**

| 値 | 意味 | 目安 |
| :--- | :--- | :--- |
| `critical` | 覆すとアーキテクチャ・スキーマ・技術選定のいずれかがやり直しになる。 | 0002（トランザクション境界）、0007（セッションの保存先）、0008（レンダリング方式）、0016（技術スタック） |
| `major` | 覆すと複数ファイルの実装変更が要るが、構造は保たれる。 | 0003（文字数の単位）、0011（検索の照合方式）、0014（物理削除） |
| `minor` | 覆しても局所的な修正で済む。 | 0005（表示名の空文字）、0013（ページ境界） |

## decided_by（誰が決めたか）

| 値 | 意味 | 扱い |
| :--- | :--- | :--- |
| `user` | 人間が起点となって指示した。AI は選択肢を出していない。 | 確定。 |
| `ai+user` | AI が背景整理・選択肢・提案を出し、人間が採否を判断した。 | 確定。 |
| `ai` | **AI が単独で決め、人間の承認を経ていない。** | **暫定。レビュー対象として優先的に洗い直す。** コード側にも同じ番号のコメントを残すこと。 |

## テンプレート

[`TEMPLATE.md`](TEMPLATE.md) を複製して使う。

> **フォーマットの過渡期について**: `TEMPLATE.md` の YAML frontmatter（`id` / `title` / `date` / `importance` / `decided_by` / `status`）は **0016 以降に適用**する。0001〜0015 は frontmatter を持たない旧形式のままとし、その `importance` / `decided_by` は下の索引が正とする。旧形式ファイルを個別に書き換えることはしない（決定の中身が変わっていないのに履歴上のノイズを増やさないため）。

## 記述ルール

- **[事実]（原典に書いてある）と [解釈]（AI が埋めた）を必ず分けて書く**。要件分析ドキュメントと同じ表記凡例に従う。
- 原典（`README.md` / `docs/`）の該当箇所を必ず引用または参照する。
- 「提案」のまま実装に進んでよい。ただしその場合、コード側にも同じ番号のコメントを残し、後から追跡できるようにする。
- 決定が覆った場合は該当ファイルを更新し、変更履歴を末尾に追記する（新規番号は起こさない）。

## 一覧

| 番号 | タイトル | ステータス | importance | decided_by | 関連論点 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| [0001](0001-readme-editing-policy.md) | README.md へのセットアップ手順追記の可否 | 決定済 | major | ai+user | D00 |
| [0002](0002-action-failure-semantics.md) | 操作が失敗したときの状態（部分書き込みの禁止） | 決定済 | critical | ai+user | — |
| [0003](0003-character-counting-and-charset.md) | 文字数の数え方と文字種の定義 | 決定済 | major | ai+user | D15, D16 |
| [0004](0004-empty-input-definition.md) | 「空」の定義とトリム方針 | 決定済 | major | ai+user | D16 |
| [0005](0005-display-name-emptiness.md) | 表示名の空文字を許すか | 決定済 | minor | ai+user | D16 |
| [0006](0006-validation-order-and-multiple-errors.md) | バリデーションの検査順序と複数エラーの提示 | 決定済 | major | ai+user | D11 |
| [0007](0007-session-multiplicity.md) | 多重ログインと既存セッションの扱い | 決定済 | critical | ai+user | D04 |
| [0008](0008-browser-back-is-out-of-model.md) | AC03-2（ブラウザバック）はデータモデル外の要件 | 決定済 | critical | ai+user | D01, D04 |
| [0009](0009-time-granularity-and-sort-tiebreak.md) | 時刻の粒度・同時刻の衝突・ソートのタイブレーク | 決定済 | major | ai+user | D17, D12 |
| [0010](0010-last-updated-derivation.md) | 最終更新日時の導出方法とコメント削除の影響 | 決定済 | major | ai+user | — |
| [0011](0011-search-matching-and-normalization.md) | 検索の照合方式・正規化・空クエリ | 決定済 | major | ai+user | D07 |
| [0012](0012-search-scope.md) | 検索対象の範囲（タイトル／削除済みコメント） | 決定済 | major | ai+user | D08, D09 |
| [0013](0013-pagination-edge-cases.md) | ページネーションの境界（範囲外ページ・詳細ページ） | 決定済 | minor | ai+user | — |
| [0014](0014-thread-deletion-is-physical.md) | スレッド削除は物理削除とする | 決定済 | major | ai+user | D02 |
| [0015](0015-display-name-resolution.md) | 表示名の解決方式（JOIN か非正規化か） | 決定済 | major | ai+user | D03 |
| [0016](0016-tech-stack.md) | 技術スタックの選定（Rust + Axum + Askama + sqlx + PostgreSQL） | 決定済 | critical | ai+user | D01, D02 |
| [0017](0017-deleted-comment-text-location.md) | 削除済みコメント固定文言（C-01）の集約先 | 決定済 | minor | ai+user | 新規（フェーズ2レビュー上流エスカレーション） |
| [0018](0018-setup-doc-location.md) | セットアップ・起動手順ドキュメントの配置とREADME参照方針 | 決定済 | minor | ai+user | D00 |
| [0019](0019-schema-details.md) | D02: スキーマ詳細（主キー型・URLのID形式・インデックス） | 決定済 | major | ai+user | D02 |
| [0020](0020-profile-edit-path.md) | D10: ユーザー編集画面（P06）のパス | 決定済 | minor | ai+user | D10 |
| [0021](0021-csrf-protection.md) | D05: CSRF対策の方式（Origin検証 + セッション非依存の二重送信トークン） | 決定済 | major | ai+user | D05 |
| [0022](0022-register-password-field-not-preserved.md) | 登録フォーム失敗時、パスワード欄だけは値を再表示しない | 決定済 | minor | ai+user | 新規（F01実装中に判明） |
| [0023](0023-no-native-client-validation.md) | フォームに novalidate を付け、ブラウザネイティブのクライアント側検証を採用しない | 決定済 | major | ai+user | 新規（F01実装レビューで顕在化） |
| [0024](0024-register-success-flash-via-query-param.md) | 登録成功後のログイン画面での通知方式（クエリパラメータによるフラッシュ表示） | 決定済 | minor | ai+user | 新規（H-12観測性改善セッションで判明） |

### 索引: `decided_by = ai`（人間の承認を経ていない暫定決定）

**該当なし（0件）。**

2026-07-20 時点で、0001〜0024 のすべてが `ai+user`（AI が提案し人間が採否を判断）である。0019〜0022・0024 は起票時点では AI 単独判断（`decided_by: ai`）だったが、いずれも同日中にユーザーが承認し `ai+user` に更新した。この索引は、後続フェーズでAIが単独判断で先へ進めた決定を洗い出すために置く。**ここに項目が増えた場合、それは「レビューされていない前提の上に実装が積まれている」ことを意味する**ので、フェーズの区切りで必ず棚卸しする。

### 索引: `importance = critical`（覆すとやり直しが大きい決定）

- [0002](0002-action-failure-semantics.md) 操作が失敗したときの状態 → 1リクエスト＝1トランザクション
- [0007](0007-session-multiplicity.md) 多重ログインと既存セッションの扱い → セッションを DB に永続化
- [0008](0008-browser-back-is-out-of-model.md) ブラウザバック → SSR / MPA 限定
- [0016](0016-tech-stack.md) 技術スタック → Rust + Axum + Askama + sqlx + PostgreSQL

この4件は互いに依存している。0002・0007 が RDBMS を要求し、0008 が SSR/MPA を要求し、その2つが 0016 の Tier 0 ゲートになっている。**0016 を覆す場合は 0002・0007・0008 が前提として生きているかを先に確認すること。**
