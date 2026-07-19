# 基盤構築計画（フェーズ1）

- 作成日: 2026-07-19
- 位置づけ: [decision 0016](decisions/0016-tech-stack.md) で確定した技術スタックを、実装着手前に用意すべき基盤へ落とし込んだ計画。
- 前提: `dev-docs/requirements-analysis.md`（要件の正典）、`dev-docs/harness-invariants.md`（H-01〜H-13）、`dev-docs/decisions/0001`〜`0016`。
- **本計画は実装ではない。** ここに書いたファイルはまだ1つも作成していない。

---

## 1. 確定スタック

各レイヤの「採用・理由・却下した代替案」を記す。理由の末尾に、判断根拠となった選定軸を付す。軸の定義は [decision 0016](decisions/0016-tech-stack.md) §2 を参照。

> **ラベルの凡例（混同注意）**: **`A`〜`G`** は Tier 1・Tier 2 の評価軸、**`T0-1`・`T0-2`** は Tier 0 のゲートを指す。**両者は別系統であり、`T0-n` は Tier 2 の軸 `G`（再現性のあるビルド）の細目ではない。**

### 1.1 言語: Rust

- **採用理由**: `Option` / `Result` の網羅マッチが強制されるため、本アプリの主要エラー面 E2（null / 未処理ケース）と E5（ドメイン不変条件）の検出が**規律依存でなく強制**になる（**A**）。加えて `formal/Bbs/` の Lean モデルが `Except Error` ＋ エラー ADT で書かれており、`Result` ＋ `enum` へほぼ 1 対 1 で移せる。モデルと実装の乖離がコンパイル時に現れる箇所が増える（**A**）／教材としての固有価値（**E**）。
- **却下**: TypeScript（A の E2/E5 が `never` チェック等の規律依存。ただし **B は案T が明確に優位**であり、Tier 1 のみの採点では案T が推奨だった）／Go（ゼロ値により「詰め忘れ」が `""` として静かに通る＝E2 に構造的な穴）／Python（Jinja も生 SQL も非検査で、支配的な E3・E4 を一つも shift-left できない）／C#（A は十分だが C・D で劣後）。
- **正直な弱点**: **B が3候補中最下位**。1ファイル変更で数十秒バンドの再コンパイル、`sqlx prepare` 儀式、借用検査を通すだけの反復が乗る。このうち借用検査の反復は、本アプリに共有可変状態が存在しない以上**捕まえたバグを伴わない純粋な摩擦**である（[0016](decisions/0016-tech-stack.md) §4・§6.2）。§6 の緩和策で桁を下げにいくが、TypeScript の 1 秒未満バンドには届かない。

### 1.2 Web フレームワーク: Axum

- **採用理由**: SSR / MPA を素直に組める（**T0-1**）。`tower` ミドルウェアで認証ガード（C-09）と `Cache-Control: no-store`（C-11）を横断的に掛けられる。エクストラクタが型で表現されるためハンドラ引数の取り違えがコンパイル時に落ちる（**A**）。Rust 系 Web FW の中でドキュメント密度が最も高い（**C**）。
- **却下**: Actix Web（本件規模で性能差が現金化されない＝**D** の差が実質ゼロ。**C** で Axum が優位）／Rocket（nightly 依存の歴史があり再現性を損なう＝**G**）。

### 1.3 テンプレート / 描画: Askama（SSR）＋ 最小限のプレーン CSS

- **採用理由**: テンプレートを**コンパイル時に型検査**する。ビューに渡し忘れたフィールド、存在しないフィールドの参照がビルドエラーになる（**A**、エラー面 E4）。HTML が独立ファイルとして読めるため教材としての可読性が高い（**E**）。
- **却下**: **Tera（ランタイム評価のため E4 の検出が ◎→× に落ちる。本スタックを選んだ A の論拠を無効化するため採用不可）**／maud（HTML が Rust コード内に埋まり **E** で劣る。ただし B と C ではこちらが優位であり、Askama のバージョン問題が実装初回に深刻化した場合の第一代替とする）／Tailwind（ビルド段と儀式を追加し **B** を悪化させる。H-02 は素の DOM でも可としている）。
- **リスク**: Askama は rinja のフォークと統合を経て API に変遷がある。**本スタックで C（生成信頼性・幻覚リスク）が最も高い箇所**。バージョンをピン留めし、記憶ではなく公式ドキュメントを参照して書く。

### 1.4 データ層: sqlx（`query_as!` マクロ ＋ オフラインキャッシュ）

- **採用理由**: `query_as!` は**実 DB に接続して SQL を検証する**ため、列名 typo・型不一致・マイグレーションとクエリのドリフトがビルド時に落ちる（**A**、エラー面 E3）。ORM の抽象を挟まないため、生成される SQL が読めて学習価値がある（**E**）。`#[sqlx::test]` がテストごとに使い捨て DB を用意する（**F**）。
- **却下**: Diesel（DSL の学習コストと **C** の薄さ）／SeaORM（生成物が厚く **B** を悪化）／生 tokio-postgres（E3 が非検査＝**A** の主要な取り分を失う）。
- **制約**: `.sqlx/` オフラインキャッシュのコミットが必須（§4.2）。

### 1.5 DB: PostgreSQL 17

- **採用理由**: [decision 0011](decisions/0011-search-matching-and-normalization.md) の「`LIKE` は大文字小文字を区別する」が**既定のまま満たされる唯一の候補**（**B**＝追加設定という儀式が要らない。副次的に **A**＝設定漏れによる静かな仕様違反の経路自体が存在しない。[0016](decisions/0016-tech-stack.md) §3 の「Tier 1 の減点に転換した基準」を参照）。[0002](decisions/0002-action-failure-semantics.md) のリクエスト単位トランザクションと [0007](decisions/0007-session-multiplicity.md) のセッション永続化を1つの DB に収められる（**T0-2**）。実運用に近い構成で学習価値がある（**E**）。
- **却下**: SQLite（`LIKE` が ASCII 範囲で case-insensitive のため `PRAGMA case_sensitive_like=ON` が必要＝**決定済み仕様を DB の既定が静かに破る**。compose が1サービスで済み **B・F・D** の他の項目では優位だったが、この **B・A** の減点と **E** を優先した）／MySQL・MariaDB（既定照合順序が ci のため `COLLATE utf8mb4_bin` の明示が必要）。

### 1.6 認証 / セッション: `sessions` テーブル ＋ HttpOnly Cookie

- **採用理由**: [decision 0007](decisions/0007-session-multiplicity.md) が「多重セッションを許す」「DB に永続化する」「Redis は使わない」を確定済み。サーバ側セッションテーブルが最も素直。
- **構成**: セッション ID は CSPRNG 生成のランダム値。Cookie 属性は `HttpOnly` / `SameSite=Lax` / `Path=/`（`Secure` は HTTP 評価環境のため付けない）。有効期限は設けない（0007）。
- **パスワードハッシュ**: `argon2` crate（RustCrypto、純 Rust。ネイティブビルド依存がなく Docker ビルドが重くならない＝**D**）。
- **却下**: JWT（[0007](decisions/0007-session-multiplicity.md) のログアウト時失効と噛み合わない）／Redis（0007 が明示的に却下。コンテナが増え H-06 の手順が複雑化）。

### 1.7 実行環境: Docker Compose（app ＋ db）

- **採用理由**: H-04（ミドルウェアはローカル前提にせず Docker 起動）を満たす。`docker compose up` の1コマンドで起動が完結し H-06 に有利。
- **開発時**: AI はホストの `cargo` で反復し、DB のみコンテナで動かす（**B** の緩和）。評価者は compose で app 込みで起動する。**両経路が同じマイグレーションを使う**ようにする（§4.1）。
- **前提ツール**: git / Docker / OS / Rust のパッケージマネージャ（cargo）＝ H-05 の4点に収まる。

### 1.8 テスト: `cargo test` ＋ `#[sqlx::test]`

- **採用理由**: ドメイン層は DB 非依存の純関数として書くためミリ秒で回る（**F**）。DB を要する層は `#[sqlx::test]` がテストごとに使い捨て DB を作るので隔離が自動（**F**）。
- **却下**: 手動の DB 準備・共有 DB でのテスト（隔離が壊れ **F** を損なう）。

### 1.9 再現性（G）

`Cargo.lock` と `rust-toolchain.toml` をコミットし、ツールチェーンとクレート版を固定する。`.sqlx/` のコミットにより、ビルド時に DB もネットワーク上の DB も要らない。

---

## 2. ディレクトリ構成

```
/
├── README.md                    # 既存。SETUP.md へのリンクを1行だけ追記（P-08）
├── SETUP.md                     # ★新規。セットアップ・起動・リセット手順の本体（docs/ の外）
├── docs/                        # 原典。読み取り専用（AI も編集しない）
├── dev-docs/                    # AI 成果物（本ファイル・decisions・session-logs）
├── formal/                      # Lean 4 形式モデル（既存）
├── compose.yaml                 # ★db + app
├── .env.example                 # ★接続情報のひな形（秘匿キーは無い想定 / H-07）
└── app/
    ├── Cargo.toml / Cargo.lock
    ├── rust-toolchain.toml      # ツールチェーン固定（G）
    ├── Dockerfile               # マルチステージ + cargo-chef
    ├── .sqlx/                   # ★オフラインクエリキャッシュ（コミット必須）
    ├── migrations/
    │   └── 0001_init.sql
    ├── templates/               # Askama テンプレート（.html）
    │   ├── layout.html          # ヘッダー/フッターの固定文言（C-01）
    │   ├── login.html  register.html
    │   ├── thread_list.html  thread_detail.html  thread_new.html
    │   ├── profile_edit.html
    │   └── error.html           # 404 相当（C-10）
    ├── static/
    │   └── app.css              # 最小限のプレーン CSS
    ├── src/
    │   ├── main.rs              # 起動・マイグレーション適用・ルータ組み立て
    │   ├── config.rs
    │   ├── error.rs             # AppError → HTTP ステータス写像（C-10 を1箇所に集約）
    │   ├── domain/              # ★DB 非依存の純粋層。formal/Bbs/ の対応先
    │   │   ├── model.rs         # ← Bbs/Basic.lean
    │   │   ├── validation.rs    # ← Bbs/Validation.lean（0003〜0006）
    │   │   └── query.rs         # ← Bbs/Query.lean（paginate / search / sort: 0009, 0011, 0012, 0013）
    │   ├── db/                  # sqlx リポジトリ層（query_as! はここに閉じる）
    │   │   ├── users.rs  sessions.rs  threads.rs  comments.rs
    │   ├── web/
    │   │   ├── middleware.rs    # 認証ガード（C-09）+ Cache-Control: no-store（C-11）
    │   │   ├── params.rs        # q / sort / page の共通パース（0011, 0013）
    │   │   ├── auth.rs  threads.rs  comments.rs  profile.rs
    │   └── views.rs             # Askama テンプレート構造体
    └── tests/                   # 統合テスト（#[sqlx::test]）
```

**層の意図**: `domain/` を DB からもフレームワークからも独立させることが、[decision 0016](decisions/0016-tech-stack.md) §6.2 の「高い B を A で取り返す」論証の前提になる。Lean モデルの述語をここに 1 対 1 で置き、`#[sqlx::test]` を通さずミリ秒で回せる状態を維持する。**`query_as!` は `db/` の外に漏らさない。**

---

## 3. データ永続化・スキーマ方針

主キーの型やインデックスの詳細は **D02 として未決定**であり、フェーズ2 で別 decision として起票する。ここでは決定済みの制約から導かれる骨格のみを記す。

| テーブル | 要点 | 根拠 |
| :--- | :--- | :--- |
| `users` | `unique_id` に UNIQUE 制約（C-04）。`display_name` は重複可（C-03）。`password_hash` を保存（平文不可） | C-02〜C-04 |
| `sessions` | セッション ID を主キー、`user_id` を外部キー。**同一ユーザーの複数行を許す**（多重セッション）。有効期限カラムは持たない | [0007](decisions/0007-session-multiplicity.md) |
| `threads` | タイトル・本文は作成後不変（C-05、UPDATE 経路を作らない）。**物理削除**するため `deleted_at` を持たない | C-05、[0014](decisions/0014-thread-deletion-is-physical.md) |
| `comments` | **論理削除**。`deleted_at`（NULL 可）を持ち、本文は保持したまま表示のみ固定文言に置換する。`thread_id` に外部キー | C-07、[0012](decisions/0012-search-scope.md) |

**決定済み事項から導かれる実装制約**

- **最終更新日時（C-15）はスレッドに非正規化カラムを持たず、コメントから導出する**（[0010](decisions/0010-last-updated-derivation.md)）。一覧クエリで集約する。
- **コメント数は削除済みを含む**（C-16、[0010](decisions/0010-last-updated-derivation.md)）。表示・ソート・スレッド削除可否（C-06）のすべてで同じ定義を使う。
- **表示名は JOIN で解決する**（[0015](decisions/0015-display-name-resolution.md)）。投稿側に表示名を複製しない。AC04-2 が自動的に満たされる。
- **タイムスタンプは UTC 保存・ミリ秒精度**（[0009](decisions/0009-time-granularity-and-sort-tiebreak.md)）→ `timestamptz(3)` を想定。表示は JST。ソートの第2キーは id。
- **検索は `LIKE '%kw%'` 相当**、正規化なし、削除済みコメントは**元本文ごと**対象外（[0011](decisions/0011-search-matching-and-normalization.md)、[0012](decisions/0012-search-scope.md)）。タイトルは対象に含めない。全文検索インデックスは不要。
- **1リクエスト＝1トランザクション**（[0002](decisions/0002-action-failure-semantics.md)）。ハンドラの入口でトランザクションを開始し、`Err` を返す経路では必ずロールバックする。この規律をミドルウェアまたはヘルパで1箇所に固める。

---

## 4. DB 初期化・マイグレーション方針

**満たすべき条件**: 空 DB から `docker compose up` だけでシナリオ 01→05 を逐次実行できる（H-06 / H-08〜H-11 / H-13）。シードデータは投入しない（H-11）。

### 4.1 マイグレーションの適用経路

- `app/migrations/*.sql` を **`sqlx::migrate!` でバイナリに埋め込み、アプリ起動時に自動適用**する。
- これにより **compose 経路（評価者）とホスト `cargo run` 経路（開発中の AI）が同一のマイグレーションを使う**ことが構造的に保証される。評価者が別途マイグレーションコマンドを打つ手順は発生しない（H-07: 対話的な手作業を要求しない）。
- app コンテナは db コンテナの healthcheck 完了を待って起動する（`depends_on: condition: service_healthy`）。

### 4.2 `.sqlx/` オフラインキャッシュ

- `sqlx::query_as!` はコンパイル時に DB 接続を要求するが、**評価者は clone 直後（H-13）にイメージをビルドする**。ビルド時に DB が居ることを前提にできない。
- したがって **`SQLX_OFFLINE=true` ＋ コミット済み `.sqlx/`** を前提とする。
- **規律**: SQL を変更したら `cargo sqlx prepare` を実行して `.sqlx/` をコミットする。忘れると Docker ビルドが壊れるため、**手順を SETUP.md ではなく開発側ドキュメントに明記し、実装フェーズの各セッションで確認する**。

### 4.3 空 DB へのリセット（D14）

- `docker compose down -v`（名前付きボリュームごと削除）→ `docker compose up` で空 DB から再構築される。
- この手順を **SETUP.md に「評価をやり直す場合」として明記**する。評価者が再実行するときに必要になる（H-08）。

### 4.4 README への追記（P-08 の範囲内）

- `README.md` に **`SETUP.md` へのリンクを1行追加するのみ**。手順の本文は書かない。既存記述の削除・書き換え・並べ替えは行わない。
- この範囲であれば事前承認は不要（[decision 0001](decisions/0001-readme-editing-policy.md) / P-08）だが、**実施後に報告する**。

---

## 5. 実装前に作る基盤の一覧

上から順に着手する。各項目に「何を満たすためのものか」を付す。

| # | 基盤 | 目的 |
| :--- | :--- | :--- |
| 1 | `rust-toolchain.toml` / `Cargo.toml` / `Cargo.lock` | ツールチェーンと依存の固定（**G**）。Askama のバージョンをここで実測・ピン留め |
| 2 | `compose.yaml`（db + app）＋ `.env.example` | H-04 / H-06。db に healthcheck を付ける |
| 3 | `Dockerfile`（マルチステージ ＋ cargo-chef） | H-06。依存レイヤをキャッシュしてコールドビルドの分単位を緩和（**B**） |
| 4 | `migrations/0001_init.sql` ＋ `sqlx::migrate!` 埋め込み | §4.1。H-08 / H-11 |
| 5 | `.sqlx/` 生成と `SQLX_OFFLINE` のビルド設定 | §4.2。これが無いと評価者のビルドが失敗する |
| 6 | `error.rs`（`AppError` → HTTP 写像） | C-10 の「404 の一律適用」を1箇所に集約。網羅マッチで分岐漏れをビルド時に検出（**A**） |
| 7 | `web/middleware.rs`（認証ガード ＋ `Cache-Control: no-store`） | C-09 / C-11（AC03-2）。[0008](decisions/0008-browser-back-is-out-of-model.md) |
| 8 | `db/sessions.rs` ＋ Cookie ヘルパ ＋ argon2 | [0007](decisions/0007-session-multiplicity.md)。多重セッション・DB 永続化 |
| 9 | `templates/layout.html` | C-01 の固定文言（`AI掲示板（仮）` / `© 2026 AI駆動開発教材プロジェクト` / `[表示名] さん`）を1箇所に固定 |
| 10 | `web/params.rs`（`q` / `sort` / `page` の共通パース） | [0011](decisions/0011-search-matching-and-normalization.md) §影響（全パラメータをクエリ文字列で保持）、C-13、[0013](decisions/0013-pagination-edge-cases.md)（不正値を1ページ目に丸める） |
| 11 | `domain/` の骨格（`validation.rs` / `query.rs`）とその単体テスト | Lean モデルの対応先。**F** の高速テスト基盤。**A** の取り返し論証の前提 |
| 12 | `tests/` の `#[sqlx::test]` ひな形 | **F**。テストごとの使い捨て DB |
| 13 | `SETUP.md` ＋ `README.md` へのリンク追記 | H-06 / P-08 / §4.3・§4.4 |
| 14 | **B 緩和策**: `mold` または `lld` リンカ指定、`[profile.dev] debug=1`、`cargo-watch`（または `bacon`）、`cargo check` を既定の高速パスにする。**リンカ指定は評価者経路を壊さない形にすること**（§6 の注記） | §6 |

**着手順の意図**: 1〜5 で「評価者が clone → `docker compose up` で起動できる」骨格を先に通す。**H-06 の充足が最も後戻りコストが高い**ため、機能実装より前に空アプリで一度通しておく。6〜10 が横断的制約（404・認証・固定文言・クエリパラメータ）を1箇所に固める層で、ここを先に作らないと各機能に散らばって C-09 / C-10 / C-13 の漏れが生じる。11〜12 でテストの背骨を通してから機能実装へ入る。

---

## 6. B（反復摩擦）の緩和策と実測計画

[decision 0016](decisions/0016-tech-stack.md) §5.2 で予測した所要時間バンドを下げにいく。**退避条項ではなく、選んだコストを引き受けたうえでの緩和である。**

| 施策 | 狙い |
| :--- | :--- |
| `mold` / `lld` をリンカに指定 | (a) 1ファイル変更→再コンパイルはリンク段が支配的。ここが最も効く。**ただし §6.1 の制約を守ること** |
| `[profile.dev] debug = 1`（フル debug info を切る） | 同上。リンク時間とバイナリサイズを削る |
| `cargo check` を既定の高速パスにする | (c) 型検査だけなら `cargo build` を待たない |
| `cargo-watch` / `bacon` | 保存時に自動で `check` を回す |
| Docker の `cargo-chef` | コールドビルドの分単位を、依存が変わらない限り回避 |
| 単一クレート構成を維持（ワークスペース分割をしない） | この規模ではクレート分割の並列化より、分割によるリンク回数増のほうが効く見込み |

### 6.1 B 緩和策を評価者経路に漏らさない（H-06 / H-13 の保護）

**上記はすべて「開発側（ホストの `cargo`）を速くする」ための施策であり、評価者経路（コンテナ内 `cargo build`）を壊してはならない。**

特に **`mold` / `lld` のリンカ指定を `.cargo/config.toml` にコミットすると、その設定はコンテナ内のビルドにも効く。** Dockerfile のビルドステージに mold / lld が入っていなければ、**clone 直後のビルドが失敗し H-06（README 起点でセットアップが完結）と H-13（clone 直後から検証が始まる）を破る。** 本計画は H-06 を「最も後戻りコストが高い」と位置づけている（§5）以上、ここは実装時の気づきに頼らず制約として明記する。

**満たすべき制約（どちらかを選ぶ）**

1. リンカ指定を**ホスト側限定**にする（コミットしない、あるいは `CARGO_BUILD_RUSTFLAGS` 等の環境変数でホストにのみ与える）。この場合、設定が開発者間で共有されない点は許容する。
2. リンカ指定をコミットするなら、**Dockerfile のビルドステージに同じリンカを同梱し、`docker compose build` を実際に通して検証する**。

いずれを選んでも、**§5 の #1〜#5 を通す時点で「クリーンな clone から `docker compose up` が通る」ことを1回実測する**（キャッシュを使わない確認を含む）。`.sqlx/` の欠落（§4.2）も同じ経路でしか露見しないため、まとめて確認する。

### 実測計画

**実装フェーズの初回セッションで、[0016](decisions/0016-tech-stack.md) §5.2 の (a)〜(d) を実測し、予測バンドと突き合わせて decision 0016 に変更履歴として追記する。** 予測が外れた場合、それ自体が「AI による技術選定の見積もり精度」という本教材の観測対象になる。

あわせて、DB 選定直後に **[0011](decisions/0011-search-matching-and-normalization.md) の `LIKE` 大文字小文字と [0003](decisions/0003-character-counting-and-charset.md) のユニーク ID の扱いを PostgreSQL 上で1回まとめて実測確認する**（両 decision が実測を指示している）。

---

## 7. 本計画で確定していないこと

| 項目 | 論点 | 扱い |
| :--- | :--- | :--- |
| スキーマ詳細（主キー型・インデックス） | D02 | フェーズ2 で decision 起票 |
| CSRF 対策の要否 | D05 | フェーズ2。破壊的操作は POST にする方針のみ先に固める |
| POST-Redirect-GET の採否 | D06 | フェーズ2 |
| P06 のパス（`/profile/edit` か `/edit_profile` か） | D10 | フェーズ2。影響は小さい |
| 削除確認ダイアログの有無 | D18 | フェーズ2。`window.confirm` は H-02（agent-browser 操作性）に影響しうる |
| スクロール連携の実現方式 | D19 | フェーズ2。[0013](decisions/0013-pagination-edge-cases.md) によりコメントは全件表示なのでアンカー方式が成立する |
| ログイン後の復帰先 | D20 | 常に P03 で足りる（要件分析 D20）。実装時に確定 |
