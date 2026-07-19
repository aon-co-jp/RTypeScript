//! `enum`宣言の展開パス。
//!
//! `interface`/`type`と異なり、TypeScriptの`enum`は**実行時表現を持つ**
//! (単なる型レベル構文ではない)。実際の`tsc`は数値enumを、前方
//! マッピング(`Name.Member`→値)と逆引きマッピング(`Name[値]`→
//! メンバー名)の両方を持つIIFE形式のオブジェクトへ展開する。
//!
//! このクレートでの忠実度(正直に書く——ユーザー指示に基づく):
//! - **数値enum**(初期値なし、または数値リテラル初期値)は、`tsc`相当の
//!   前方+逆引きマッピング付きIIFEへ展開する。
//! - **文字列enum**(初期値が文字列リテラル)は、`tsc`と同じく
//!   **逆引きマッピングなし**の前方マッピングのみを生成する
//!   (`tsc`の実際の挙動に合わせた意図的な非対称)。
//! - **計算メンバー**(初期値が数値・文字列リテラル以外の任意の式、
//!   例: `Flag = 1 << 2`・`B = A + 1`)は式トークン列をそのまま埋め込み、
//!   前方マッピングのみ生成する(逆引きは値が実行時までわからないため
//!   省略する——`tsc`ほど厳密な制約チェックはしない、ベストエフォート)。
//! - `const enum`は最適化(呼び出し側への値インライン化)は行わず、
//!   通常のenumと同じIIFE展開にフォールバックする(値は正しいが、
//!   `tsc --isolatedModules`が要求するような最適化はしない、という
//!   正直な割り切り)。

use crate::token::Token;
use crate::transpile::{consume_matching_bracket, is_significant, next_sig_index, token_text};

enum Init {
    None,
    Number(String),
    StringLit(String),
    Other(String),
}

struct EnumMember {
    name: String,
    init: Init,
}

/// `brace_idx+1..close`(enum本体)を、深さ0のカンマで分割して
/// メンバー列を組み立てる。
fn parse_members(tokens: &[Token], brace_idx: usize, close: usize) -> Option<Vec<EnumMember>> {
    let mut members = Vec::new();
    let mut i = brace_idx + 1;

    while i < close {
        let Some(name_idx) = next_sig_index(tokens, i).filter(|&n| n < close) else {
            break;
        };
        let Token::Identifier(name) = &tokens[name_idx] else {
            return None;
        };

        let mut j = name_idx + 1;
        let init = match next_sig_index(tokens, j).filter(|&n| n < close) {
            Some(eq_idx) if matches!(&tokens[eq_idx], Token::Punctuator(p) if p == "=") => {
                // 深さ0のカンマ、または本体終端(`close`)まで初期値式を集める。
                let mut depth = 0i32;
                let mut k = eq_idx + 1;
                let expr_start = k;
                loop {
                    if k >= close {
                        break;
                    }
                    match &tokens[k] {
                        Token::Punctuator(p) if depth == 0 && p == "," => break,
                        // 括弧・角括弧・波括弧のみで深さを追跡する
                        // (`<`/`>`はシフト演算子`<<`・比較演算子として
                        // 使われる方が実用上多いため、ここでは深さ追跡の
                        // 対象に含めない——ジェネリクスを含む計算メンバーの
                        // 深さ追跡は次段階の課題として明示的にスコープ外)。
                        Token::Punctuator(p) if matches!(p.as_str(), "(" | "[" | "{") => {
                            depth += 1;
                            k += 1;
                        }
                        Token::Punctuator(p) if matches!(p.as_str(), ")" | "]" | "}") => {
                            depth -= 1;
                            k += 1;
                        }
                        _ => k += 1,
                    }
                }
                let expr_tokens = &tokens[expr_start..k];
                let significant: Vec<&Token> = expr_tokens.iter().filter(|t| is_significant(t)).collect();
                let init = if significant.len() == 1 {
                    match significant[0] {
                        Token::Number(n) => Init::Number(n.clone()),
                        Token::StringLiteral(s) => Init::StringLit(s.clone()),
                        _ => Init::Other(expr_tokens.iter().map(|t| token_text(t)).collect::<String>().trim().to_string()),
                    }
                } else {
                    Init::Other(expr_tokens.iter().map(|t| token_text(t)).collect::<String>().trim().to_string())
                };
                j = k;
                init
            }
            _ => Init::None,
        };

        members.push(EnumMember { name: name.clone(), init });

        // 次のカンマ(あれば)の直後まで進める。
        match next_sig_index(tokens, j).filter(|&n| n < close) {
            Some(comma_idx) if matches!(&tokens[comma_idx], Token::Punctuator(p) if p == ",") => {
                i = comma_idx + 1;
            }
            _ => break,
        }
    }

    Some(members)
}

fn render_enum(name: &str, members: &[EnumMember]) -> String {
    let mut js = String::new();
    js.push_str("var ");
    js.push_str(name);
    js.push_str(";\n(function (");
    js.push_str(name);
    js.push_str(") {\n");

    let mut next_numeric: i64 = 0;
    for m in members {
        match &m.init {
            Init::None => {
                js.push_str(&format!(
                    "    {name}[{name}[\"{member}\"] = {value}] = \"{member}\";\n",
                    name = name,
                    member = m.name,
                    value = next_numeric
                ));
                next_numeric += 1;
            }
            Init::Number(n) => {
                js.push_str(&format!(
                    "    {name}[{name}[\"{member}\"] = {value}] = \"{member}\";\n",
                    name = name,
                    member = m.name,
                    value = n
                ));
                if let Ok(parsed) = n.parse::<i64>() {
                    next_numeric = parsed + 1;
                }
            }
            Init::StringLit(s) => {
                js.push_str(&format!("    {name}[\"{member}\"] = {value};\n", name = name, member = m.name, value = s));
            }
            Init::Other(expr) => {
                js.push_str(&format!("    {name}[\"{member}\"] = {value};\n", name = name, member = m.name, value = expr));
            }
        }
    }

    js.push_str("})(");
    js.push_str(name);
    js.push_str(" || (");
    js.push_str(name);
    js.push_str(" = {}));");
    js
}

fn drop_trailing_keyword(out: &mut Vec<Token>, kw: &str) {
    let mut idx = out.len();
    while idx > 0 && !is_significant(&out[idx - 1]) {
        idx -= 1;
    }
    if idx > 0 && matches!(&out[idx - 1], Token::Identifier(k) if k == kw) {
        out.truncate(idx - 1);
    }
}

/// `enum`宣言を、前方+逆引きマッピング付きIIFE(数値enum)、または
/// 前方マッピングのみ(文字列enum)へ展開したトークン列を返す。
pub(crate) fn strip_enums(tokens: &[Token]) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut i = 0usize;

    while i < tokens.len() {
        if matches!(tokens[i], Token::Eof) {
            out.push(tokens[i].clone());
            break;
        }

        if let Token::Identifier(name) = &tokens[i] {
            if name == "enum" {
                if let Some(parsed) = (|| {
                    let name_idx = next_sig_index(tokens, i + 1)?;
                    let Token::Identifier(enum_name) = &tokens[name_idx] else {
                        return None;
                    };
                    let brace_idx = next_sig_index(tokens, name_idx + 1)?;
                    if !matches!(&tokens[brace_idx], Token::Punctuator(p) if p == "{") {
                        return None;
                    }
                    let close = consume_matching_bracket(tokens, brace_idx, "{", "}")?;
                    let members = parse_members(tokens, brace_idx, close)?;
                    Some((enum_name.clone(), close, members))
                })() {
                    let (enum_name, close, members) = parsed;
                    drop_trailing_keyword(&mut out, "const");
                    drop_trailing_keyword(&mut out, "export");
                    out.push(Token::Raw(render_enum(&enum_name, &members)));
                    i = close + 1;
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
    use crate::transpile::transpile;

    #[test]
    fn expands_a_simple_numeric_enum_with_forward_and_reverse_mapping() {
        let js = transpile("enum Color { Red, Green, Blue }");
        assert_eq!(
            js,
            "var Color;\n(function (Color) {\n    Color[Color[\"Red\"] = 0] = \"Red\";\n    Color[Color[\"Green\"] = 1] = \"Green\";\n    Color[Color[\"Blue\"] = 2] = \"Blue\";\n})(Color || (Color = {}));"
        );
    }

    #[test]
    fn expands_a_numeric_enum_with_explicit_and_continued_values() {
        let js = transpile("enum Status { Ok = 1, Retry, Fail = 10 }");
        assert_eq!(
            js,
            "var Status;\n(function (Status) {\n    Status[Status[\"Ok\"] = 1] = \"Ok\";\n    Status[Status[\"Retry\"] = 2] = \"Retry\";\n    Status[Status[\"Fail\"] = 10] = \"Fail\";\n})(Status || (Status = {}));"
        );
    }

    #[test]
    fn expands_a_string_enum_without_reverse_mapping() {
        let js = transpile("enum Direction { Up = \"UP\", Down = \"DOWN\" }");
        assert_eq!(
            js,
            "var Direction;\n(function (Direction) {\n    Direction[\"Up\"] = \"UP\";\n    Direction[\"Down\"] = \"DOWN\";\n})(Direction || (Direction = {}));"
        );
    }

    #[test]
    fn strips_export_and_const_modifiers_from_an_enum_declaration() {
        let js = transpile("export const enum Flag { None, One }");
        assert_eq!(
            js,
            "var Flag;\n(function (Flag) {\n    Flag[Flag[\"None\"] = 0] = \"None\";\n    Flag[Flag[\"One\"] = 1] = \"One\";\n})(Flag || (Flag = {}));"
        );
    }

    #[test]
    fn handles_a_computed_member_expression_with_forward_mapping_only() {
        let js = transpile("enum Bits { A = 1 << 0, B = 1 << 1 }");
        assert_eq!(
            js,
            "var Bits;\n(function (Bits) {\n    Bits[\"A\"] = 1 << 0;\n    Bits[\"B\"] = 1 << 1;\n})(Bits || (Bits = {}));"
        );
    }
}
