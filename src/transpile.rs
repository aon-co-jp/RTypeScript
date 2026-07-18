//! 最小スコープのトランスパイラ: **単純な型注釈の削除のみ**を行い、
//! TypeScript(のごく一部の構文サブセット)をJS相当のテキストへ変換
//! する。フルの型チェック・型推論・`interface`/`type`宣言・
//! ジェネリクス制約等は一切扱わない(ユーザー指示: 「まず最小の
//! スコープで立ち上げる」——RTypeScriptはRHTML/RCSSに比べスコープが
//! 巨大なため、最初の一歩として型注釈除去のみに絞る)。
//!
//! 対応する型注釈の位置(ヒューリスティックにトークン列を追跡する、
//! 完全なパーサーではない):
//! - 変数宣言: `let x: number = 1;` → `let x = 1;`
//!   (`let`/`const`/`var` 直後の識別子に続く`:`のみを対象。
//!   複数宣言子(`let x: T = 1, y: U = 2;`)の2つ目以降は次段階の課題)
//! - 関数パラメータ: `function f(a: number, b: string) {}` →
//!   `function f(a, b) {}` (`(`または`,`直後の識別子に続く`:`)
//! - 戻り値型: `function f(): number { ... }` → `function f() { ... }`
//!   (`)`直後の`:`で、型注釈の終端が`{`または`=>`である場合のみ確定
//!   ——`(a ? b : c)`のような紛らわしいケースを誤って削らないための
//!   フォールバック確認)
//!
//! 型注釈の終端は、`(`/`[`/`{`のネスト深さを追跡しつつ、深さ0の位置に
//! 現れる区切り記号(`,`・`;`・`=`・`)`・`{`・`=>`)まで、という
//! ヒューリスティックで判定する(ジェネリクス`<T>`のようなケースの
//! 深さ追跡はスコープ外、単純な型名・配列型・関数型程度を想定)。

use crate::token::{CollectingSink, Token};
use crate::tokenizer::tokenize;

fn is_significant(t: &Token) -> bool {
    !matches!(t, Token::Whitespace(_) | Token::Comment(_) | Token::Eof)
}

fn token_text(t: &Token) -> &str {
    match t {
        Token::Identifier(s)
        | Token::Number(s)
        | Token::StringLiteral(s)
        | Token::Punctuator(s)
        | Token::Whitespace(s)
        | Token::Comment(s) => s,
        Token::Eof => "",
    }
}

/// `find_type_end`が返した終端インデックスの直前に連続する空白トークン
/// (`(number) => void`と`=`の間の空白のような、区切り記号の直前の
/// フォーマット用空白)は削らずに残す。そうしないと`x: number = 1`を
/// 削った結果が`x= 1`のように詰まってしまうため。
fn preserve_trailing_whitespace(tokens: &[Token], colon_index: usize, end: usize) -> usize {
    let mut keep_from = end;
    while keep_from > colon_index + 1 && matches!(tokens[keep_from - 1], Token::Whitespace(_)) {
        keep_from -= 1;
    }
    keep_from
}

/// `start`から走査し、`(`/`[`/`{`のネスト深さが0の位置で`stops`に
/// 含まれる記号に達したら、その記号のインデックス(まだ消費しない)を
/// 返す。見つからなければトークン列の末尾を返す。
fn find_type_end(tokens: &[Token], start: usize, stops: &[&str]) -> usize {
    let mut depth: i32 = 0;
    let mut i = start;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Punctuator(p) => {
                if depth == 0 && stops.contains(&p.as_str()) {
                    return i;
                }
                match p.as_str() {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" => depth -= 1,
                    _ => {}
                }
            }
            Token::Eof => return i,
            _ => {}
        }
        i += 1;
    }
    i
}

/// ソース文字列をトークナイズしたうえで型注釈を除去する。
pub fn transpile(source: &str) -> String {
    let mut sink = CollectingSink::default();
    tokenize(source, &mut sink);
    strip_type_annotations(&sink.tokens)
}

/// 既にトークナイズ済みの列から型注釈を除去してJS相当の文字列を作る
/// (`transpile`の下請け、テストや将来のパイプライン連携のために公開)。
pub fn strip_type_annotations(tokens: &[Token]) -> String {
    let mut out = String::new();
    let mut paren_depth: i32 = 0;
    // これまでに出力した「意味のある」(空白・コメントでない)トークンの
    // インデックス列。直近2件を見れば「let/const/varの直後」
    // 「(または,の直後」といった文脈判定ができる。
    let mut sig_history: Vec<usize> = Vec::new();
    let mut i = 0usize;

    while i < tokens.len() {
        let tok = &tokens[i];
        if matches!(tok, Token::Eof) {
            break;
        }
        if !is_significant(tok) {
            out.push_str(token_text(tok));
            i += 1;
            continue;
        }

        if let Token::Punctuator(p) = tok {
            if p == ":" {
                let prev = sig_history.last().map(|&idx| &tokens[idx]);
                let prev2 = if sig_history.len() >= 2 { Some(&tokens[sig_history[sig_history.len() - 2]]) } else { None };

                let is_var_decl = matches!(prev, Some(Token::Identifier(_)))
                    && matches!(prev2, Some(Token::Identifier(kw)) if matches!(kw.as_str(), "let" | "const" | "var"));
                let is_param = matches!(prev, Some(Token::Identifier(_)))
                    && paren_depth >= 1
                    && matches!(prev2, Some(Token::Punctuator(pp)) if pp == "(" || pp == ",");
                let is_return_type = matches!(prev, Some(Token::Punctuator(pp)) if pp == ")");

                if is_var_decl || is_param {
                    let end = find_type_end(tokens, i + 1, &["=", ";", ",", ")"]);
                    i = preserve_trailing_whitespace(tokens, i, end);
                    continue;
                } else if is_return_type {
                    let end = find_type_end(tokens, i + 1, &["{", "=>", ";", ","]);
                    let stopped_at_body_or_arrow =
                        matches!(tokens.get(end), Some(Token::Punctuator(sp)) if sp == "{" || sp == "=>");
                    if stopped_at_body_or_arrow {
                        i = preserve_trailing_whitespace(tokens, i, end);
                        continue;
                    }
                    // 戻り値型注釈と確信できないので(例: 三項演算子の
                    // `:`)、通常のトークンとして下へフォールスルーする。
                }
            } else if p == "(" {
                paren_depth += 1;
            } else if p == ")" {
                paren_depth -= 1;
            }
        }

        out.push_str(token_text(tok));
        sig_history.push(i);
        i += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_a_simple_variable_declaration_annotation() {
        assert_eq!(transpile("let x: number = 1;"), "let x = 1;");
    }

    #[test]
    fn strips_const_and_var_annotations_too() {
        assert_eq!(transpile("const x: string = \"a\";"), "const x = \"a\";");
        assert_eq!(transpile("var x: boolean = true;"), "var x = true;");
    }

    #[test]
    fn strips_function_parameter_annotations() {
        assert_eq!(
            transpile("function add(a: number, b: number) { return a + b; }"),
            "function add(a, b) { return a + b; }"
        );
    }

    #[test]
    fn strips_function_return_type_annotation() {
        assert_eq!(
            transpile("function add(a: number, b: number): number { return a + b; }"),
            "function add(a, b) { return a + b; }"
        );
    }

    #[test]
    fn strips_arrow_function_param_and_return_type_annotations() {
        assert_eq!(transpile("const f = (a: number): number => a + 1;"), "const f = (a) => a + 1;");
    }

    #[test]
    fn strips_array_and_function_type_annotations_by_tracking_bracket_depth() {
        assert_eq!(transpile("let xs: number[] = [1, 2];"), "let xs = [1, 2];");
        assert_eq!(transpile("let cb: (x: number) => void = f;"), "let cb = f;");
    }

    #[test]
    fn leaves_plain_javascript_untouched() {
        let js = "function add(a, b) { return a + b; }";
        assert_eq!(transpile(js), js);
    }

    #[test]
    fn does_not_strip_object_literal_or_ternary_colons() {
        // これらは型注釈ではない`:`なので、変更されずそのまま残る
        // べき(is_var_decl/is_param/is_return_typeのいずれの文脈にも
        // 一致しないことを確認)。
        assert_eq!(transpile("const o = { a: 1, b: 2 };"), "const o = { a: 1, b: 2 };");
        assert_eq!(transpile("const r = cond ? a : b;"), "const r = cond ? a : b;");
    }

    #[test]
    fn preserves_whitespace_and_comments() {
        let ts = "let x: number = 1; // keep me\n";
        assert_eq!(transpile(ts), "let x = 1; // keep me\n");
    }
}
