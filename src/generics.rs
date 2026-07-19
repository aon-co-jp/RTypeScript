//! ジェネリクス型パラメータ(`<T>`)をトークン列から除去するパス。
//!
//! 対応する形:
//! - 宣言側: `function name<T>(...)`・`class name<T> extends/implements/{`
//! - 呼び出し/メソッド側: `name<Type, Type2>(...)`
//!   (`identity<T>(x)`のような呼び出し時ジェネリクスも含む)
//!
//! `<`/`>`は比較演算子としても使われるため(`a < b`のような式)、
//! パーサーを持たないこのクレートでは完全な判定はできない。安全側の
//! ヒューリスティックとして、以下の**両方**を満たす場合のみ除去する:
//! 1. `<`と対応する`>`の間の中身が「型らしい」トークン列(識別子・
//!    `,`・`.`・`[`・`]`・`<`・`>`・`|`・`&`・`=`・`extends`のみ)である
//! 2. 直前の意味トークンが`function`/`class`キーワート、または
//!    (宣言文脈でなくても)`>`の直後が`(`である(呼び出し/メソッド
//!    シグネチャの形)
//!
//! いずれかが崩れる場合(例: `a < b && c > d`のような論理式)は
//! 除去せず素通しする——型注釈除去の取りこぼしよりも、有効なJS式を
//! 誤って壊す方が実害が大きいため。

use crate::token::Token;
use crate::transpile::{consume_angle_generic, next_sig_index};

fn last_significant_text(out: &[Token]) -> Option<&str> {
    out.iter().rev().find_map(|t| match t {
        Token::Identifier(s) => Some(s.as_str()),
        Token::Punctuator(s) => Some(s.as_str()),
        Token::Number(_) | Token::StringLiteral(_) => None,
        _ => None,
    })
}

/// `lt+1..gt`(山括弧の中身)が型パラメータ列らしいトークンだけで
/// 構成されているかを判定する(比較式・論理式との区別のため)。
fn looks_type_like(tokens: &[Token], lt: usize, gt: usize) -> bool {
    for tok in &tokens[lt + 1..gt] {
        match tok {
            Token::Identifier(_) | Token::Whitespace(_) | Token::Comment(_) => {}
            Token::Punctuator(p) => {
                if !matches!(p.as_str(), "," | "." | "[" | "]" | "<" | ">" | "|" | "&" | "=" | "extends") {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// ジェネリクス型パラメータを除去したトークン列を返す。
pub(crate) fn strip_generics(tokens: &[Token]) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut i = 0usize;

    while i < tokens.len() {
        if matches!(tokens[i], Token::Eof) {
            out.push(tokens[i].clone());
            break;
        }

        if matches!(tokens[i], Token::Identifier(_)) {
            if let Some(lt) = next_sig_index(tokens, i + 1) {
                if matches!(&tokens[lt], Token::Punctuator(p) if p == "<") {
                    if let Some(gt) = consume_angle_generic(tokens, lt) {
                        if let Some(after) = next_sig_index(tokens, gt + 1) {
                            let prev_kw = last_significant_text(&out);
                            // `class Box<T> extends Base<T>`のように、基底クラスの
                            // 型引数(`extends`/`implements`直後の識別子のジェネリクス)
                            // も宣言側と同じ扱いで除去したいので、直前が
                            // `function`/`class`キーワードだけでなく`extends`/
                            // `implements`である場合も宣言文脈とみなす。
                            let decl_context = matches!(prev_kw, Some("function") | Some("class") | Some("extends") | Some("implements"));
                            let decl_follow_ok = matches!(&tokens[after], Token::Punctuator(p) if p == "(" || p == "{")
                                || matches!(&tokens[after], Token::Identifier(kw) if kw == "extends" || kw == "implements");
                            let call_or_method_follow = matches!(&tokens[after], Token::Punctuator(p) if p == "(");

                            let should_strip = if decl_context {
                                decl_follow_ok
                            } else {
                                call_or_method_follow && looks_type_like(tokens, lt, gt)
                            };

                            if should_strip {
                                out.push(tokens[i].clone());
                                i = gt + 1;
                                continue;
                            }
                        }
                    }
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
    use crate::transpile::transpile;

    #[test]
    fn strips_generic_type_parameter_from_a_function_declaration() {
        assert_eq!(transpile("function identity<T>(x: T): T { return x; }"), "function identity(x) { return x; }");
    }

    #[test]
    fn strips_generic_type_parameters_from_a_class_declaration() {
        assert_eq!(
            transpile("class Box<T> extends Base<T> {\n  value;\n}"),
            "class Box extends Base {\n  value;\n}"
        );
    }

    #[test]
    fn strips_call_site_generics() {
        assert_eq!(transpile("foo<string>(x);"), "foo(x);");
        assert_eq!(transpile("const xs = identity<number>(1);"), "const xs = identity(1);");
    }

    #[test]
    fn strips_multiple_generic_type_parameters_and_constraints() {
        assert_eq!(
            transpile("function pick<T, K extends keyof T>(obj: T, key: K) { return obj[key]; }"),
            "function pick(obj, key) { return obj[key]; }"
        );
    }

    #[test]
    fn does_not_mangle_less_than_and_greater_than_comparisons() {
        assert_eq!(transpile("if (a < b && c > d) { x(); }"), "if (a < b && c > d) { x(); }");
        assert_eq!(transpile("const ok = a < b;"), "const ok = a < b;");
    }

    #[test]
    fn does_not_mangle_a_generic_looking_comparison_chain_without_a_call() {
        // `a<b>(c)`の形はcall-siteジェネリクスと区別がつかないため許容
        // するが、末尾に`(`が続かない比較演算はそのまま残ることを確認。
        assert_eq!(transpile("const r = a < b > c;"), "const r = a < b > c;");
    }
}
