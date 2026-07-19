//! 式レベルの型構文(非nullアサーション`!`・`as Type`キャスト)を
//! トークン列から除去するパス。
//!
//! 対応する形:
//! - **非nullアサーション**: `foo!.bar`・`foo!()`・`arr[0]!` — 値の直後に
//!   付く後置の`!`のみ除去する。前置の論理否定(`!foo`)や`!=`/`!==`
//!   (トークナイザ側で既に単一トークン化済み、`tokenizer.rs`参照)とは
//!   別のトークンなので混同しない。
//! - **`as Type`キャスト**: `expr as Type`の`as`以降(型部分)を除去する。
//!
//! **意図的にスコープ外**: `<Type>expr`という前置キャスト構文
//! (TypeScriptの古い記法)は、ジェネリクス`<T>`呼び出しやJSXタグと
//! 構文上区別がつきにくく、他のヒューリスティックパスと同様「安全側」
//! に倒すとほぼ何もできないため、今回は対応しない(`as`構文の方が
//! 現代のTypeScriptコードベースで圧倒的に主流のため、実用上の影響は
//! 小さいと判断)。
//!
//! `?.`(オプショナルチェイニング)・`??`(nullish合体)は、
//! トークナイザが`?`単体とは別の複数文字トークンとして扱うため
//! (`tokenizer.rs`のPUNCTUATORS一覧参照)、このパスが後置`!`を探す際に
//! 誤って`?`絡みのトークンを型構文と誤認することはない。

use crate::token::Token;
use crate::transpile::{is_significant, next_sig_index};

/// 直前の意味トークンが「値の終端」(後置`!`が非nullアサーションとして
/// 妥当な位置)かどうかを判定する。
fn is_value_end(tok: &Token) -> bool {
    match tok {
        Token::Identifier(s) => !matches!(
            s.as_str(),
            // これらの直後の`!`は非nullアサーションではなく前置の論理否定
            // (例: `return !foo`・`typeof !foo`は文法上あり得ないが、
            // `return`直後は明らかに値の終端ではない)。
            "return" | "typeof" | "delete" | "void" | "in" | "of" | "new" | "instanceof" | "yield" | "await"
        ),
        Token::Number(_) | Token::StringLiteral(_) | Token::Raw(_) => true,
        Token::Punctuator(p) => matches!(p.as_str(), ")" | "]"),
        _ => false,
    }
}

pub(crate) fn strip_expression_type_syntax(tokens: &[Token]) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];

        // 後置`!`(非nullアサーション): 直前に出力済みの意味トークンが
        // 値の終端であれば、この`!`だけ読み飛ばす(出力しない)。
        if let Token::Punctuator(p) = tok {
            if p == "!" {
                if let Some(prev) = out.iter().rev().find(|t| is_significant(t)) {
                    if is_value_end(prev) {
                        i += 1;
                        continue;
                    }
                }
            }
        }

        // `as Type`: 意味トークンとして`as`が出現し、直前が値の終端の
        // 場合のみキャストとして扱う(`as`は予約語ではないため、変数名
        // として使われるケース—`const as = 1;`—との区別が必要)。
        if let Token::Identifier(s) = tok {
            if s == "as" {
                let prev_is_value_end = out
                    .iter()
                    .rev()
                    .find(|t| is_significant(t))
                    .map(is_value_end)
                    .unwrap_or(false);
                if prev_is_value_end {
                    if let Some(next_sig) = next_sig_index(tokens, i + 1) {
                        // `as`の直後が型名らしい識別子(`const`のような
                        // アサーション`as const`も含め、識別子1つで
                        // 十分実用的にカバーできる)であれば、`as`から
                        // その型注釈の終わりまでを丸ごと読み飛ばす。
                        if matches!(&tokens[next_sig], Token::Identifier(_)) {
                            let mut end = next_sig + 1;
                            // 型が配列(`as Type[]`)・ユニオン
                            // (`as A | B`)の場合も、後続の`[`/`]`/`|`/
                            // 型名トークン列である間は取り込む。
                            loop {
                                let Some(sig) = next_sig_index(tokens, end) else { break };
                                let is_array_or_union = match &tokens[sig] {
                                    Token::Punctuator(p) => matches!(p.as_str(), "[" | "]" | "|" | "&" | "."),
                                    Token::Identifier(_) => true,
                                    _ => false,
                                };
                                if is_array_or_union && sig == end {
                                    end = sig + 1;
                                } else {
                                    break;
                                }
                            }
                            i = end;
                            continue;
                        }
                    }
                }
            }
        }

        out.push(tok.clone());
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::CollectingSink;
    use crate::tokenizer::tokenize;
    use crate::transpile::token_text as tt;

    fn strip_and_render(src: &str) -> String {
        let mut sink = CollectingSink::default();
        tokenize(src, &mut sink);
        let stripped = strip_expression_type_syntax(&sink.tokens);
        stripped.iter().map(tt).collect()
    }

    #[test]
    fn strips_postfix_non_null_assertion_after_identifier() {
        assert_eq!(strip_and_render("foo!.bar();"), "foo.bar();");
    }

    #[test]
    fn strips_non_null_assertion_after_call_and_index() {
        assert_eq!(strip_and_render("getFoo()!.bar;"), "getFoo().bar;");
        assert_eq!(strip_and_render("arr[0]!.length;"), "arr[0].length;");
    }

    #[test]
    fn does_not_strip_prefix_logical_not() {
        assert_eq!(strip_and_render("if (!foo) { bar(); }"), "if (!foo) { bar(); }");
    }

    #[test]
    fn does_not_confuse_not_equal_operators() {
        assert_eq!(strip_and_render("if (a !== b) { c(); }"), "if (a !== b) { c(); }");
        assert_eq!(strip_and_render("if (a != b) { c(); }"), "if (a != b) { c(); }");
    }

    #[test]
    fn strips_as_type_cast() {
        assert_eq!(strip_and_render("const x = foo as Bar;"), "const x = foo ;");
    }

    #[test]
    fn strips_as_const_assertion() {
        assert_eq!(strip_and_render("const x = [1, 2] as const;"), "const x = [1, 2] ;");
    }

    #[test]
    fn does_not_strip_as_used_as_a_plain_identifier() {
        assert_eq!(strip_and_render("const as = 1;"), "const as = 1;");
    }

    #[test]
    fn leaves_optional_chaining_and_nullish_coalescing_untouched() {
        assert_eq!(strip_and_render("const x = foo?.bar ?? baz;"), "const x = foo?.bar ?? baz;");
    }

    #[test]
    fn leaves_optional_chaining_call_untouched() {
        assert_eq!(strip_and_render("foo?.bar?.();"), "foo?.bar?.();");
    }
}
