//! `interface`宣言・`type`エイリアス宣言をトークン列から丸ごと除去する
//! パス。どちらもTypeScriptの型レベル構文で、JSランタイム表現を
//! 一切持たないため(`class`と違って実行時オブジェクトを生成しない)、
//! 出力からは完全に消える(コメント化ではなく削除)。
//!
//! パーサーではなくヒューリスティックなトークン走査のため、以下の
//! 形式にのみ対応する:
//! - `interface Name { ... }` / `interface Name<T> extends Base<T> { ... }`
//! - `type Name = ...;` / `type Name<T> = ...;`
//! - 両方とも先頭の`export`修飾子(あれば)も合わせて除去する。
//!
//! 想定形からずれる場合(次の意味トークンが`{`/`=`ではない等)は
//! **安全側に倒して除去を諦め**、`interface`/`type`を普通の識別子
//! として素通しする(型注釈除去ではなく構文破壊の方が害が大きいため)。

use crate::token::Token;
use crate::transpile::{consume_angle_generic, consume_matching_bracket, is_significant, next_sig_index};

fn consume_brace_body(tokens: &[Token], open: usize) -> Option<usize> {
    consume_matching_bracket(tokens, open, "{", "}")
}

/// `tokens[start]`が`interface`識別子である前提で、宣言全体
/// (`interface`〜対応する`}`)が確実に判定できた場合、その次の
/// トークンのインデックスを返す(消費し切れなければ`None`)。
fn try_consume_interface(tokens: &[Token], start: usize) -> Option<usize> {
    let name_idx = next_sig_index(tokens, start + 1)?;
    if !matches!(tokens.get(name_idx), Some(Token::Identifier(_))) {
        return None;
    }
    let mut i = name_idx + 1;
    loop {
        let sig = next_sig_index(tokens, i)?;
        match &tokens[sig] {
            Token::Punctuator(p) if p == "<" => {
                i = consume_angle_generic(tokens, sig)? + 1;
            }
            Token::Punctuator(p) if p == "{" => {
                let close = consume_brace_body(tokens, sig)?;
                return Some(close + 1);
            }
            Token::Identifier(kw) if kw == "extends" || kw == "implements" => {
                i = sig + 1;
            }
            Token::Identifier(_) | Token::Punctuator(_) => {
                // extends/implements節中の型名・`,`・`.`(修飾名)。
                i = sig + 1;
            }
            _ => return None,
        }
    }
}

/// `tokens[start]`が`type`識別子である前提で、`type Name = ...;`
/// (トップレベルの`;`まで、括弧・角括弧・波括弧・山括弧の深さを
/// 追跡する)を1つの宣言として消費できた場合、その次のインデックスを
/// 返す。
fn try_consume_type_alias(tokens: &[Token], start: usize) -> Option<usize> {
    let name_idx = next_sig_index(tokens, start + 1)?;
    if !matches!(tokens.get(name_idx), Some(Token::Identifier(_))) {
        return None;
    }
    let mut i = name_idx + 1;
    // 任意のジェネリクス`<T>`。
    if let Some(sig) = next_sig_index(tokens, i) {
        if matches!(&tokens[sig], Token::Punctuator(p) if p == "<") {
            i = consume_angle_generic(tokens, sig)? + 1;
        }
    }
    let eq_idx = next_sig_index(tokens, i)?;
    if !matches!(&tokens[eq_idx], Token::Punctuator(p) if p == "=") {
        return None;
    }

    let mut depth = 0i32;
    let mut j = eq_idx + 1;
    loop {
        match tokens.get(j) {
            None => return Some(j),
            Some(Token::Eof) => return Some(j),
            Some(Token::Punctuator(p)) => match p.as_str() {
                "(" | "[" | "{" | "<" => {
                    depth += 1;
                    j += 1;
                }
                ")" | "]" | "}" | ">" => {
                    depth -= 1;
                    j += 1;
                }
                ";" if depth <= 0 => return Some(j + 1),
                _ => j += 1,
            },
            _ => j += 1,
        }
    }
}

/// すでに出力済みのトークン列の末尾が`export`(+空白)であれば、
/// それも合わせて取り除く(`export interface`/`export type`の
/// `export`だけが取り残されて構文エラーになるのを防ぐ)。
fn drop_trailing_export(out: &mut Vec<Token>) {
    let mut idx = out.len();
    while idx > 0 && !is_significant(&out[idx - 1]) {
        idx -= 1;
    }
    if idx > 0 && matches!(&out[idx - 1], Token::Identifier(kw) if kw == "export") {
        out.truncate(idx - 1);
    }
}

/// `interface`/`type`宣言を丸ごと除去したトークン列を返す。
pub(crate) fn strip_interfaces_and_type_aliases(tokens: &[Token]) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut i = 0usize;
    while i < tokens.len() {
        if matches!(tokens[i], Token::Eof) {
            out.push(tokens[i].clone());
            break;
        }

        if let Token::Identifier(name) = &tokens[i] {
            if name == "interface" {
                if let Some(end) = try_consume_interface(tokens, i) {
                    drop_trailing_export(&mut out);
                    i = end;
                    continue;
                }
            } else if name == "type" {
                if let Some(end) = try_consume_type_alias(tokens, i) {
                    drop_trailing_export(&mut out);
                    i = end;
                    continue;
                }
            }
        }

        out.push(tokens[i].clone());
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transpile::transpile;

    #[test]
    fn strips_a_simple_interface_declaration() {
        assert_eq!(transpile("interface Foo { x: number }\nconst y = 1;"), "\nconst y = 1;");
    }

    #[test]
    fn strips_an_exported_interface_with_extends_and_generics() {
        let ts = "export interface Box<T> extends Container<T> {\n  value: T;\n}\nconst z = 1;";
        assert_eq!(transpile(ts), "\nconst z = 1;");
    }

    #[test]
    fn strips_a_type_alias_with_object_type_and_semicolons_inside() {
        let ts = "type Foo = { a: number; b: string };\nconst z = 1;";
        assert_eq!(transpile(ts), "\nconst z = 1;");
    }

    #[test]
    fn strips_a_generic_type_alias() {
        assert_eq!(transpile("type Box<T> = { value: T };\nlet a = 1;"), "\nlet a = 1;");
    }

    #[test]
    fn does_not_touch_a_plain_variable_literally_named_type_or_interface() {
        // `type`/`interface`はJSの予約語ではないため、通常の識別子として
        // 使われている場合(`=`が続かない・`{`が続かない)は素通しする。
        assert_eq!(transpile("type.push(1);"), "type.push(1);");
    }
}
