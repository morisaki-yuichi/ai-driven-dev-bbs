---
id: 0019
title: "D02: スキーマ詳細(主キー型・URLのID形式・インデックス)"
date: 2026-07-19
importance: major
decided_by: ai
status: 提案
---

# 0019 D02: スキーマ詳細(主キー型・URLのID形式・インデックス)

- 関連論点: D02（`dev-docs/requirements-analysis.md` §3）
- 関連原典: C-02〜C-18、AC 全般。原典は主キーの型・URLのID形式・インデックスについて一言も述べていない。
- 影響範囲: `app/migrations/0001_init.sql`、`domain/model.rs`（`UserId`/`ThreadId`/`CommentId`型）、URLルーティング（`/threads/:id`等のパス形式）。

## 背景（原典が何を言い、何を言っていないか）

- **[事実]** 永続化必須（H-03）。方式は自由。
- **[事実]** decision 0009 により、タイムスタンプは UTC 保存・ミリ秒精度と既に確定している（`timestamptz(3)` が示唆される、decision 0016 §7.2）。
- **[事実]** decision 0016 §7.2 は「D02（スキーマ詳細）: 主キーの型、タイムスタンプ型の具体、インデックス。フェーズ2で別decisionとして起票する」と明記しており、本決定がそれに対応する。
- **[事実]** `formal/Bbs/Db.lean` は `UserId`/`ThreadId`/`CommentId`/`SessionId` を `abbrev := Nat`（連番の自然数）としてモデル化し、`Db`構造体は `nextUserId`等のカウンタを持つ。ただし `Session` の `id` については、`foundation-plan.md` §1.6 が「セッションIDはCSPRNG生成のランダム値」と実装上の要件を既に確定しており、Lean側の`Nat`表現は簡略化のための抽象である。
- **[空白]** 主キーの型（連番 / UUID）。
- **[空白]** URLに現れる`:id`の形式（要件分析D02が指摘: 連番だとシナリオ02-2-4「`/スレッドID/delete`への直接アクセス」が容易になる一方、IDの推測可能性が生じる。**ただし要件分析自身が「本教材では問題にならない想定」と注記済み**）。
- **[空白]** インデックス設計。

## 選択肢

**主キーの型**

1. **連番（`bigint generated always as identity`）**: 挿入順と一致する単調増加の値。人間にも読みやすい。
2. **UUID（v4等のランダム値）**: 推測不可能だが、`uuid`クレート依存が増え、URLの可読性が落ちる。

**URLのID形式**

1. 主キーをそのままパスパラメータに使う（例: `/threads/42`）。
2. 別途公開用トークンを発行し、内部IDと分離する。

## 提案（および理由）

**主キー: 連番（`bigint generated always as identity`）を採用する。**

- decision 0009 は「全ソートキーに第2キー`id`」を要求しており、これは**`id`の大小が挿入順（=作成時刻順）と一致する**ことを暗黙に前提にしたタイブレークである。連番はこの前提を構造的に満たすが、UUID v4はランダムなため`id`順が作成順と無関係になり、0009の意図（同時刻衝突時の決定的な順序付け）を壊す。UUID v7（時刻順）を使えば回避できるが、それを導入する積極的な理由がない。
- 要件分析D02自身が「IDの推測可能性は本教材では問題にならない想定」と注記済みであり、UUIDを選ぶ動機（推測不可能性）が原典上の要求として存在しない。
- `sessions.id`だけは例外とし、`text`型のCSPRNGランダムトークンを主キーにする（`foundation-plan.md` §1.6で既に確定済み。認証トークンは推測不可能性が本質的に必要なため、他の主キーと性質が異なる）。

**URLのID形式: 主キー（連番）をそのままパスパラメータに使う。**

- 選択肢2（公開用トークンの分離）は、原典が要求していない抽象化を追加するだけで、要件分析が「問題にならない想定」と既に注記した懸念への対処に見合わない。

**インデックス: 構造的な制約（`PRIMARY KEY`/`UNIQUE`/`FOREIGN KEY`）から自動的に得られるものに加え、`comments(thread_id)`にのみ明示的な索引を追加する。**

- `users.unique_id`の`UNIQUE`制約（C-04）は自動的に索引を伴う。
- `comments.thread_id`はスレッド詳細ページでの「あるスレッドの全コメント取得」に頻出するが、PostgreSQLは外部キーの参照元列に自動で索引を作らないため明示する。
- 検索（decision 0011: `LIKE '%kw%'`）用の索引は追加しない。decision 0016 §6.3が「全文検索インデックスは不要」と既に結論しており、先頭ワイルドカードのLIKEはbtree索引を活用できないため、`pg_trgm`等の追加は本件の規模（教材アプリ）に対して過剰な複雑化になる。
- 文字数・文字種などdecision 0003〜0006のバリデーションルールは、SQLの`CHECK`制約として重複させない。`domain/validation.rs`の単体テストで担保し、1リクエスト=1トランザクション（decision 0002）の中でSQL到達前に検証する。

## 決定（2026-07-19 AI判断・未承認）

上記の提案どおり、**連番主キー（`bigint identity`、`sessions`のみ`text`のCSPRNGトークン）・URLはそのまま主キーを使う・索引は`comments(thread_id)`のみ追加**で実装を進める。**この決定はAI単独判断であり、人間の承認を経ていない。**

## 影響

- `app/migrations/0001_init.sql`: `users`/`threads`/`comments`の主キーは`bigint generated always as identity`。`sessions.id`は`text primary key`。
- `domain/model.rs`: `UserId`/`ThreadId`/`CommentId`はRust側で`i64`相当の新型（型エイリアスまたはnewtype）として定義し、`SessionId`は`String`。
- URLルーティング: `/threads/:id`のような形式で連番IDをそのまま使う。パスから直接推測・アクセスされても、認可チェック（作成者本人か等）はサーバ側で別途行う（C-06/AC06-3等、URLの不透明性に頼らない）。

## 変更履歴

（新規作成のため、なし）
