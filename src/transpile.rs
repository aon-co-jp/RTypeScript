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

use crate::classes::strip_class_syntax;
use crate::enums::strip_enums;
use crate::expressions::strip_expression_type_syntax;
use crate::generics::strip_generics;
use crate::interfaces::strip_interfaces_and_type_aliases;
use crate::token::{CollectingSink, Token};
use crate::tokenizer::tokenize;

pub(crate) fn is_significant(t: &Token) -> bool {
    !matches!(t, Token::Whitespace(_) | Token::Comment(_) | Token::Raw(_) | Token::Eof)
}

pub(crate) fn token_text(t: &Token) -> &str {
    match t {
        Token::Identifier(s)
        | Token::Number(s)
        | Token::StringLiteral(s)
        | Token::Punctuator(s)
        | Token::Whitespace(s)
        | Token::Comment(s)
        | Token::Raw(s) => s,
        Token::Eof => "",
    }
}

/// クラス構文モジュール等、可視性修飾子・パラメータプロパティを
/// 判定する際に使う予約語チェック(複数箇所から参照するため公開)。
pub(crate) fn is_modifier_keyword(s: &str) -> bool {
    matches!(s, "public" | "private" | "protected" | "readonly" | "abstract" | "override")
}

/// `start`(含む)以降で最初に見つかる意味のあるトークンのインデックスを返す
/// (`interfaces`/`generics`/`classes`/`expressions`の各パスから共有する)。
pub(crate) fn next_sig_index(tokens: &[Token], start: usize) -> Option<usize> {
    (start..tokens.len()).find(|&i| is_significant(&tokens[i]))
}

/// `tokens[lt]`が`<`である前提で、対応する`>`のインデックスを返す
/// (ジェネリクスの入れ子深さを追跡するだけの単純な実装。`>>`のような
/// 複数文字トークンはトークナイザ側で単一トークン化しないため、
/// この単純な深さ追跡で`Foo<Bar<Baz>>`のような入れ子も正しく扱える)。
pub(crate) fn consume_angle_generic(tokens: &[Token], lt: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = lt;
    loop {
        match tokens.get(i) {
            None => return None,
            Some(Token::Eof) => return None,
            Some(Token::Punctuator(p)) if p == "<" => {
                depth += 1;
                i += 1;
            }
            Some(Token::Punctuator(p)) if p == ">" => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
}

/// `tokens[open]`が開き括弧(`{`/`(`/`[`)である前提で、対応する
/// 閉じ括弧のインデックスを返す。
pub(crate) fn consume_matching_bracket(tokens: &[Token], open: usize, open_ch: &str, close_ch: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = open;
    loop {
        match tokens.get(i) {
            None => return None,
            Some(Token::Eof) => return None,
            Some(Token::Punctuator(p)) if p == open_ch => {
                depth += 1;
                i += 1;
            }
            Some(Token::Punctuator(p)) if p == close_ch => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
}

/// `find_type_end`が返した終端インデックスの直前に連続する空白トークン
/// (`(number) => void`と`=`の間の空白のような、区切り記号の直前の
/// フォーマット用空白)は削らずに残す。そうしないと`x: number = 1`を
/// 削った結果が`x= 1`のように詰まってしまうため。
pub(crate) fn preserve_trailing_whitespace(tokens: &[Token], colon_index: usize, end: usize) -> usize {
    let mut keep_from = end;
    while keep_from > colon_index + 1 && matches!(tokens[keep_from - 1], Token::Whitespace(_)) {
        keep_from -= 1;
    }
    keep_from
}

/// `start`から走査し、`(`/`[`/`{`のネスト深さが0の位置で`stops`に
/// 含まれる記号に達したら、その記号のインデックス(まだ消費しない)を
/// 返す。見つからなければトークン列の末尾を返す。
pub(crate) fn find_type_end(tokens: &[Token], start: usize, stops: &[&str]) -> usize {
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
///
/// パイプライン(各段はトークン列→トークン列の変換、最終段のみ
/// トークン列→文字列):
/// 1. `interface`/`type`宣言の全体除去(JSランタイム表現を持たないため)
/// 2. `enum`宣言の展開(実行時表現を持つため、tsc相当の数値enum
///    (前方+逆引きマッピング付きIIFE)へ展開。文字列enumは逆引きなし)
/// 3. クラス構文(可視性修飾子・コンストラクタのパラメータプロパティの
///    `this.x = x`展開・クラスフィールドの型注釈/オプショナル`?`/
///    確定代入`!`除去)
/// 4. ジェネリクス型パラメータ(`function f<T>`・`class C<T>`・
///    呼び出し側`foo<string>(...)`の`<...>`)の除去
/// 5. 式レベルのTS専用構文(非null表明`x!`・型アサーション`x as T`)の除去
///    (`import { a as b }`のリネーム構文とは区別する)
/// 6. 変数宣言・関数パラメータ・戻り値型注釈(オプショナル`?`markerも
///    含む)の除去、および最終的な文字列への再構成
pub fn transpile(source: &str) -> String {
    let mut sink = CollectingSink::default();
    tokenize(source, &mut sink);
    let tokens = sink.tokens;
    let tokens = strip_interfaces_and_type_aliases(&tokens);
    let tokens = strip_enums(&tokens);
    let tokens = strip_class_syntax(&tokens);
    let tokens = strip_generics(&tokens);
    let tokens = strip_expression_type_syntax(&tokens);
    strip_type_annotations(&tokens)
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
            if p == "?" {
                // オプショナルパラメータの`?`(`function f(a?: number)`・
                // `function f(a?)`)。**次の意味のあるトークンが`:` /
                // `,` / `)` / `=` のいずれかである場合に限り**除去する
                // ——この即時後続チェックが、三項演算子`cond ? a : b`
                // (`?`の直後に式が続く)との誤判定を防ぐ決め手になる
                // (三項演算子なら`?`の直後は識別子等の式トークンであり、
                // 上記の4記号のいずれにも一致しない)。
                let prev = sig_history.last().map(|&idx| &tokens[idx]);
                let prev2 = if sig_history.len() >= 2 { Some(&tokens[sig_history[sig_history.len() - 2]]) } else { None };
                let is_param_position = matches!(prev, Some(Token::Identifier(_)))
                    && paren_depth >= 1
                    && matches!(prev2, Some(Token::Punctuator(pp)) if pp == "(" || pp == ",");

                if is_param_position {
                    let next_sig = tokens[i + 1..].iter().find(|t| is_significant(t));
                    let looks_like_optional_marker =
                        matches!(next_sig, Some(Token::Punctuator(np)) if matches!(np.as_str(), ":" | "," | ")" | "="));
                    if looks_like_optional_marker {
                        i += 1;
                        continue;
                    }
                }
            } else if p == ":" {
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
