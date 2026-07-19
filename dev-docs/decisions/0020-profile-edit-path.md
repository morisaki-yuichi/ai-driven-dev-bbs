---
id: 0020
title: "D10: ユーザー編集画面(P06)のパス"
date: 2026-07-19
importance: minor
decided_by: ai+user
status: 決定済
---

# 0020 D10: ユーザー編集画面(P06)のパス

- 関連論点: D10（`dev-docs/requirements-analysis.md` §3、§1.3 の「原典間の不整合」注記）
- 関連原典: `docs/product/designs/ui_design.md`（画面一覧）、`docs/product/designs/common_layout.md`（共通バリデーション・挙動ルール）
- 影響範囲: `app/templates/layout.html`（ヘッダーのプロフィール編集リンク）、以降のP06ルーティング実装。

## 背景（原典が何を言い、何を言っていないか）

- **[事実]** `docs/product/designs/ui_design.md` §1「画面一覧」は P06 のパスを `/profile/edit` と明記している。
- **[事実]** `docs/product/designs/common_layout.md` §4 は「認証が必要なURL（/edit_profile等）にアクセスした場合は」と、認証ガードの説明の中で `/edit_profile` という異なるパスを例示している。
- **[事実]** 要件分析 §1.3 が「原典間の不整合」として両者の食い違いをD10として既に記録済み。
- **[空白]** どちらが正か、原典は明言していない。

## 選択肢

1. `ui_design.md` の画面一覧（`/profile/edit`）を採用する。
2. `common_layout.md` の例示（`/edit_profile`）を採用する。

## 提案（および理由）

**選択肢1（`/profile/edit`）を採用する。**

- `ui_design.md` §1「画面一覧」はパスを主題とする構造化された一覧表であり、パスの正典として書かれている。
- `common_layout.md` §4 の `/edit_profile` は「認証ガードの挙動」を説明する文中の**例示**であり、パス自体を主題として定義した記述ではない。
- 構造化された一覧表を、説明文中の例示より優先するのが自然な読み方である。

## 決定（2026-07-19 ユーザー判断）

**P06のパスは `/profile/edit` とする。** decision 0016 §7.2・foundation-plan.md §7 が「フェーズ2。影響は小さい」としていたとおり、importance は `minor`。ユーザーが承認した。

## 影響

- `app/templates/layout.html` のヘッダーのプロフィール編集リンクは `/profile/edit` を指す。
- 後続のP06ハンドラ実装（`web/profile.rs`、`docs/product/issues/04_user_profile_edit.md` 対応）のルーティングも `/profile/edit` に統一する。

## 変更履歴

（新規作成のため、なし）
