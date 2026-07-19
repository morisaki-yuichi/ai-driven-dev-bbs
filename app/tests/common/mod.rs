//! 結合テスト共通のヘルパ。`login_test.rs` / `register_test.rs` / `logout_test.rs`
//! がそれぞれ`mod common;`で取り込む。
//!
//! Why: 統合テストは1ファイル1バイナリなので、`mod common;`で取り込んだ側から
//! 使われない関数は各バイナリで`dead_code`警告になる(3ファイルすべてが全ヘルパを
//! 使うわけではない)。共有ヘルパ置き場としては正常な状態なので、モジュール単位で
//! 許可する。

#![allow(dead_code)]

use bbs::db::password;
use sqlx::PgPool;

/// 同一オリジン検証(decision 0021)を通すために、リクエストの`Host`と
/// `Origin`で同じ値を使う。
pub const HOST: &str = "example.test";

pub fn origin_header() -> String {
    format!("http://{HOST}")
}

/// ハンドラを経由せずにユーザーを1件用意する(登録の検証ではなく、
/// ログイン・ログアウトの前提を作るためのヘルパ)。
pub async fn insert_test_user(pool: &PgPool, unique_id: &str, plain_password: &str) -> i64 {
    let hash = password::hash(plain_password).unwrap();
    sqlx::query_scalar!(
        "insert into users (unique_id, password_hash, display_name) values ($1, $2, $3) returning id",
        unique_id,
        hash,
        "テストユーザー01"
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// テスト用の最小限のパーセントエンコード(記号を含むパスワードを送るため)。
/// 本番のフォーム送信はブラウザが行うので、実装側にエンコーダは要らない。
pub fn urlencoding_stub(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
