# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 概要

Foldage: 木構造そのものをプログラムとし、木を葉から根へ書き換えて「畳む」難解プログラミング言語の Rust 処理系（外部依存なし）。ルールも木の一部なので、プログラムは最後に自分のルールを畳み込んで消さないと終われない。設計方針は「必要性のある難解さのみを入れる」。

## コマンド

```
cargo build
cargo test                                   # 全テスト
cargo test --test samples                    # samples/ の結合テストだけ
cargo test --test samples ex1_add            # 結合テストを 1 本
cargo test engine::tests::nearest_scope_wins # 単体テストを 1 本
cargo run -- samples/ex1-add.fdg             # ||| を出力（末尾改行なし）
cargo run -- <file> --trace                  # 各ステップの木を正規形で標準エラーに表示
cargo run -- <file> --max-steps 1000
```

終了コード: 0 正常終了 / 1 折りたたみ失敗・ステップ上限 / 2 使い方・読み込み・構文エラー。

## 仕様の所在

- **[docs/spec.md](docs/spec.md) が言語仕様の正**。ソースの doc コメントは「spec.md §5.3」のように節番号で参照している。意味論を変えるときはこの文書を先に更新する
- [docs/how-it-works.html](docs/how-it-works.html): 図解。ex1〜ex4 のステップごとの遷移（位置・適用ルール・束縛）をスクリプト内のデータとして持つ。[docs/turing-completeness.md](docs/turing-completeness.md): 2-タグシステムからの翻訳規則 T と ex5
- [tests/samples.rs](tests/samples.rs) は資料に書いたステップ数・途中の木と一致することを検査する。書き換え戦略やルール判定を変えるとステップ数が変わり、資料とテストの両方を直す必要がある
- [tests/tag_system.rs](tests/tag_system.rs) は翻訳規則 T で生成したプログラムを、タグシステムの直接シミュレーションと突き合わせる

## アーキテクチャ

処理の流れ: `parser` → `tree::Node` → `engine::run`（毎ステップ `find_redex` → `replace_at`）→ `printer`。

- [src/tree.rs](src/tree.rs): `Node`（ラベル `Token` + 子の列）。`Token` はシンボル・文字列・変数 `?x`/`??x`・列変数 `?..xs` で、変数のレベルは `?` の個数。変数もただのトークンとして木に置かれ、変数として解釈されるのはルールのパターン/置換に使われるときだけ
- [src/rule.rs](src/rule.rs): `as_rule` が「ラベル `->`・子 2 つ・置換のレベル 1 変数がすべてパターンに現れる」ノードをルールと判定する。メタ書き換えでルールとデータが入れ替わるため、ルールのキャッシュは持たず毎ステップ判定し直す
- [src/matcher.rs](src/matcher.rs): 継続渡しのバックトラッキング照合。列変数は左から最短一致、非線形マッチあり。`instantiate` はレベル 2 以上の変数の `?` を 1 つ減らす（ルールを生成するルールのため）。組み立てられない置換はそのルールが「当たらなかった」扱いになり、次のルールを試す
- [src/engine.rs](src/engine.rs): 最左最内（帰りがけ順）で位置を探し、1 ステップに 1 箇所だけ書き換える。各位置では近いスコープ（自身の子のルール → 親の子 → … → 根の子）から、同スコープ内は左から試し、最後に組込みの `++`（文字列 2 つの連結。根の外側スコープにあるルールとみなす）を試す。ルールのノードは内部に入らず 1 つの位置として扱う（ノード全体はメタルールのマッチ対象になる）
- [src/printer.rs](src/printer.rs): 正規形（spec.md 第 7 章）で書き出す。出力はそのままパース可能で、`--trace` の各ステップは実行可能なソースになる
- [src/parser.rs](src/parser.rs): インデント（スペース 2 個固定。タブ・CR・U+FEFF は文字列リテラルとコメントの中でだけ使える）+ 行内ネスト `:`（右結合）+ 行内の部分木 `()`（同じ行で閉じる糖衣構文。正規形では使わない）。`#` は字句の先頭でのみコメントになる（`b#c` は 1 つのシンボル）

## 深い木への対策（変更時に壊さないこと）

ペアノ数などで木は 10 万段規模に深くなる。そのため、対象の木の深さに比例する再帰を書かない。

- `Node` の `Clone`・`PartialEq`・`Drop` は明示的スタックで手書きしている（derive に戻さない）。`Debug` だけは derive なので深い木の表示は溢れうる
- parser の組み立て、printer、engine の探索、matcher の照合と置換の組み立て、rule の変数集めも明示的スタックで実装されている
- パターン・置換もメタ書き換えで対象の木と同じくらい深くなり、照合は兄弟の数だけ積み重なりうるので、matcher でも再帰しない。そのため `main` はスタックを広げず既定のメインスレッドで動く
- 深い木のテスト（`deep_trees_*`、`deep_tree_runs_without_overflow`）がこれを検査する

## 規約

- doc コメント・コメント・エラーメッセージは日本語、識別子とテスト関数名は英語
- 途中経過（トレース・エラー）は標準エラー、結果のアトムだけを標準出力に書く
