//! decision 0002(critical, foundation-plan.md §3): 1リクエスト＝1トランザクション。
//! 「ハンドラの入口でトランザクションを開始し、`Err`を返す経路では必ずロールバックする」
//! 規律を、各ハンドラ(register.rs・login.rs・logout.rs)に書き散らさずここ1箇所に
//! 集約する。formal/Bbs/Invariant.leanの`register_atomic`/`login_atomic`/
//! `logout_atomic`(いずれもNoWriteOnError = 失敗時に部分書き込みが残らない)の
//! 実装側の対応物。

use std::future::Future;

use sqlx::{PgPool, Postgres, Transaction};

use crate::web::error::AppError;

/// `pool`からトランザクションを開始し、`f`に貸し出して実行する。
///
/// Why-not: `f`を`FnOnce(&mut Transaction) -> (借用付きFuture)`という形にはしない。
/// その形は`for<'c> FnOnce(&'c mut _) -> TxFuture<'c, T>`という高階トレイト境界と
/// `Box<dyn Future>`を要求し(async closureが安定化していないため)、呼び出し側の
/// 記述が複雑になる。`Transaction`の所有権をクロージャへ渡し、使い終えたら
/// 組にして返してもらう形にすれば、借用のライフタイムに縛られない普通の
/// ジェネリクスで済む。
///
/// `f`が`Ok`を返した場合のみ`commit`する。`Err`を返した場合は`commit`を呼ばずに
/// そのままエラーを伝播させる —— `tx`はこの関数を抜けるときにdropされ、sqlxの
/// `Transaction`は`commit`されないままdropされると自動的にROLLBACKを送信する
/// (sqlx-core `transaction.rs`: 「If neither [commit nor rollback] are called
/// before the transaction goes out-of-scope, `rollback` is called」)。この既定の
/// 振る舞いに乗ることで、「`Err`を返す経路では必ずロールバックする」を明示的な
/// `rollback()`呼び出し無しに保証できる。
pub async fn with_transaction<T, F, Fut>(pool: &PgPool, f: F) -> Result<T, AppError>
where
    F: FnOnce(Transaction<'static, Postgres>) -> Fut,
    Fut: Future<Output = Result<(T, Transaction<'static, Postgres>), AppError>>,
{
    let tx = pool.begin().await?;
    let (value, tx) = f(tx).await?;
    tx.commit().await?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::users;

    async fn count_users(pool: &PgPool) -> i64 {
        sqlx::query_scalar!(r#"select count(*) as "count!" from users"#)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// decision 0002(critical)の中核。閉包が`Err`を返したとき、その閉包の中で
    /// 実際に行った書き込みが残らないこと ―― `NoWriteOnError`
    /// (formal/Bbs/Invariant.leanの`register_atomic`ほか)の実装側の言明。
    /// commitを呼ばずにdropしたTransactionがROLLBACKされる、というsqlxの既定に
    /// 乗っている(上のdocコメント)ので、その既定ごとここで検証する。
    #[sqlx::test]
    async fn err_from_the_closure_rolls_back_writes_made_inside_it(pool: PgPool) {
        let result: Result<(), AppError> = with_transaction(&pool, |mut tx| async move {
            users::insert(&mut *tx, "testuser_01", "hash", "テストユーザー01")
                .await
                .unwrap();
            // トランザクションの内側からは書き込みが見えていることを先に確かめる。
            // これが無いと、後段のcount==0が「ロールバックされた」のか
            // 「そもそも書き込まれていなかった」のか区別できず、テストが空振りする。
            let visible_inside: i64 =
                sqlx::query_scalar!(r#"select count(*) as "count!" from users"#)
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
            assert_eq!(visible_inside, 1);
            // 書き込んだ「後で」失敗する経路。書き込み前に失敗するなら
            // ロールバックの有無は観測できず、テストの意味が無い。
            Err(AppError::Internal("deliberate failure".to_string()))
        })
        .await;

        assert!(result.is_err());
        assert_eq!(
            count_users(&pool).await,
            0,
            "a write made before the closure returned Err must not survive"
        );
    }

    /// アボート済みトランザクションでも同じであること。DBエラー(23505)を起こした
    /// 後にErrで抜ける経路 ―― web/register.rsのユニークID重複がこの形 ―― でも、
    /// 先行する書き込みが残らない。
    #[sqlx::test]
    async fn err_after_a_database_error_leaves_no_rows(pool: PgPool) {
        let result: Result<(), AppError> = with_transaction(&pool, |mut tx| async move {
            users::insert(&mut *tx, "testuser_01", "hash", "テストユーザー01")
                .await
                .unwrap();
            let err = users::insert(&mut *tx, "testuser_01", "hash", "重複")
                .await
                .unwrap_err();
            assert!(users::is_unique_violation(&err));
            Err(AppError::from(err))
        })
        .await;

        assert!(result.is_err());
        assert_eq!(count_users(&pool).await, 0);
    }

    /// 対になる正常系: `Ok`を返した閉包の書き込みはcommitされ、
    /// 同じプールの別の接続から見える(単にロールバックし続ける実装では通らない)。
    #[sqlx::test]
    async fn ok_from_the_closure_commits_writes(pool: PgPool) {
        let id = with_transaction(&pool, |mut tx| async move {
            let id = users::insert(&mut *tx, "testuser_01", "hash", "テストユーザー01").await?;
            Ok((id, tx))
        })
        .await
        .unwrap();

        assert!(id > 0);
        assert_eq!(count_users(&pool).await, 1);
    }
}
