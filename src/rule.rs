//! 木の中のどのノードがルールとして働くかの判定（spec.md §5.1）。

use std::collections::HashSet;

use crate::tree::{Node, Token};

/// ルールとして働くノードの、パターンと置換。
#[derive(Clone, Copy, Debug)]
pub struct Rule<'a> {
    pub pattern: &'a Node,
    pub replacement: &'a Node,
}

/// ラベルが `->` で子が 2 つあり、置換のレベル 1 の変数がすべてパターンにも現れるなら、ルールとして返す。
/// 条件を満たさない `->` ノードはただのデータなので `None`。
/// メタ書き換えでルールとデータが入れ替わるため、呼び出し側は毎ステップ判定し直す。
pub fn as_rule(node: &Node) -> Option<Rule<'_>> {
    if !node.token.is_sym("->") || node.children.len() != 2 {
        return None;
    }
    let (pattern, replacement) = (&node.children[0], &node.children[1]);
    level1_vars(replacement).is_subset(&level1_vars(pattern)).then_some(Rule { pattern, replacement })
}

/// 木に現れるレベル 1 の変数を集める。`?x` と `?..x` はトークンが異なるので、別の変数として数える。
/// 置換を組み立てるときの代入は入れ子の `->` の中やラベル位置にも及ぶので、それらも数える。
/// データとして置かれた `->` も判定するので、深い木でも溢れないようスタックでたどる。
fn level1_vars(node: &Node) -> HashSet<&Token> {
    let mut vars = HashSet::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if matches!(n.token, Token::Var { level: 1, .. } | Token::SeqVar { level: 1, .. }) {
            vars.insert(&n.token);
        }
        stack.extend(&n.children);
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn is_rule(src: &str) -> bool {
        as_rule(&parse(src).unwrap()).is_some()
    }

    #[test]
    fn arrow_with_two_children_is_a_rule() {
        assert!(is_rule("->\n  add z ?n\n  ?n"));
        assert!(is_rule("-> greet hello"));
    }

    #[test]
    fn replacement_variable_missing_from_pattern_makes_data() {
        assert!(!is_rule("-> greet ?x"));
        assert!(!is_rule("->\n  f ?xs\n  g ?..xs"));
    }

    #[test]
    fn level2_variables_are_not_counted() {
        assert!(is_rule("->\n  mkid ?f\n  ->\n    ?f ??x\n    ??x"));
    }

    #[test]
    fn nested_rule_variables_are_counted() {
        assert!(!is_rule("->\n  mkid ?f\n  ->\n    ?f ?x\n    ?x"));
    }

    #[test]
    fn deep_arrow_is_judged_without_overflow() {
        let chain = format!("{}z", "s: ".repeat(100_000));
        assert!(is_rule(&format!("->\n  {chain}\n  z")));
        assert!(!is_rule(&format!("->\n  {chain}\n  ?x")));
    }

    #[test]
    fn wrong_shape_is_not_a_rule() {
        assert!(!is_rule("-> a"));
        assert!(!is_rule("-> a b c"));
        assert!(!is_rule("=> a b"));
    }
}
