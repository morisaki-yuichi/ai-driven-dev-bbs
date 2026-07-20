---
id: 0029
title: 形式モデルの論理時計(tick)と実装のcreated_at(now())の乖離を許容する
date: 2026-07-20
importance: minor
decided_by: ai+user
status: 決定済
---

# 0029 形式モデルの論理時計(tick)と実装のcreated_at(now())の乖離を許容する

- 関連論点: なし（新規。F07コメント作成のレビュー中に判明）。
- 関連原典: なし（原典は時刻の実装粒度に言及していない）。decision 0009（時刻の粒度・同時刻の衝突・ソートのタイブレーク）、decision 0027（`comment_bumps_lastUpdated`が論理時計の単調性を要求する）と関連する。
- 影響範囲: `formal/Bbs/Db.lean`（`tick`）と `app/`（`created_at` カラム）の対応関係の理解。実装コード・スキーマの変更は伴わない。

## 背景（原典が何を言い、何を言っていないか）

**[事実]** 形式モデルの `formal/Bbs/Db.lean` の `tick` は `clock := clock + 1` として新しい値を返す論理時計であり、呼び出しごとに厳密に増加する。同じ `Db` から2回 `tick` を呼べば、必ず異なる値が返る。decision 0009 のソート順（`comments`は`created_at asc, id asc`、`threads`は`created_at desc, id desc`）の議論は、この「時刻が相異なる」という前提に暗黙に依拠している。decision 0027 の `comment_bumps_lastUpdated` も、論理時計の単調性を仮定としてモデルに明示した。

**[事実]** 一方、実装側の `comments.created_at` / `threads.created_at` は PostgreSQL の `default now()` によって決まる。`now()` は**同一トランザクション内では値が変わらない**（トランザクション開始時刻を返す）。レビューで実測したところ、1トランザクション内で連続して2件 INSERT した場合、両方の `created_at` が同一のミリ秒値になり得ることを確認した。

**[空白]** 原典は時刻の実装粒度・トランザクション境界と時刻採番の関係について何も述べていない。「モデルの論理時計と実装のウォールクロックをどう対応させるか」は実装固有の論点であり、原典の守備範囲外。

## 選択肢

### (a) 実装側でも厳密に相異なる時刻を保証する（例: 採番カウンタを別途持つ、`clock_timestamp()`をレコードごとに呼ぶ等）

- 利点: 形式モデルの前提（`tick`の厳密単調性）と実装が完全に一致し、`comment_bumps_lastUpdated`のような不変条件がそのまま実装の性質として読める。
- 欠点: 現状「コメント1件＝1トランザクション」という制約下では実害が無い変化を先取りして実装を複雑化させる。過剰実装。

### (b) 乖離を許容し、決定として明文化する（今回採る）

- 利点: 差分ゼロ。現状の実装（`default now()`）のまま進められる。乖離の理由と実害の範囲を記録に残すことで、将来この前提が崩れる条件（後述）が来たときに気づける。
- 欠点: モデルと実装の対応が厳密には1対1でなくなる。ドキュメント上の負債として残る。

### (c) 乖離を放置し記録もしない

- 利点: 差分ゼロ。
- 欠点: **採らない。** レビューで実測確認された既知のギャップを記録しないと、後続セッションが「モデルの不変条件は実装でも成り立つはず」と誤って前提に使う可能性がある。

## 提案（および理由）

**(b) を採る。** 実害が限定的である理由:

- `comments` テーブルへのSELECTは常に `created_at asc, id asc`、`threads` テーブルへのSELECTは常に `created_at desc, id desc` と、**いずれも `id` をタイブレーカに持つ**（decision 0009）。したがって `created_at` が同一値になっても、`id`（採番順、厳密に単調）が最終的な順序を決定するため、**表示順は決定的**であり揺れない。

  > **[2026-07-20 追記・F12実装後の実態]** この段落の `threads` に関する記述は、起票時点（F07レビュー）の実装を述べたもので**現在は正しくない**。F12（ソート切替）以降、スレッド一覧の表示順を決めているのはSQLの `order by` ではなく Rust 側の純粋関数 `domain::query::sort_thread_fields`（`formal/Bbs/Query.lean` の `sortThreads` の対応先）であり、`db::threads::search` の `order by` は再整列前の初期順序を与えるだけになった。**結論（表示順は決定的）は変わらない** ―― `sort_thread_fields` は4つのソートキーすべてで「主キー → `id` 昇順」の辞書式であり、`id` をタイブレーカに持つという本decisionの実害限定の根拠はそのまま保たれる。詳細は下の「影響」を参照。
- 現状は「コメント1件＝1トランザクション」（F07の実装、`app/src/web/thread_detail.rs`の`create_comment`）であり、1トランザクション内で複数の `comments`/`threads` レコードを同時に作る経路が存在しない。したがって `comment_bumps_lastUpdated` が要求する「新しいコメントの時刻は既存レコードの時刻を厳密に上回る」という性質も、実質的には成立する（同一トランザクション内での衝突が起こり得るのは、複数レコードを同時に作る場合のみ）。

(a) は今のところ払うコストに見合う実害が無いため、過剰実装として見送る。

## 決定（2026-07-20 ユーザー判断）

(b) を採る。形式モデルの `tick`（厳密単調な論理時計）と実装の `created_at`（`default now()`、1トランザクション内では同一値になり得る）の乖離を許容する。ユーザーが承認した。

## 影響

- 実装（`app/`）への変更は無い。`comments.created_at` / `threads.created_at` は引き続き `default now()` のまま。
- 将来この乖離が問題になりうる条件: **1トランザクションで複数の `comments` または `threads` レコードを作る機能**が入った場合。例えば「スレッド作成と同時に複数コメントを一括投入する」「バッチインポート」のような機能ができると、同一トランザクション内で `created_at` が衝突する行が複数発生し、`id` のタイブレークだけに順序保証を依存する現状の前提（decision 0009）自体は変わらず有効だが、`comment_bumps_lastUpdated` のような「時刻が厳密に進む」ことを前提にした不変条件の解釈には注意が要る。そのような機能を実装するセッションでは、本decisionを参照し、(a)（実装側での厳密な時刻分離、またはモデル側の要求緩和）を再検討すること。
- `formal/Bbs/Db.lean` の `tick` および `formal/Bbs/Invariant.lean` の `comment_bumps_lastUpdated`（decision 0027）は、モデル内では引き続き厳密単調性を前提とする。この決定はモデルを変更するものではなく、「モデルと実装の対応が厳密な1対1ではない」ことを明文化するもの。
- 他のdecisionとの関係: decision 0009（ソート順のタイブレーク）が本decisionの実害限定の根拠。decision 0027（`comment_bumps_lastUpdated`の単調性仮定）が本decisionの発端。
- **スレッド一覧の表示順を決める場所（F12以降）**: `app/src/web/thread_list.rs` が `db::threads::search` の結果を `domain::query::ThreadSortFields` へ写し、`domain::query::sort_thread_fields` で整列してから描画する。SQLの `order by` は再整列前の初期順序にすぎない。本decisionが述べる「`created_at` が同値になりうる」性質が実際に効くのは**この Rust 側の比較関数**であり、SQL側ではない。
- **本decisionが到達可能だと示した経路が、実際にバグを1件顕在化させた**（F12レビューで検出・修正）。`db::threads::search` の `order by` は `threads.created_at desc, threads.id desc` だったが、Rust側 `sort_thread_fields` とLeanの `leOf` のタイブレークは `id` **昇順**であり、両者が逆を向いていた。本decisionが「1トランザクション内で `now()` は同値になりうる」と明記している以上、`created_at` 同値でSQLとRustの順序が食い違う状況は到達可能である。SQL側を `threads.id asc` に揃えて解消した。
- **時刻の精度に関する補足**（F12レビューで明文化）: Rust側の比較は `web::format::to_millis` でミリ秒に丸めた値を使うため、`created_at` が**1ミリ秒未満しか違わない**2件も同値に潰れ、`id` 昇順のタイブレークに落ちる。列は `timestamptz`（マイクロ秒精度）なので精度低下は実際に起きているが、失われる精度は decision 0009 の要求粒度（ミリ秒）を下回るため許容する。この経路でも順序は決定的。

## 変更履歴

- 2026-07-20（F12ソート機能のレビュー対応）: 「提案」節の実害限定の根拠のうち、`threads` の表示順を **SQLの `order by created_at desc, id desc` が決めている**とする記述が、F12実装後の実態と乖離したため追記で訂正した（表示順を決めるのは Rust の `domain::query::sort_thread_fields`）。結論（`id` をタイブレーカに持つので表示順は決定的）は変わらないため、決定そのものは覆っていない。あわせて「影響」節に、本decisionが到達可能と示した経路が実際に SQL と Rust のタイブレーク方向の不一致として顕在化し修正されたこと、およびミリ秒丸めによる同値化の扱いを追記した。
