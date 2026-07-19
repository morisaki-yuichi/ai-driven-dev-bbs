//! パスワードハッシュ化(argon2)。平文保存・可逆暗号は行わない(CLAUDE.md セキュリティ必須要件)。
//!
//! 呼び出し元(F01登録・F02ログインのハンドラ)はfoundation-plan.md §5の範囲外
//! (機能実装フェーズ)のため、それまでの間 `dead_code` を抑止する。

#![allow(dead_code)]

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};

pub fn hash(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default().hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

pub fn verify(password: &str, hash: &str) -> Result<bool, argon2::password_hash::Error> {
    let parsed = PasswordHash::new(hash)?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_roundtrips() {
        let hashed = hash("correct horse battery staple").unwrap();
        assert!(verify("correct horse battery staple", &hashed).unwrap());
    }

    #[test]
    fn wrong_password_does_not_verify() {
        let hashed = hash("correct horse battery staple").unwrap();
        assert!(!verify("wrong password", &hashed).unwrap());
    }
}
