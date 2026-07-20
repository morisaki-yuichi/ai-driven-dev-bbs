# CLAUDE.md

このファイルはリポジトリ直下に置く**恒久ルール**。全フェーズ・全セッションを通じて有効。個々の判断の記録や作業ログはここに書かず、`dev-docs/decisions/`（決定記録）と `dev-docs/session-logs/`（作業ログ）に置く。本ファイルは「毎回守るルール」だけを持つ。

## プロジェクト概要

ブラウザ上で動く掲示板（BBS）アプリケーションを、人間が直接コードを書かずにAIエージェントを指揮して開発する教材プロジェクト。原典の仕様は `README.md` と `docs/` 配下（人間が書いたもの・読み取り専用）。要件の構造化・論点整理・技術選定・形式モデル化は `dev-docs/` 配下にAI成果物として蓄積している。

- 要件の正典: `dev-docs/requirements-analysis.md`
- 評価ハーネスの不変条件（H-01〜H-13）: `dev-docs/harness-invariants.md`
- 形式モデル（Lean 4）: `formal/`
- 決定記録: `dev-docs/decisions/`（仕様が規定していない論点への結論。番号付き）
- 基盤構築計画: `dev-docs/foundation-plan.md`

評価は自然言語シナリオ（`docs/evaluation/scenarios/`）をAIが `agent-browser` で解釈・実行する形で行われる（H-12）。Playwright等の自動テストランナーは使われない。**画面上の表示・状態変化が自然言語で観測可能であること**が常に前提になる。

開発環境には `.mcp.json` で `playwright` MCP（`@playwright/mcp`）を用意しているが、これは**開発中のAIが自分の実装を素早く目視確認するための手段**であり、評価そのもの（`agent-browser` による自然言語判定）の代替ではない。Playwrightベースの自動E2Eテストは作らない——H-12が想定しない資産を積むことになる。`dev-docs/ui-ux-guidelines.md` §6 が定める `agent-browser` 固有の操作性（隠し要素・ネイティブダイアログ・トーストの消失速度）は、Playwrightでの目視確認だけでは検証できないことに注意する。

## 確定スタック（[decision 0016](dev-docs/decisions/0016-tech-stack.md)）

| レイヤ | 採用 |
| :--- | :--- |
| 言語 | Rust |
| Web フレームワーク | Axum |
| テンプレート | Askama（SSR）＋ 最小限のプレーン CSS |
| データ層 | sqlx（`query_as!` マクロ ＋ `.sqlx/` オフラインキャッシュ、`SQLX_OFFLINE=true`） |
| DB | PostgreSQL 17 |
| 認証／セッション | 自作 `sessions` テーブル ＋ Cookie属性 `HttpOnly` / `SameSite=Lax` / `Path=/`（`Secure`はHTTP評価環境のため付けない）。有効期限は設けない（[decision 0007](dev-docs/decisions/0007-session-multiplicity.md)）。パスワードは `argon2` crate |
| 実行環境 | Docker Compose（app + db）。開発時はホストの `cargo` を使い、DBのみコンテナ |
| テスト | `cargo test` ＋ `#[sqlx::test]`（テストごとに使い捨てDB） |

採用理由・却下した代替案・トレードオフの全文は decision 0016 と `dev-docs/foundation-plan.md` §1 を参照。**このスタックの採否をこのプロジェクトの中で再判断しない**（覆すなら decision 0016 の変更履歴として記録する）。

ディレクトリ構成・スキーマ方針・マイグレーション方針は `dev-docs/foundation-plan.md` §2〜4 を正とする。

**再現性（G）**: `Cargo.lock` と `rust-toolchain.toml` をコミットし、ツールチェーンとクレート版を固定する（[foundation-plan §1.9](dev-docs/foundation-plan.md#19-再現性g)）。`.sqlx/` のコミットと合わせて、評価者のクリーンな clone からのビルドを再現可能にする「コミット必須の生成物」の一群。

**Askamaのバージョンリスク**: Askamaは実装初回に**実測してバージョンを固定**する（0016 §7.1）。rinjaのフォークと統合を経てAPIに変遷があり、本スタックで生成信頼性リスクが最も高い箇所。記憶で書かず、`context7` MCP（`.mcp.json`）で公式ドキュメントを確認して書く。深刻化した場合の第一代替は maud（[foundation-plan §1.3](dev-docs/foundation-plan.md#13-テンプレート--描画askamassr--最小限のプレーン-css)）。

## 主要コマンド

開発中（ホストの `cargo` を使い、DBのみコンテナで動かす — [foundation-plan §1.7](dev-docs/foundation-plan.md#17-実行環境-docker-compose-app-db)）:

```bash
docker compose up -d db        # DBのみ起動
cargo check                    # 型検査のみの高速パス（既定でこれを使う）
cargo build
cargo run
cargo test                     # ドメイン層の純粋テスト + #[sqlx::test]
cargo sqlx prepare -- --all-targets  # SQLを変更したら必ず実行し .sqlx/ をコミットする
cargo fmt
cargo clippy
```

評価者経路の検証（本番相当。クリーンな clone から通ることを定期的に確認する — [foundation-plan §6.1](dev-docs/foundation-plan.md#61-b-緩和策を評価者経路に漏らさない-h-06--h-13-の保護)）:

```bash
docker compose up              # app + db を1コマンドで起動
docker compose down -v         # 名前付きボリュームごと削除し、空DBへリセット（D14）
```

Lean 4 形式モデル（`formal/`）:

```bash
lake build
```

## 開発ルール（人間とAIの役割分担）

- **人間はソースコード・git・`docs/`・`dev-docs/` を直接編集しない。** 実装・デバッグ・git操作（commit / push / branch など）はすべてAIが行う。
- 人間が編集してよいのは、AIの動作を制御するファイル（`CLAUDE.md` 本体、`.claude/` 配下、`.github/` 配下）とエディタ等の環境設定ファイルのみ。
- **AIが行き詰まり、人間がデバッグ・介入した場合は、原因と解決策のフィードバックを人間から受け取り、再発防止のためルール（本ファイル）・スキル（`.claude/skills/`）・ドキュメント（`dev-docs/`）に反映すること。** 同じ理由で人間の手を借りる事態を繰り返さない。

## TDD: Red → Green → Refactor

実装は原則としてこのサイクルで進める。

1. **Red**: 振る舞いを表すテストを先に書き、失敗することを確認する。
2. **Green**: テストを通す最小限の実装を書く。
3. **Refactor**: テストが通った状態を保ったまま整理する。

ドメイン層（`domain/`）はDB非依存の純粋関数なのでミリ秒で回る。ここを厚くテストすることが、次節の実装方針の前提。

## 実装方針: Action / Calculation / Data の分離

ロジックは可能な限り**純粋な計算（Calculation）**に寄せる。入力→出力のみで副作用を持たず、決定的であること。**副作用（Action）**——DB・セッション・HTTP・時刻・乱数——は薄く隔離する（functional core / imperative shell）。**Data**（起きた出来事の事実）は型・構造体で表す。

- `domain/` に DB からもフレームワークからも独立した純粋層を置き、`formal/Bbs/` の Lean モデルの対応先とする（[foundation-plan §2](dev-docs/foundation-plan.md#2-ディレクトリ構成) の層の意図）。`query_as!` は `db/` の外に漏らさない。
- 純粋な核を厚くテストすることで、高コストな `agent-browser` 越しのE2E確認への依存を減らす。E2Eはあくまで「配線が繋がっているか」の確認に留める。
- **これは既定の型であり、強制の分類ではない。** 単純なCRUDに過剰な層・間接を作らない。1つのハンドラで完結する処理を、この分離のためだけに3ファイルへ分割しない。
- **1リクエスト＝1トランザクション**（[decision 0002](dev-docs/decisions/0002-action-failure-semantics.md)、critical）。Action層の副作用のうちDB書き込みは、ハンドラの入口でトランザクションを開始し、`Err` を返す経路では必ずロールバックする。この規律をミドルウェアまたはヘルパで1箇所に固め、各ハンドラに書き散らさない（[foundation-plan §3](dev-docs/foundation-plan.md#3-データ永続化スキーマ方針)）。

## 意図の置き場所（How / What / Why / Why-not）

| 置き場所 | 内容 |
| :--- | :--- |
| コード | How（どう実装したか） |
| テスト | What（振る舞いの仕様） |
| コミット本文 | Why（なぜこの変更をしたか）。大きなWhyは decision に委ね、参照するのみで重複させない |
| コメント | Why・Why-not（非自明な理由、却下した代替案）。**Howの再説明は書かない**。公開APIのdocコメント・不変条件・警告は別枠で可 |

排他ルールではなく「どこに何を置くか」の指針。迷ったら、そこに書こうとしている情報が How なのか Why なのかを自問する。

## Definition of Done

1機能・1変更が完了したと言えるための最低条件。

- [ ] 振る舞いを表すテストがあり、`cargo test` が通る。
- [ ] `cargo clippy` に警告が残っていない。`cargo fmt` を適用済み。
- [ ] SQLを変更した場合、`cargo sqlx prepare -- --all-targets` を実行し `.sqlx/` の更新をコミットに含めている。
- [ ] UIに関わる変更は `dev-docs/ui-ux-guidelines.md` の要件（状態網羅・フィードバック・二重バリデーション・a11y・レスポンシブ・`agent-browser` 操作性）を満たしている。固定文言（C-01、同ファイル§8）を変更していない、または新規に埋め込んだ場合は字句一致を確認した。
- [ ] `dev-docs/harness-invariants.md`（H-01〜H-13）に違反していない。
- [ ] 仕様の未規定点を新たに解釈・決定した場合、`/log-decision` で記録している。
- [ ] コミットは `dev-docs/workflow.md` の規約（Conventional Commits、1論理変更1コミット、ユーザー承認後）に従っている。
- [ ] ビルド設定（`.cargo/config.toml` / `Cargo.toml` / `Dockerfile`）を変更した場合、クリーンな clone から `docker compose up` が通ることを確認した（[foundation-plan §6.1](dev-docs/foundation-plan.md#61-b-緩和策を評価者経路に漏らさない-h-06--h-13-の保護)。mold/lldのリンカ指定が評価者経路に漏れるとH-06/H-13を破る）。

## 決定記録ルール

仕様（`docs/`）が規定していない論点に結論を出したら、実装を進める前に **`/log-decision`**（`.claude/skills/log-decision/`）で `dev-docs/decisions/` に記録する。番号採番・frontmatter・`decisions/README.md` の索引更新まで一貫して行われる。既存の決定記録・命名規約・importance/decided_byの定義は `dev-docs/decisions/README.md` を参照。

- 「提案」（`decided_by: ai`）のまま実装に進んでよいが、コード側に同じ番号のコメントを残し、後から人間が洗い出せるようにする。
- 決定が覆った場合は既存ファイルを更新し変更履歴を追記する。新規番号は起こさない。

## セッションログ記録ルール

**全フェーズ・毎回のセッションで**、作業の区切りに **`/log-session`**（`.claude/skills/log-session/`）を実行し、`dev-docs/session-logs/` に記録を残す。形式・ファイル名規約は `dev-docs/session-log-format.md` を正とする。decision記録とは別系統であり、混同しない（決定記録＝論点ごとの規範、セッションログ＝時系列の作業日誌）。

## UI/UX 必須要件

UIに関わる作業（画面・フォーム・エラー表示など）に着手する前に、必ず **`dev-docs/ui-ux-guidelines.md`** を読む。状態網羅、送信中/成功/失敗フィードバック、クライアント+サーバの二重バリデーション、a11y、レスポンシブ、`agent-browser` での操作可能性（隠し要素にアクションを強いない）を定めている。

## セキュリティ必須要件

- **XSS対策**: Askama はデフォルトでHTMLエスケープする。生の文字列をエスケープなしで埋め込む処理（`|safe` 相当）は使わない。
- **CSRF対策**: 破壊的操作（作成・削除・更新）はPOSTで受け、状態変更エンドポイントにCSRFトークン検証を入れる（方式の詳細はフェーズ2以降のdecisionで確定する。D05）。**H-07（秘匿キーがある場合のみユーザーに伝え、それ以外はAIが自動実行できること）との整合に注意する**: CSRFトークンやセッション署名に固定鍵を使う場合、それを必須の環境変数にすると評価者はclone直後（H-13）に値を用意できず起動できない。D05を確定する際は、人間の手入力を要する秘密鍵を必須にしない（`.env.example` は「秘匿キーは無い想定」— [foundation-plan §2](dev-docs/foundation-plan.md#2-ディレクトリ構成)）という前提を踏まえる。
- **SQLインジェクション対策**: `sqlx::query_as!` 等のマクロ経由のパラメータ化クエリのみを使う。文字列結合でSQLを組み立てない。
- **パスワードハッシュ**: 平文保存・可逆暗号は禁止。`argon2` crate でハッシュ化する。
- **セッション失効**: ログアウト時はサーバ側の `sessions` レコードを削除する。認証必須画面には `Cache-Control: no-store` を付与し、ログアウト後のブラウザバックでキャッシュ経由の表示が起きないようにする（C-11 / AC03-2、[decision 0008](dev-docs/decisions/0008-browser-back-is-out-of-model.md)）。

## 非収束時のエスカレーション

**同一の失敗**（同じテストが同じ理由で赤のまま／同種のエラーが場所を変えて再発する）への修正試行が**3回連続で進展しない**場合、それ以上の試行を止める。試したこと・立てた仮説・残っている症状を整理して報告し、ユーザーの判断を待つ。

詰まった状態のまま大規模な書き換えや場当たり的な変更を積み増して、コードを荒らさないこと。回数（3回）は運用しながらユーザーが調整してよい初期値。

## やってはいけないこと

- **`docs/` 配下と `README.md` を編集しない。** 人間が書いた仕様であり、原則読み取り専用として扱う。AI側の成果物は `dev-docs/` に置く。
  - **例外（README.mdへの参照追加のみ）**: 評価ハーネスがREADME.md起点でのセットアップ・起動を要求する（H-06）ため、その充足に必要な**手順への参照追加**（1行程度のリンク追加）に限り、最小限の追記を認める。手順の**本文**はREADME.mdに書かず、`docs/` の外の別ドキュメント（例: `SETUP.md`）に置く。**追加のみ・再構成なし**（既存記述の削除・書き換え・並べ替えをしない）。この範囲であれば事前承認は不要だが、実施後に人間へ報告し、承認を得る（事後承認）。範囲を超える変更は事前承認を得る。詳細は [decision 0001](dev-docs/decisions/0001-readme-editing-policy.md) と `dev-docs/requirements-analysis.md` の D00。
- **`human-guide/` を読み込み・編集しない。** 人間オペレータ向けの手順書・技術選定や実装原則の根拠資料を置く場所であり、このプロジェクトの正典でも作業対象でもない。要件は `docs/`、作業文書は `dev-docs/` を参照する。存在する場合でもAIは開かない。`.claude/settings.json` の `permissions.deny` が `Read`/`Edit`/`Write` を機械的に塞ぐが、`Grep`/`Glob`/`Bash(cat ...)` 等の経路までは塞いでいない。そこは本ルールに従う。
- 技術スタック（[decision 0016](dev-docs/decisions/0016-tech-stack.md)）を再判断・再選定しない。
- `git push --force`、`git reset --hard`、hookのスキップ（`--no-verify` 等）を、ユーザーの明示的な指示なしに行わない。
- ユーザーから明示的な指示がない限り commit しない。

## dev-docs/ の参照ガイド

作業内容に応じて、着手前に該当ファイルを読む。

| 作業内容 | 参照先 |
| :--- | :--- |
| 要件・受け入れ基準の確認 | `dev-docs/requirements-analysis.md` |
| 評価ハーネスの制約確認 | `dev-docs/harness-invariants.md` |
| 未規定点の扱いの確認 | `dev-docs/decisions/README.md` とその一覧 |
| 実装前の基盤構築の順序・スキーマ方針 | `dev-docs/foundation-plan.md` |
| UI・フォーム・画面の作業 | `dev-docs/ui-ux-guidelines.md` |
| ブランチ運用・コミットの粒度 | `dev-docs/workflow.md` |
| セッションログの形式 | `dev-docs/session-log-format.md` |
| 過去のレビュー指摘の反映結果を知りたい | まず `dev-docs/session-logs/` と各成果物の変更履歴を見る。`dev-docs/reviews/` の原文そのものは、見送り済みの指摘を蒸し返さないため通常は読まない。 |
