---
name: verify
description: Build and run this project's Axum app on the host, then drive it with curl against the live dev DB to observe real behavior (login, CSRF, session cookies, auth-gated routes). Use before considering a feature done.
---

# verify: 手を動かして実際の挙動を確認する

このプロジェクトは Axum + Askama + PostgreSQL の SSR アプリ（`app/`）。
`cargo test` はロジックの正しさを保証するが、実際に HTTP サーバとして
起動してブラウザ相当のリクエストを送ったときの挙動（Cookie・リダイレクト・
ヘッダ）はテストだけでは見えないので、機能実装の仕上げに必ずこれをやる。

## 起動

開発DBはホスト側で既に起動していることが多い（他プロジェクトとポート
競合するため `compose.yaml` の `5432:5432` がそのまま使えないことがある）。
まず生きているか確認する:

```bash
docker ps --format '{{.Names}}\t{{.Ports}}' | grep -i postgres
```

`app/.env` の `DATABASE_URL` がホスト側のポートに向いているか確認してから
（`.env.example` は5432前提だが、競合時は`.env`側だけ実際のポートに書き換え
られていることがある。`.env`はコミット対象外なので過去セッションの変更が
残っている場合がある）:

```bash
cd app
cargo run &         # ホストのcargoでビルド・起動。マイグレーションは起動時に自動適用される
sleep 5
curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:3000/   # 303ならOK(未ログインでログイン画面へ)
```

## 手で叩くときの落とし穴

- **Bashツールはコマンドごとにシェルが切り替わり、環境変数は引き継がれない。**
  `CSRF=$(...)` を1回のBash呼び出しで設定し、別の呼び出しで参照すると空文字列になり、
  CSRFトークン不一致で403になる（tokens_matchは空文字列を常に不一致とする —
  decision 0021）。**1つのBash呼び出しの中で取得と使用を完結させる**か、
  ファイル(`cookies.txt`)から都度読み直すこと。
- CSRFトークンはCookieジャー(`-b cookies.txt -c cookies.txt`)と、フォームの
  `csrf_token`フィールドの両方に同じ値を積む。`grep -o 'csrf_token\s\S*' cookies.txt`
  で取り出せる。
- 状態変更系(POST)には`-H "Origin: http://127.0.0.1:3000"`を付けること
  (decision 0021の同一オリジン検証。無いと403)。

## 典型的な確認フロー(ログイン機能の例)

```bash
# 1. 未ログインで保護ルートへ -> /login へリダイレクトされること(C-09)
curl -s -i http://127.0.0.1:3000/ | head -5

# 2. GET /login でCSRF Cookie + hidden inputを取得
curl -s -c cookies.txt http://127.0.0.1:3000/login -o login.html
CSRF=$(grep -o 'csrf_token\s\S*' cookies.txt | awk '{print $2}')

# 3. 誤ったパスワードでPOST -> 200 + 「IDまたはパスワードが正しくありません」
curl -s -i -b cookies.txt -H "Origin: http://127.0.0.1:3000" \
  --data-urlencode "unique_id=testuser_01" --data-urlencode "password=wrong" \
  --data-urlencode "csrf_token=$CSRF" http://127.0.0.1:3000/login

# 4. 正しいパスワードでPOST -> 303 + Set-Cookie: session_id=... + csrf_tokenのローテーション
curl -s -i -b cookies.txt -c cookies.txt -H "Origin: http://127.0.0.1:3000" \
  --data-urlencode "unique_id=testuser_01" --data-urlencode "password=correct" \
  --data-urlencode "csrf_token=$CSRF" http://127.0.0.1:3000/login

# 5. セッションCookieで保護ルートへ -> 200 + Cache-Control: no-store(C-11)
curl -s -i -b cookies.txt http://127.0.0.1:3000/
```

DB側の状態は直接見るのが早い(セッション行の有無で原子性を確認するなど):

```bash
PGPASSWORD=bbs psql -h localhost -p <実際のポート> -U bbs -d bbs -c "select * from sessions;"
```

## 終了

```bash
pkill -f 'target/debug/bbs'
```
