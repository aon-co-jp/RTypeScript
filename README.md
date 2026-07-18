# rtypescript

TypeScript相当を一から開発するプロジェクト。`RHTML5`/`RCSS3`/`RTypeScript`/`RBootStrap`構想の一部。

スコープが非常に大きいため、最初の一歩として**単純な型注釈の削除・JSへのトランスパイルのみ**から立ち上げる(フル型チェックは対象外)。

## 使用例

```rust
use rtypescript::transpile;

let js = transpile("function add(a: number, b: number): number { return a + b; }");
assert_eq!(js, "function add(a, b) { return a + b; }");
```

## 対応範囲(第一段)

- 変数宣言の型注釈(`let x: number = 1;` → `let x = 1;`)
- 関数パラメータの型注釈
- 関数の戻り値型注釈(ブロック本体・アロー関数どちらも)
- 配列型・関数型など、括弧のネストを伴う型注釈

## 未対応(次段階)

`interface`/`type`宣言、ジェネリクス制約、複数変数宣言子の2つ目以降、Wasmへの直接コンパイル。

## ビルド・テスト

```bash
cargo test
```

## ライセンス

Apache-2.0 OR MIT
