---
id: 0024
title: 登録成功後のログイン画面での通知方式（クエリパラメータによるフラッシュ表示）
date: 2026-07-20
importance: minor
decided_by: ai+user
status: 決定済
---

# 0024 登録成功後のログイン画面での通知方式（クエリパラメータによるフラッシュ表示）

- 関連論点: なし（新規。H-12の観測性改善のためのセッションで判明）。
- 関連原典: なし（原典はP01/P02間の遷移で成功通知を出すことを明示的には要求していない）。ただし `dev-docs/ui-ux-guidelines.md` §2 が「成功: 共通メッセージエリアに正常系通知を出す」ことを一般原則として定めており、本論点はその適用方法の具体化。
- 影響範囲: `app/src/web/register.rs`（`submit`のリダイレクト先）、`app/src/web/login.rs`（`show`）、`app/templates/login.html`（共通メッセージエリア）、`app/tests/register_test.rs` / `app/tests/login_test.rs`。

## 背景（原典が何を言い、何を言っていないか）

**[事実]** F01（AC01-5）は登録成功後 `/login` へリダイレクトすることのみを規定し、遷移先での表示内容までは規定していない。`ui-ux-guidelines.md` §2 は「成功時は共通メッセージエリアに正常系通知を出す」ことを一般原則として要求している。

**[事実]** decision 0008 によりこのプロジェクトはSSR/MPA限定（JavaScriptを使わない）。decision 0021（CSRF）により、未ログイン状態のCookieはCSRF二重送信トークン専用であり、認証以外の目的で新規のCookie機構やサーバ側セッション状態を増やすことは、CSRF方式の前提（秘匿キー無し・DB非依存のステートレストークン）と独立に保つ必要がある。

**[空白]** 「登録完了」という1回限りのイベントを、リダイレクト先の画面にどう伝えるか（=いわゆるflashメッセージの実現方式）は原典・既存decisionのいずれも規定していない。

## 選択肢

### (a) クエリパラメータ

`POST /register` 成功時に `Redirect::to("/login?registered=1")` のようにクエリ文字列を付けてリダイレクトし、`GET /login` 側でその値を見て成功メッセージを出す。

- 利点: サーバ側に状態を持たない。Cookieもセッションも増やさない。decision 0021のCSRF機構と完全に独立(混同・相互作用のリスクがゼロ)。GETへのパラメータ付与はdecision 0011/0013で既にクエリ文字列でパラメータを保持するパターンがある。JS不要でdecision 0008と整合。
- 欠点: URLに状態が残る。ユーザーがそのURLをブックマーク・再読み込みすると通知が再表示され続ける（本件は「見えた」ことが害にならない性質の通知なので実害は小さい）。

### (b) 専用のフラッシュ用Cookie

登録成功時に短命のCookie（例: `flash=registered`）をセットしてリダイレクトし、`GET /login`側でCookieを読んで表示後に削除する。

- 利点: URLが汚れない。再読み込みでは消える(Cookie削除のタイミング次第)。
- 欠点: 新しいCookie種別が増える。decision 0021がCSRF用Cookieの属性・ローテーション規律を丁寧に定義した直後に、目的の異なるCookieを追加すると、「Cookie=CSRF専用」という単純な対応関係が崩れ、実装・レビューの見通しを悪くする。読み取り後に消す一撃仕様の実装(サーバがGETで読んで即座に無効化するSet-Cookie)は、二重送信トークンの「一度きりにしない」（decision 0021 決定4）という設計判断と隣接していて紛らわしい。

### (c) セッションに状態を持たせる（同期トークン相当の仕組み）

- 却下: 登録は未ログインの操作でありセッションが存在しない（decision 0021の背景と同じ制約）。この1件のためだけに未ログイン状態の何らかの永続状態を新設するのは過剰。

## 提案（および理由）

**(a) クエリパラメータを採る。** 状態を一切増やさずに実現でき、`ui-ux-guidelines.md`が要求する「成功時の共通メッセージエリア表示」を満たす最小の手段だから。(b)のCookie方式は実現可能だが、decision 0021が確立したばかりの「Cookie=CSRFトークン」という単純な対応を崩すコストに見合わない。

具体的な設計:

- `POST /register`成功時のリダイレクト先を `/login` から `/login?registered=1` に変更する。
- `GET /login`はクエリパラメータ`registered`の有無（true相当の値かどうかは問わず、キーの存在のみを見る）を見て、存在すれば共通メッセージエリアに成功メッセージを表示する。文言はC-01（固定文言）の対象外の新規文言であり、既存の失敗メッセージ（`login.html`の`form_message`）と同じ`.message`領域・`aria-live`機構に載せるが、クラスは`.message-success`（`static/app.css`に既存・未使用）を使う。
- 文言案: 「登録が完了しました。ログインしてください。」（既存の失敗文言「ログインできませんでした。入力内容を確認してください。」とトーンを揃える）。
- ログイン失敗時（`POST /login`失敗の再表示）はこのクエリパラメータを保持しない・見ない。失敗メッセージと成功メッセージが同時に出ることはない（`GET /login`表示時のみ成功メッセージの対象、`POST /login`失敗時の再描画は`render_form`が別経路で`form_message`のみを使う）。

## 決定（2026-07-20 ユーザー判断）

登録成功時のリダイレクト先を `/login?registered=1` とし、`GET /login` がこのクエリパラメータの有無を見て共通メッセージエリアに成功通知（「登録が完了しました。ログインしてください。」）を表示する方式を採用する。ユーザーが承認した。

## 影響

- `app/src/web/register.rs`: `submit`内の`Redirect::to("/login")`を`Redirect::to("/login?registered=1")`に変更。
- `app/src/web/login.rs`: `show`が`Query<HashMap<String, String>>`（または専用の軽量構造体）を受け取り、`registered`キーの有無を`render_form`系の描画関数に渡す。`LoginTemplate`に成功メッセージ用のフィールド（例: `success_message: Option<String>`）を追加する。既存の`form_message`（失敗用）とは別フィールドにし、意味を混同しない。
- `app/templates/login.html`: 共通メッセージエリアに成功メッセージの分岐を追加（`.message-success`クラス、既存の失敗表示`.message-error`と同じ`<div class="message-area">`内）。
- `app/tests/register_test.rs`: 成功時のリダイレクト先URLに`?registered=1`が付くことを検証するテストを追加・更新。
- `app/tests/login_test.rs`: `GET /login?registered=1`で成功メッセージが表示されること、`GET /login`（パラメータ無し）では表示されないことを検証するテストを追加。
- スキーマ・decision 0021（CSRF）・decision 0008（SSR/MPA）への影響なし。

## 変更履歴

- 2026-07-20: 起票時は AI 単独判断（`decided_by: ai` / `status: 提案`）だったが、本セッションのレビュー裁定でユーザーが内容を確認し承認した。`decided_by` を `ai+user`、`status` を `決定済` に更新。決定内容そのものは変更していない。
- 2026-07-20: レビュー指摘により、採用理由・利点の記述を実装の実態に合わせて訂正した。「既存の`web/params.rs`のクエリパラメータパターンと実装の型が揃う」としていたが、実装では`registered`パラメータの型を`web/params.rs`に置かず、`login.rs`の`show`内で`HashMap<String, String>`に対する`contains_key`をインラインで行っている（CLAUDE.mdが定める「単純な処理にこの分離のためだけの層を作らない」という既定の型どおり）。決定内容（クエリパラメータ方式そのもの）に変更はない。
