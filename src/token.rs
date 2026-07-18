//! TypeScript(のごく一部の構文サブセット)向けトークン列挙型。
//! `rhtml5`の`Token`/`TokenSink`パターン(トークナイザとその後段の
//! 処理を疎結合にする設計)を踏襲する。ただし本クレートでは第一段の
//! スコープ(単純な型注釈の削除・JSへのトランスパイルのみ)のため、
//! 後段はDOM木構築器ではなく`transpile`モジュールの文字列生成器になる。
//!
//! 出力(JSへのトランスパイル)を素直な文字列再構成で行うため、
//! 空白・改行も`Whitespace`トークンとして保持し、フォーマットを
//! できるだけ保存する(html5everの`Characters`相当の考え方を空白にも
//! 適用したもの)。

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// 識別子・予約語(`let`/`const`/`function`等も区別せずここに
    /// 含める、第一段では予約語を特別扱いする必要がないため)。
    Identifier(String),
    Number(String),
    /// 引用符を含む生のソーステキスト(`"a"`・`'a'`・`` `a` ``)。
    /// 内部のエスケープは解釈せず、開始〜終了引用符までをそのまま
    /// 保持する(第一段のスコープ外)。
    StringLiteral(String),
    /// 記号(1文字または`=>`・`==`・`===`等の多文字演算子)。
    Punctuator(String),
    /// 空白・改行・タブの連続(フォーマット保存のため生テキストのまま
    /// 保持する)。
    Whitespace(String),
    /// `//`行コメント・`/* */`ブロックコメント(生テキストのまま保持)。
    Comment(String),
    Eof,
}

/// トークナイザが生成した各`Token`を受け取る側のトレイト
/// (`rhtml5::TokenSink`と同じ疎結合パターン)。
pub trait TokenSink {
    fn process_token(&mut self, token: Token);
}

/// テスト・デバッグ用途の単純な`TokenSink`実装。
#[derive(Default)]
pub struct CollectingSink {
    pub tokens: Vec<Token>,
}

impl TokenSink for CollectingSink {
    fn process_token(&mut self, token: Token) {
        self.tokens.push(token);
    }
}
