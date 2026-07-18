# 開発方針＆開発環境ルール(rtypescript)

作業ドライブは`F:\open-runo`。この節は[`open-raid-z`](https://github.com/aon-co-jp/open-raid-z)の`CLAUDE.md`を正本とし、各プロジェクトへコピーして同期する方針に準じる。

## このプロジェクトの構想

`RHTML5`/`RCSS3`/`RTypeScript`/`RBootStrap`という4プロジェクト構想の1つ。
詳細な全体構想は[`rhtml5`](https://github.com/aon-co-jp/RTHML)のCLAUDE.mdを参照(構想はrhtml5側に集約して記録)。

**スコープが非常に大きい(フルの型システム・型推論・構文解析)ため、
無理に全体実装を狙わず最小のスコープから立ち上げる方針**(ユーザー
指示、2026-07-18)。第一段は「単純な型注釈の削除・JSへの
トランスパイルのみ」に絞り、フル型チェックは対象外とする。

以前に検討された3つの実装方針案(A: 独自VM実行、B: Rustネイティブ
DSL+Wasm直接コンパイル、C: swcでASTのみ取り込み+独自インタプリタ)は
将来のフル実装フェーズでの選択肢として維持するが、第一段の
トークナイザ+型注釈除去はいずれの案にも先立つ土台であり、方針決定を
待たずに着手した。

## 現状(第一段、2026-07-18)

- `src/token.rs`: `Token`列挙型(`Identifier`/`Number`/`StringLiteral`/
  `Punctuator`/`Whitespace`/`Comment`/`Eof`)、`TokenSink`トレイト、
  テスト用`CollectingSink`。`rhtml5::token`と同じ疎結合パターンを踏襲
  (異なる点: 空白・コメントも独立トークンとして保持し、トランスパイル
  時にフォーマットを保存できるようにしている)。
- `src/tokenizer.rs`: 識別子・数値・文字列リテラル(`"`/`'`/`` ` ``、
  素朴なバックスラッシュエスケープ対応)・行/ブロックコメント・
  記号(多文字演算子は最長一致)・空白の字句解析器。
- `src/transpile.rs`: トークン列から型注釈を除去してJS相当の文字列を
  再構成する`transpile`/`strip_type_annotations`。対応: 変数宣言
  (`let`/`const`/`var`直後の識別子の`:`)、関数パラメータ(`(`/`,`直後の
  識別子の`:`)、関数の戻り値型注釈(`)`直後の`:`、終端が`{`または`=>`
  であることを確認してから確定するフォールバック付き)。型注釈の終端は
  `(`/`[`/`{`のネスト深さ追跡で判定(配列型・関数型のような入れ子を
  誤って途中で打ち切らないため)。オブジェクトリテラルの`key: value`・
  三項演算子の`cond ? a : b`は型注釈の文脈に一致しないため保持される
  ことをテストで確認済み。
- **未対応(次段階)**: `interface`/`type`宣言、ジェネリクス制約、複数
  変数宣言子の2つ目以降(`let x: T = 1, y: U = 2;`のyの型は残ってしまう)、
  Wasmへの直接コンパイル、swc AST取り込み。
- **検証**: `cargo test`で14件全green(トークナイザ5件+トランスパイル
  9件、単純な変数宣言・関数パラメータ・戻り値型・アロー関数・配列型/
  関数型・非型注釈コロンの非破壊・空白コメント保存を含む)。警告0件。

## 次にすべきこと

1. 複数変数宣言子対応(`let x: T = 1, y: U = 2;`)
2. `interface`/`type`宣言の扱い(第一段では無視して素通りさせている
   だけなので、パース時にエラーにするか読み飛ばすか方針を決める)
3. 実装方針の最終決定(B案「Rustネイティブdsl+直接Wasmコンパイル」
   から着手しC案「swc AST+独自インタプリタ」へ拡張、が暫定方針)

## 関連プロジェクト

- [rhtml5](https://github.com/aon-co-jp/RTHML) / [rcss3](https://github.com/aon-co-jp/RCSS) / [rreact](https://github.com/aon-co-jp/RReact) — 全体構想の詳細、相互統合の現状はこちら側に記録
- [open-raid-z](https://github.com/aon-co-jp/open-raid-z) — 開発ルールの正本

## HANDOFF

- **2026-07-18 リポジトリ新設・最小スコープでの立ち上げ**: 完全に
  空だったリポジトリに、`Cargo.toml`と`token`/`tokenizer`/`transpile`
  の3モジュールを新規実装。フル型システムを狙わず「単純な型注釈の
  削除・JSへのトランスパイルのみ」というユーザー指示のスコープに
  絞った。`cargo test`で14件全green、警告0件を確認。`git init`済み、
  GitHub側`aon-co-jp/RTypeScript`リポジトリは既に存在していたため
  originとして紐付けてpush予定。
  次にすべきこと: 複数変数宣言子対応、`interface`/`type`宣言の扱いの
  方針決定、Wasmコンパイル方針(B/C案)の最終決定。
