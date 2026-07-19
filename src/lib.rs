//! RTypeScript — TypeScript相当を既存実装のコードを一切流用せず
//! 一から開発するプロジェクト(`RHTML5`/`RCSS3`/`RTypeScript`/
//! `RBootStrap`構想の一部、2026-07-18新設)。
//!
//! スコープが非常に大きい(フルの型システム・型推論・構文解析)ため、
//! 無理に全体実装を狙わず**最小のスコープ**から立ち上げる方針
//! (ユーザー指示、2026-07-18): 単純な型注釈の削除・JSへの
//! トランスパイルのみ。フル型チェックは対象外。
//!
//! ## 現状(第一段)
//! - `token`/`tokenizer`: `rhtml5`の`Token`/`TokenSink`パターンを
//!   踏襲した、識別子・数値・文字列・記号・空白・コメントの字句解析器。
//! - `transpile`: 変数宣言・関数パラメータ・戻り値型の単純な型注釈を
//!   トークン列から除去し、JS相当のテキストを再構成する。
//!
//! ## 未対応(次段階)
//! `interface`/`type`宣言、ジェネリクス制約、複数変数宣言子
//! (`let x: T = 1, y: U = 2;`の2つ目以降)、Wasmへの直接コンパイル
//! (実装方針B案)、swc AST取り込み(C案)。

mod classes;
mod enums;
mod expressions;
mod generics;
mod interfaces;
pub mod token;
pub mod tokenizer;
pub mod transpile;

pub use token::{CollectingSink, Token, TokenSink};
pub use tokenizer::{tokenize, Tokenizer};
pub use transpile::{strip_type_annotations, transpile};
