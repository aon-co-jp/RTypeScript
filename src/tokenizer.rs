//! TypeScript(サブセット)向けの単純な字句解析器。WHATWG HTML相当の
//! 厳密な状態機械ではなく、実用上必要な部分(識別子・数値・文字列・
//! 記号・空白・コメント)だけを素直な逐次スキャンで切り出す
//! (`rhtml5::tokenizer`と同じく「完全な仕様準拠ではなく実用的な
//! サブセット」という設計方針)。

use crate::token::{Token, TokenSink};

/// 長い記号から順に試すことで、`===`が`==`+`=`のように分割されない
/// ようにする(最長一致)。
const MULTI_CHAR_PUNCTUATORS: &[&str] =
    &["=>", "===", "!==", "==", "!=", "<=", ">=", "&&", "||", "??", "?.", "...", "+=", "-=", "*=", "/="];

pub struct Tokenizer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    rest: &'a str,
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { chars: input.chars().peekable(), rest: input, pos: 0 }
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.next()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn remaining(&self) -> &'a str {
        &self.rest[self.pos..]
    }

    fn run<F: Fn(char) -> bool>(&mut self, pred: F) -> String {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if pred(c) {
                out.push(c);
                self.advance();
            } else {
                break;
            }
        }
        out
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn is_ident_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

pub fn tokenize<S: TokenSink>(input: &str, sink: &mut S) {
    let mut tokenizer = Tokenizer::new(input);

    loop {
        let Some(c) = tokenizer.peek() else {
            sink.process_token(Token::Eof);
            break;
        };

        if c.is_whitespace() {
            let ws = tokenizer.run(|c| c.is_whitespace());
            sink.process_token(Token::Whitespace(ws));
            continue;
        }

        // 行コメント。
        if c == '/' && tokenizer.remaining().starts_with("//") {
            let mut text = String::new();
            while let Some(c) = tokenizer.peek() {
                if c == '\n' {
                    break;
                }
                text.push(c);
                tokenizer.advance();
            }
            sink.process_token(Token::Comment(text));
            continue;
        }

        // ブロックコメント。
        if c == '/' && tokenizer.remaining().starts_with("/*") {
            let mut text = String::new();
            text.push_str("/*");
            tokenizer.advance();
            tokenizer.advance();
            loop {
                match tokenizer.peek() {
                    None => break,
                    Some('*') if tokenizer.remaining().starts_with("*/") => {
                        text.push_str("*/");
                        tokenizer.advance();
                        tokenizer.advance();
                        break;
                    }
                    Some(c) => {
                        text.push(c);
                        tokenizer.advance();
                    }
                }
            }
            sink.process_token(Token::Comment(text));
            continue;
        }

        // 文字列リテラル(`"`/`'`/`` ` ``)。バックスラッシュエスケープの
        // 直後の1文字は区切り文字として扱わない(素朴なエスケープ対応)。
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            let mut text = String::new();
            text.push(quote);
            tokenizer.advance();
            loop {
                match tokenizer.advance() {
                    None => break,
                    Some('\\') => {
                        text.push('\\');
                        if let Some(escaped) = tokenizer.advance() {
                            text.push(escaped);
                        }
                    }
                    Some(c) if c == quote => {
                        text.push(c);
                        break;
                    }
                    Some(c) => text.push(c),
                }
            }
            sink.process_token(Token::StringLiteral(text));
            continue;
        }

        if c.is_ascii_digit() {
            let mut num = tokenizer.run(|c| c.is_ascii_digit());
            if tokenizer.peek() == Some('.') {
                num.push('.');
                tokenizer.advance();
                num.push_str(&tokenizer.run(|c| c.is_ascii_digit()));
            }
            sink.process_token(Token::Number(num));
            continue;
        }

        if is_ident_start(c) {
            let ident = tokenizer.run(is_ident_continue);
            sink.process_token(Token::Identifier(ident));
            continue;
        }

        // 記号(最長一致で多文字演算子を優先)。
        let remaining = tokenizer.remaining();
        if let Some(op) = MULTI_CHAR_PUNCTUATORS.iter().find(|op| remaining.starts_with(*op)) {
            for _ in 0..op.chars().count() {
                tokenizer.advance();
            }
            sink.process_token(Token::Punctuator(op.to_string()));
            continue;
        }

        tokenizer.advance();
        sink.process_token(Token::Punctuator(c.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::CollectingSink;

    fn tokenize_all(input: &str) -> Vec<Token> {
        let mut sink = CollectingSink::default();
        tokenize(input, &mut sink);
        sink.tokens
    }

    #[test]
    fn tokenizes_identifiers_and_whitespace() {
        let tokens = tokenize_all("let x");
        assert_eq!(
            tokens,
            vec![
                Token::Identifier("let".to_string()),
                Token::Whitespace(" ".to_string()),
                Token::Identifier("x".to_string()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_number_and_string_literals() {
        let tokens = tokenize_all(r#"1.5 "hi""#);
        assert_eq!(
            tokens,
            vec![
                Token::Number("1.5".to_string()),
                Token::Whitespace(" ".to_string()),
                Token::StringLiteral("\"hi\"".to_string()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn multi_char_punctuators_take_priority_over_single_char() {
        let tokens = tokenize_all("a => b === c");
        assert_eq!(
            tokens,
            vec![
                Token::Identifier("a".to_string()),
                Token::Whitespace(" ".to_string()),
                Token::Punctuator("=>".to_string()),
                Token::Whitespace(" ".to_string()),
                Token::Identifier("b".to_string()),
                Token::Whitespace(" ".to_string()),
                Token::Punctuator("===".to_string()),
                Token::Whitespace(" ".to_string()),
                Token::Identifier("c".to_string()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn line_and_block_comments_are_captured_verbatim() {
        let tokens = tokenize_all("// hi\n/* block */");
        assert_eq!(
            tokens,
            vec![
                Token::Comment("// hi".to_string()),
                Token::Whitespace("\n".to_string()),
                Token::Comment("/* block */".to_string()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn type_annotation_colon_is_tokenized_as_a_plain_punctuator() {
        // トークナイザ自体は型注釈を特別扱いしない(その除去は
        // transpileモジュールの責務)。
        let tokens = tokenize_all("let x: number = 1;");
        assert_eq!(
            tokens,
            vec![
                Token::Identifier("let".to_string()),
                Token::Whitespace(" ".to_string()),
                Token::Identifier("x".to_string()),
                Token::Punctuator(":".to_string()),
                Token::Whitespace(" ".to_string()),
                Token::Identifier("number".to_string()),
                Token::Whitespace(" ".to_string()),
                Token::Punctuator("=".to_string()),
                Token::Whitespace(" ".to_string()),
                Token::Number("1".to_string()),
                Token::Punctuator(";".to_string()),
                Token::Eof,
            ]
        );
    }
}
