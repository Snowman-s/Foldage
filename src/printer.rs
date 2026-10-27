//! 木を正規形（spec.md 第 7 章）のソースとして書き出す。
//! 書き出したものはそのまま読み込めるので、トレースをソースのまま見せられる。

use std::collections::HashSet;

use crate::tree::{Node, Token};

/// 木を正規形のソースにする。各行は改行で終わる。
/// 先頭から続く葉の子は行内に、残りの子は 1 段深い子ブロックに書く。1 行で書けるノードは 1 行にまとめる。
/// 深い木でもスタックを使わないよう、書き出す行を明示的なスタックで順に取り出す。
pub fn to_source(root: &Node) -> String {
    let fits = one_line_nodes(root);
    let mut out = String::new();
    let mut stack = vec![(root, 0)];
    while let Some((node, depth)) = stack.pop() {
        out.push_str(&"  ".repeat(depth));
        if fits.contains(&(node as *const Node)) {
            out.push_str(&one_line(node));
            out.push('\n');
            continue;
        }
        let inline = node.children.iter().take_while(|c| c.is_leaf()).count();
        let mut words = vec![token_text(&node.token)];
        words.extend(node.children[..inline].iter().map(|c| token_text(&c.token)));
        out.push_str(&words.join(" "));
        out.push('\n');
        stack.extend(node.children[inline..].iter().rev().map(|c| (c, depth + 1)));
    }
    out
}

/// トークン 1 個の字面。文字列は引用符で囲み、必要な文字をエスケープする。
fn token_text(token: &Token) -> String {
    match token {
        Token::Sym(s) => s.clone(),
        Token::Str(s) => {
            let mut out = String::from("\"");
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
        Token::Var { level, name } => format!("{}{}", "?".repeat(*level), name),
        Token::SeqVar { level, name } => format!("{}..{}", "?".repeat(*level), name),
    }
}

/// 1 行で書けるノードを集める。「最後の子以外がすべて葉で、最後の子も 1 行で書ける」なら 1 行で書ける。葉は常に 1 行。
/// ブロックの各段で最後の子の連鎖をたどり直すと深さの 2 乗かかるので、帰りがけ順で 1 回だけ判定する。
fn one_line_nodes(root: &Node) -> HashSet<*const Node> {
    let mut fits = HashSet::new();
    // 2 つ目は「子を積み終えたか」。子を先に判定するため、親は子の後に取り出す
    let mut stack = vec![(root, false)];
    while let Some((node, children_done)) = stack.pop() {
        if !children_done {
            stack.push((node, true));
            stack.extend(node.children.iter().map(|c| (c, false)));
            continue;
        }
        let ok = match node.children.split_last() {
            None => true,
            Some((last, init)) => init.iter().all(Node::is_leaf) && fits.contains(&(last as *const Node)),
        };
        if ok {
            fits.insert(node as *const Node);
        }
    }
    fits
}

/// 1 行で書ける木を 1 行にする。最後の子が葉でない間は `:` を付けてその子へ進む。
fn one_line(node: &Node) -> String {
    let mut words = Vec::new();
    let mut n = node;
    loop {
        words.push(token_text(&n.token));
        let Some((last, init)) = n.children.split_last() else { break };
        words.extend(init.iter().map(|c| token_text(&c.token)));
        if last.is_leaf() {
            words.push(token_text(&last.token));
            break;
        }
        words.last_mut().unwrap().push(':');
        n = last;
    }
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn roundtrip(src: &str) {
        assert_eq!(to_source(&parse(src).unwrap()), src);
    }

    #[test]
    fn leaves_only_stay_on_one_line() {
        roundtrip("-> greet hello\n");
        roundtrip("++ rev \"c\"\n");
    }

    #[test]
    fn right_deep_trees_use_colon() {
        roundtrip("show: s: s: add z: s z\n");
        roundtrip("++ \"|\": show ?n\n");
    }

    #[test]
    fn non_leaf_before_last_child_opens_a_block() {
        roundtrip("add\n  s: s z\n  s z\n");
        roundtrip("->\n  add z ?n\n  ?n\n");
    }

    #[test]
    fn colon_input_is_normalized() {
        assert_eq!(to_source(&parse("show\n  s\n    z").unwrap()), "show: s z\n");
    }

    #[test]
    fn paren_input_is_normalized_without_parens() {
        assert_eq!(to_source(&parse("show: add (s: s z) (s z)").unwrap()), "show\n  add\n    s: s z\n    s z\n");
    }

    #[test]
    fn variables_and_strings_are_written_back() {
        roundtrip("f ?x ??y ?..xs ??..ys \"a\\\"b\\\\c\\nd\"\n");
    }

    #[test]
    fn deep_colon_chain_prints_without_overflow() {
        // 最も内側の (s z) は子がすべて葉なので `:` を付けない
        let src = format!("{}s z\n", "s: ".repeat(99_999));
        assert!(to_source(&parse(&src).unwrap()) == src);
    }

    #[test]
    fn deep_block_nesting_roundtrips() {
        // 最後の子の手前に葉でない子があるので、1 段ごとにブロックが深くなる
        let tree = (0..3000).fold(Node::leaf(Token::sym("x")), |inner, _| {
            Node::branch(Token::sym("g"), vec![inner, Node::leaf(Token::sym("y"))])
        });
        let src = to_source(&tree);
        assert!(src.starts_with("g\n  g\n    g\n"));
        assert_eq!(parse(&src).unwrap(), tree);
    }

    #[test]
    fn sample_program_is_already_in_normal_form() {
        roundtrip(concat!(
            "main\n",
            "  ->\n",
            "    mkid ?f\n",
            "    ->\n",
            "      ?f ??x\n",
            "      ??x\n",
            "  ->\n",
            "    main ?..rules ?r\n",
            "    ?r\n",
            "  mkid id\n",
            "  id foo\n",
        ));
    }
}
