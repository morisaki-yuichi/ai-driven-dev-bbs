# 0015 表示名の解決方式（JOIN か非正規化か）

- ステータス: 決定済
- 起票日: 2026-07-19
- 関連論点: D03
- 関連原典: AC04-2（表示名変更が過去の投稿にも反映される）、シナリオ05-2
- 影響範囲: `Bbs.Db.Thread` / `Bbs.Db.Comment` のフィールド、`Bbs.Query.displayNameOf`

## 背景

- **[事実]** AC04-2「表示名変更後、**過去の投稿（スレッド・コメント）の表示名も新しい名前に反映される**」。
- **[空白]** 実装方式。①投稿は `user_id` のみ持ち、表示時に解決する ②投稿に表示名を
  非正規化保存し、変更時にカスケード更新する。**どちらでも AC は満たせる。**
- **[解釈]** 原典は「変更時点のスナップショットを残す」とは一言も言っていない。

## 提案

**方式①（`user_id` のみ保持し、表示時に解決）。**

モデルでは `Thread` / `Comment` に `authorId : UserId` だけを持たせ、表示名は
`Query.displayNameOf` が `db.users` から引く。この構造の結果として:

- **AC04-2 は `updateDisplayName` の副作用ではなく、データ構造から自動的に従う。**
  カスケード更新の実装も、その漏れによるバグも存在しえない。
- `Invariant.displayName_propagates` は、方式①では自明に近い言明になる。方式②なら
  「カスケードが全投稿に行き渡ること」を実際に証明する必要がある。

## 決定（2026-07-19 ユーザー判断）

**方式①（`user_id` のみ保持し、表示時に JOIN で解決）。**

```sql
threads(id, author_id, title, body, created_at)
comments(id, thread_id, author_id, body, created_at, deleted)

SELECT t.*, u.display_name
FROM threads t JOIN users u ON u.id = t.author_id
```

投稿テーブルに `author_display_name` 列は置かない。

## 影響

### AC04-2 がデータ構造から自動的に従う

表示名の変更は `users` の 1 行を UPDATE するだけで、全投稿に即座に反映される。
**カスケード更新の実装も、その漏れによるバグも存在しえない。**

方式②なら decision 0002 のトランザクション内で `users` / `threads` / `comments` の
3テーブルを更新する必要があり、1つでも忘れると AC04-2 が壊れる。
シナリオ05-2 は一覧（スレッド）と詳細（コメント）の**両方**で新しい表示名を確認するため、
どちらの取りこぼしも検出される。

### 証明が軽くなる

`Invariant.displayName_propagates` は方式①では自明に近い言明になる。
方式②なら「カスケードが全投稿に行き渡ること」を実際に証明する必要があった。

### 退会機能を足すならここが決め直しになる

方式①では退会ユーザーの投稿の表示名が解決できない。モデルの `Query.displayNameOf` を
`Option String` にしてこの穴を明示してある。**原典に退会機能はない**ので実装上は起こらないが、
将来足すなら「表示名を `退会したユーザー` にフォールバック」等の判断が要る。

なお decision 0005（表示名は必ず 1 文字以上）により、**存在するユーザーの表示名が空になることはない**。
`none` が返るのは参照先ユーザーが存在しない場合だけで、それは `Invariant.Wf.threadAuthorsExist` /
`commentAuthorsExist` が排除する。

### 性能

一覧表示のたびに JOIN が要るが、評価規模では問題にならない。
decision 0010（最終更新日時の都度導出）と同じ判断基準。

### 形式モデル

`Db.Thread` / `Db.Comment` は `authorId` のみを保持し、`Query.displayNameOf` が解決する。変更不要。
