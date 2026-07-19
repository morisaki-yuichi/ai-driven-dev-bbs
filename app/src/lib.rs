//! `tests/`の統合テストから`db`/`domain`/`web`を参照できるよう、
//! バイナリ(`main.rs`)とは別にライブラリターゲットとして公開する。

pub mod db;
pub mod domain;
pub mod web;
