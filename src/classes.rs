//! クラス構文(可視性修飾子・コンストラクタのパラメータプロパティ・
//! クラスフィールドの型注釈)を扱うパス。
//!
//! 対応する内容:
//! - `public`/`private`/`protected`/`readonly`/`abstract`/`override`
//!   修飾子の除去(クラスメンバー宣言・コンストラクタパラメータの
//!   いずれの位置でも)。`static`はJSの正式な構文なので対象外
//!   (除去しない)。
//! - クラスフィールド宣言の型注釈・オプショナル`?`・確定代入表明`!`
//!   の除去(`private x: number;` → `x;`、`y?: string;` → `y;`、
//!   `z!: number;` → `z;`)。
//! - **コンストラクタのパラメータプロパティの展開**(最も間違えやすい
//!   ポイント——TypeScriptの`constructor(public y: string)`は単なる
//!   型注釈の除去では済まず、`this.y = y;`という**実行時に意味のある
//!   代入文**を生成しなければ正しいJSにならない)。
//!   `constructor(public y: string, private z: number = 0)`は
//!   `constructor(y, z = 0) { this.y = y; this.z = z; ...元の本体... }`
//!   のように、モディファイア付きパラメータの名前を記録しておき、
//!   コンストラクタ本体の開き波括弧の直後に代入文を注入する。
//!   (モディファイアなしの通常パラメータは対象外——プロパティ化
//!   されない、これは`tsc`の実際の仕様通り)。
//!
//! 型注釈自体の除去(`(a: number)`のような通常のパラメータ・戻り値型)
//! は、モディファイアを除去した後の後続パス(`transpile::
//! strip_type_annotations`)に委ねる(モディファイアさえ除去して
//! しまえば、パラメータの直前トークンが`(`/`,`になるため、既存の
//! ヒューリスティックがそのまま働く)。

use crate::token::Token;
use crate::transpile::{find_type_end, is_modifier_keyword, is_significant, preserve_trailing_whitespace};

struct ParenCtx {
    is_constructor: bool,
    props: Vec<String>,
}

fn last_significant(out: &[Token]) -> Option<&Token> {
    out.iter().rev().find(|t| is_significant(t))
}

fn is_stop_before_marker(t: &Token) -> bool {
    matches!(t, Token::Punctuator(p) if matches!(p.as_str(), ":" | ";" | "=" | "}"))
}

pub(crate) fn strip_class_syntax(tokens: &[Token]) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut paren_ctx_stack: Vec<ParenCtx> = Vec::new();
    let mut class_member_depths: Vec<i32> = Vec::new();
    let mut brace_depth: i32 = 0;
    let mut awaiting_class_brace = false;
    let mut pending_ctor_injection: Option<Vec<String>> = None;
    let mut pending_property_marker = false;
    let mut i = 0usize;

    while i < tokens.len() {
        let tok = &tokens[i];
        if matches!(tok, Token::Eof) {
            out.push(tok.clone());
            break;
        }
        if !is_significant(tok) {
            out.push(tok.clone());
            i += 1;
            continue;
        }

        let is_member_level = class_member_depths.last() == Some(&brace_depth) && paren_ctx_stack.is_empty();

        if let Token::Identifier(name) = tok {
            if name == "class" {
                awaiting_class_brace = true;
                out.push(tok.clone());
                i += 1;
                continue;
            }

            if is_modifier_keyword(name) {
                let prev_sig = last_significant(&out);
                let in_param_start = !paren_ctx_stack.is_empty()
                    && matches!(prev_sig, Some(Token::Punctuator(p)) if p == "(" || p == ",");
                let in_member_start = is_member_level
                    && (prev_sig.is_none()
                        || matches!(prev_sig, Some(Token::Punctuator(p)) if matches!(p.as_str(), ";" | "{" | "}")));

                if in_param_start {
                    if paren_ctx_stack.last().map(|c| c.is_constructor).unwrap_or(false) {
                        pending_property_marker = true;
                    }
                    // モディファイア直後の空白1個も合わせて読み飛ばす
                    // (`public x`の`public`だけ除いて空白を残すと、
                    // 直前のカンマ後の空白と合わさって`,  y`のように
                    // 空白が二重になってしまうため)。
                    i += 1;
                    if matches!(tokens.get(i), Some(Token::Whitespace(_))) {
                        i += 1;
                    }
                    continue;
                } else if in_member_start {
                    i += 1;
                    if matches!(tokens.get(i), Some(Token::Whitespace(_))) {
                        i += 1;
                    }
                    continue;
                }
            }

            if pending_property_marker {
                // モディファイア(1つ以上)の直後に来た識別子 = パラメータ
                // プロパティのパラメータ名。
                if let Some(ctx) = paren_ctx_stack.last_mut() {
                    ctx.props.push(name.clone());
                }
                pending_property_marker = false;
            }
        }

        if let Token::Punctuator(p) = tok {
            match p.as_str() {
                "(" => {
                    let prev_sig = last_significant(&out);
                    let is_ctor = matches!(prev_sig, Some(Token::Identifier(n)) if n == "constructor");
                    paren_ctx_stack.push(ParenCtx { is_constructor: is_ctor, props: Vec::new() });
                }
                ")" => {
                    if let Some(ctx) = paren_ctx_stack.pop() {
                        if ctx.is_constructor && !ctx.props.is_empty() {
                            pending_ctor_injection = Some(ctx.props);
                        }
                    }
                }
                "{" => {
                    let new_depth = brace_depth + 1;
                    if awaiting_class_brace {
                        class_member_depths.push(new_depth);
                        awaiting_class_brace = false;
                    }
                    brace_depth = new_depth;
                    out.push(tok.clone());
                    if let Some(props) = pending_ctor_injection.take() {
                        let injected: String = props.iter().map(|p| format!(" this.{p} = {p};")).collect();
                        out.push(Token::Raw(injected));
                    }
                    i += 1;
                    continue;
                }
                "}" => {
                    if class_member_depths.last() == Some(&brace_depth) {
                        class_member_depths.pop();
                    }
                    brace_depth -= 1;
                }
                "?" | "!" if is_member_level && matches!(last_significant(&out), Some(Token::Identifier(_))) => {
                    let next_sig = tokens[i + 1..].iter().find(|t| is_significant(t));
                    if matches!(next_sig, Some(t) if is_stop_before_marker(t)) {
                        i += 1;
                        continue;
                    }
                }
                ":" if is_member_level && matches!(last_significant(&out), Some(Token::Identifier(_))) => {
                    let end = find_type_end(tokens, i + 1, &["=", ";", "}"]);
                    i = preserve_trailing_whitespace(tokens, i, end);
                    continue;
                }
                _ => {}
            }
        }

        out.push(tok.clone());
        i += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use crate::transpile::transpile;

    #[test]
    fn strips_visibility_modifiers_from_a_constructor_parameter_without_property_shorthand() {
        // モディファイアなしの通常パラメータ(型注釈のみ)。
        assert_eq!(
            transpile("class Point {\n  constructor(x: number, y: number) {\n    this.sum = x + y;\n  }\n}"),
            "class Point {\n  constructor(x, y) {\n    this.sum = x + y;\n  }\n}"
        );
    }

    #[test]
    fn expands_constructor_parameter_properties_into_this_assignments() {
        // これがこのクレートで最も間違えやすいケース: `public`/`private`は
        // 単なる型レベルの飾りではなく、TypeScriptコンパイラが実際に
        // `this.y = y;`という代入文を生成する実行時挙動を持つ。
        let ts = "class Point {\n  constructor(public x: number, private y: string) {\n    console.log(x);\n  }\n}";
        let js = transpile(ts);
        assert_eq!(
            js,
            "class Point {\n  constructor(x, y) { this.x = x; this.y = y;\n    console.log(x);\n  }\n}"
        );
    }

    #[test]
    fn expands_a_readonly_only_parameter_property() {
        assert_eq!(
            transpile("class C {\n  constructor(readonly id: string) {}\n}"),
            "class C {\n  constructor(id) { this.id = id;}\n}"
        );
    }

    #[test]
    fn mixes_parameter_properties_with_plain_parameters() {
        let ts = "class C {\n  constructor(public a: number, b: number) {\n    this.b = b;\n  }\n}";
        assert_eq!(
            transpile(ts),
            "class C {\n  constructor(a, b) { this.a = a;\n    this.b = b;\n  }\n}"
        );
    }

    #[test]
    fn strips_class_field_modifiers_and_type_annotations() {
        let ts = "class Foo {\n  private x: number;\n  public y: string = \"a\";\n  readonly z: boolean = true;\n}";
        assert_eq!(transpile(ts), "class Foo {\n  x;\n  y = \"a\";\n  z = true;\n}");
    }

    #[test]
    fn strips_optional_and_definite_assignment_markers_on_class_fields() {
        assert_eq!(
            transpile("class Foo {\n  x?: number;\n  y!: string;\n}"),
            "class Foo {\n  x;\n  y;\n}"
        );
    }

    #[test]
    fn leaves_static_modifier_untouched_since_it_is_real_javascript() {
        assert_eq!(transpile("class Foo {\n  static count = 0;\n}"), "class Foo {\n  static count = 0;\n}");
    }

    #[test]
    fn does_not_touch_method_bodies_that_merely_contain_colons() {
        // メソッド本体内のラベル付き文・三項演算子・オブジェクトリテラルの
        // `:`は、クラスメンバーレベルの型注釈と誤認しない
        // (`is_member_level`が`brace_depth`をメソッド本体の深さと
        // 区別しているため)。
        let ts = "class Foo {\n  bar() {\n    const o = { a: 1 };\n    return true ? 1 : 2;\n  }\n}";
        assert_eq!(transpile(ts), ts);
    }
}
