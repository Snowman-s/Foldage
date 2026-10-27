# Foldage

木構造そのものがプログラムで、実行とは木を葉から根へ書き換えて「畳む」ことである難解プログラミング言語。根が単一のアトムになったら、それが出力になる。

書き換えのルールも木の一部なので、プログラムは最後に自分のルールを畳み込んで消さないと終われない。自分を書き換えることが、飾りではなく終了の条件になっている。

## 例

2 + 1 をペアノ数で計算し、結果を棒の本数で出力する。

```
main
  -> (add z ?n) ?n
  -> (add (s ?m) ?n) (s: add ?m ?n)
  -> (show z) ""
  -> (show: s ?n) (++ "|": show ?n)
  -> (main ?..rules ?r) ?r    # ルール群（自分自身を含む）ごと main を畳む
  show: add (s: s z) (s z)
```

出力は `|||`。どのルールがどこに当たって畳まれていくかは[動作原理（図解）](how-it-works.html#stepper)の「サンプルを 1 ステップずつ」で、例1 を 1 ステップずつ動かして見られる。

## ドキュメント

初めて読むなら上から順に。

| 文書 | 内容 |
|---|---|
| [動作原理（図解）](how-it-works.html) | 木の材料、変数、1 ステップの進み方、自己書き換えを図で説明する。例1〜例4 を、どのルールがどこに当たったかとともに 1 ステップずつ動かして見られる |
| [言語仕様](spec.md) | 実行モデルと構文の定義。処理系はこの文書に従う |
| [チューリング完全性](turing-completeness.md) | 任意の 2-タグシステムを Foldage に翻訳できることの証明と、Collatz タグシステムの例 |

## サンプル

| ファイル | 内容 | 解説 |
|---|---|---|
| [ex1-add.fdg](https://github.com/Snowman-s/foldage/blob/main/samples/ex1-add.fdg) | 足し算の結果を棒の本数で出力する | [ステップ実行](how-it-works.html#stepper) |
| [ex2-meta.fdg](https://github.com/Snowman-s/foldage/blob/main/samples/ex2-meta.fdg) | ルールが別のルールを書き換える | [ステップ実行](how-it-works.html#stepper) |
| [ex3-mkid.fdg](https://github.com/Snowman-s/foldage/blob/main/samples/ex3-mkid.fdg) | ルールを生成するルール | [ステップ実行](how-it-works.html#stepper) |
| [ex4-rev.fdg](https://github.com/Snowman-s/foldage/blob/main/samples/ex4-rev.fdg) | 文字列の並びを逆順にする | [ステップ実行](how-it-works.html#stepper) |
| [ex5-tag.fdg](https://github.com/Snowman-s/foldage/blob/main/samples/ex5-tag.fdg) | 2-タグシステム（Collatz） | [チューリング完全性](turing-completeness.md) |
| [ex6-fizzbuzz.fdg](https://github.com/Snowman-s/foldage/blob/main/samples/ex6-fizzbuzz.fdg) | 1〜15 の FizzBuzz | ソース中のコメント |

## 処理系

Rust で書かれている（外部依存なし）。

```
git clone https://github.com/Snowman-s/foldage.git
cd foldage
cargo run -- samples/ex1-add.fdg            # ||| を出力
```

オプション・終了コードは [README](https://github.com/Snowman-s/foldage#処理系) を参照。
