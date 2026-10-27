//! 書き換えの進行（spec.md 第 6 章）。
//! 帰りがけ順で位置を調べ、近いスコープのルールから試し、最初に当たった 1 箇所だけを書き換える。

use crate::matcher::{find_match, instantiate};
use crate::rule::{as_rule, Rule};
use crate::tree::{Node, Token};

/// 1 ステップで何が当たったか。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Applied {
    /// 木の中のルール。`at` はそのルールのノードの位置。
    Rule { at: Vec<usize> },
    /// 外側スコープにある組込みの連結ルール。
    Concat,
}

/// 次に書き換わる箇所と、そこに入る部分木。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Redex {
    pub path: Vec<usize>,
    pub replacement: Node,
    pub applied: Applied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 根が単一のアトムになった。中身はそのアトム。
    /// 出力の字面だけではシンボルか文字列か区別できず、ソースとして書き戻せないので、木のまま持つ。
    Folded(Node),
    /// 書き換えられる箇所がないまま、根がアトムにならなかった（折りたたみ失敗）。
    Stuck(Node),
    /// ステップ数の上限に達した。
    StepLimit(Node),
}

impl Outcome {
    /// 止まった時点の木 Tₙ（spec.md §6.6）。どの終わり方でも、そのまま正規形にして書き出せる。
    pub fn tree(&self) -> &Node {
        match self {
            Outcome::Folded(tree) | Outcome::Stuck(tree) | Outcome::StepLimit(tree) => tree,
        }
    }
}

/// スコープに置かれたルール。
#[derive(Clone, Copy)]
enum ScopeRule<'a> {
    /// 木の中のルール。1 つ目は親の子としての番号。
    Tree(usize, Rule<'a>),
    /// 外側スコープの連結ルール群 `-> (++ "a" "b") "ab"`。
    Concat,
}

impl ScopeRule<'_> {
    /// 対象に当て、当たれば置換を返す。
    fn apply(self, target: &Node) -> Option<Node> {
        match self {
            ScopeRule::Tree(_, rule) => {
                let b = find_match(rule.pattern, target)?;
                // 置換を組み立てられないときは、このルールは当たらなかったものとして次を試す
                instantiate(rule.replacement, &b)
            }
            ScopeRule::Concat => builtin_concat(target),
        }
    }
}

/// ルールを子に持つ祖先 1 つ分のスコープ。
struct Scope<'a> {
    /// その祖先の深さ（パスの長さ）。外側スコープでは使わない。
    depth: usize,
    /// 子にあるルール。
    rules: Vec<ScopeRule<'a>>,
}

impl Scope<'_> {
    /// 根より外側にあり、組込みの連結だけを持つスコープ（spec.md §6.4）。
    /// 常に `scopes` の底に置くので、どの位置でも最後に試される。
    fn outer() -> Self {
        Scope { depth: 0, rules: vec![ScopeRule::Concat] }
    }
}

/// 調べている途中のデータのノード 1 つ分の状態。
struct Frame<'a> {
    node: &'a Node,
    /// 次に調べる子の番号。
    next: usize,
    /// 各子がルールかどうか。ノードに入るときに 1 回だけ判定する。
    child_is_rule: Vec<bool>,
    /// このノードのスコープを `scopes` に積んだか。
    pushed_scope: bool,
}

/// 次に書き換わる箇所を探す。どこにも当たらなければ `None`。
///
/// 帰りがけ順（最左最内）で位置を調べ、最初に当たった位置で打ち切る。ルールのノードは 1 つの位置とし、内部には入らない。
/// 深い木でもスタックを使わないよう、調べている途中のノードを `frames` に積む。
/// 各ノードのルール判定を 1 回で済ませるため、祖先の子にあるルールを、その子孫を調べる間だけ `scopes` に積んでおく。
/// 位置ごとの走査が深さに比例しないよう、ルールを持たない祖先は積まない。
pub fn find_redex(root: &Node) -> Option<Redex> {
    let mut scopes = vec![Scope::outer()];
    let mut path = Vec::new();
    let mut frames = Vec::new();
    // 根は、外側スコープを持つ仮想の親の唯一の子として、ほかの子と同じ手順で扱う
    if let Some(redex) = visit(root, as_rule(root).is_some(), &mut path, &mut frames, &mut scopes) {
        return Some(redex);
    }
    while let Some(top) = frames.last_mut() {
        let node = top.node;
        if top.next < node.children.len() {
            let i = top.next;
            top.next += 1;
            let is_rule = top.child_is_rule[i];
            path.push(i);
            if let Some(redex) = visit(&node.children[i], is_rule, &mut path, &mut frames, &mut scopes) {
                return Some(redex);
            }
            continue;
        }
        // ルールのスコープは親自身を含むので、自分の子にあるルールは自分にも当たりうる。スコープを外す前に試す
        let frame = frames.pop().unwrap();
        let found = try_at(frame.node, &path, &scopes);
        if frame.pushed_scope {
            scopes.pop();
        }
        if found.is_some() {
            return found;
        }
        path.pop();
    }
    None
}

/// 位置 `path` のノードに進む。ルールなら内部に入らずその位置で試し、当たらなければ `path` から外れる。
/// データなら中に入ってフレームを積む（その位置は、フレームを調べ終えたときに外れる）。
fn visit<'a>(
    node: &'a Node,
    is_rule: bool,
    path: &mut Vec<usize>,
    frames: &mut Vec<Frame<'a>>,
    scopes: &mut Vec<Scope<'a>>,
) -> Option<Redex> {
    if !is_rule {
        frames.push(enter(node, path.len(), scopes));
        return None;
    }
    // ルールのノードの内部はルールの一部なので、親のスコープから試す
    let found = try_at(node, path, scopes);
    if found.is_none() {
        path.pop();
    }
    found
}

/// データのノードに入る。子のルールを判定し、ルールがあればスコープとして積む。
fn enter<'a>(node: &'a Node, depth: usize, scopes: &mut Vec<Scope<'a>>) -> Frame<'a> {
    let mut rules = Vec::new();
    let mut child_is_rule = Vec::with_capacity(node.children.len());
    for (i, child) in node.children.iter().enumerate() {
        let rule = as_rule(child);
        child_is_rule.push(rule.is_some());
        rules.extend(rule.map(|r| ScopeRule::Tree(i, r)));
    }
    let pushed_scope = !rules.is_empty();
    if pushed_scope {
        scopes.push(Scope { depth, rules });
    }
    Frame { node, next: 0, child_is_rule, pushed_scope }
}

/// 位置 `path` のノードに、近いスコープのルールから順に（同じスコープ内では左から）当てる。
fn try_at(target: &Node, path: &[usize], scopes: &[Scope<'_>]) -> Option<Redex> {
    for scope in scopes.iter().rev() {
        for &rule in &scope.rules {
            let Some(replacement) = rule.apply(target) else { continue };
            let applied = match rule {
                ScopeRule::Tree(i, _) => {
                    let mut at = path[..scope.depth].to_vec();
                    at.push(i);
                    Applied::Rule { at }
                }
                ScopeRule::Concat => Applied::Concat,
            };
            return Some(Redex { path: path.to_vec(), replacement, applied });
        }
    }
    None
}

/// 1 箇所だけ書き換える。書き換えられる箇所がなければ木は変えずに `None`。
pub fn step(root: &mut Node) -> Option<Applied> {
    let redex = find_redex(root)?;
    root.replace_at(&redex.path, redex.replacement);
    Some(redex.applied)
}

/// 止まるまで書き換える。`on_step` は初期状態（0）と、各ステップの後の木を受け取る。
pub fn run(mut root: Node, max_steps: Option<usize>, on_step: &mut dyn FnMut(usize, &Node)) -> Outcome {
    on_step(0, &root);
    let mut count = 0;
    while let Some(redex) = find_redex(&root) {
        if max_steps.is_some_and(|limit| count >= limit) {
            return Outcome::StepLimit(root);
        }
        root.replace_at(&redex.path, redex.replacement);
        count += 1;
        on_step(count, &root);
    }
    if root.atom_text().is_some() {
        Outcome::Folded(root)
    } else {
        Outcome::Stuck(root)
    }
}

/// 外側スコープの連結ルール群 `-> (++ "a" "b") "ab"` を、計算で代用して当てる。
fn builtin_concat(target: &Node) -> Option<Node> {
    if !target.token.is_sym("++") {
        return None;
    }
    let [a, b] = target.children.as_slice() else { return None };
    match (&a.token, &b.token) {
        (Token::Str(x), Token::Str(y)) if a.is_leaf() && b.is_leaf() => Some(Node::leaf(Token::Str(format!("{x}{y}")))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::printer::to_source;

    fn t(src: &str) -> Node {
        parse(src).unwrap()
    }

    fn after_one_step(src: &str) -> String {
        let mut root = t(src);
        step(&mut root).expect("書き換えられる箇所があるはず");
        to_source(&root)
    }

    #[test]
    fn innermost_position_is_rewritten_first() {
        // 先行順なら (g x) が bad になるが、帰りがけ順なので先に x が y になる
        let src = "main\n  -> x y\n  ->\n    g x\n    bad\n  f: g x";
        assert!(after_one_step(src).ends_with("  f: g y\n"));
    }

    #[test]
    fn rule_interior_is_opaque() {
        let mut root = t("main\n  -> a b\n  wrap: -> a c");
        assert_eq!(step(&mut root), None);
    }

    #[test]
    fn data_arrow_is_entered() {
        // 置換の ?x がパターンにないのでルールではなく、内部も書き換え位置になる
        assert!(after_one_step("main\n  -> a b\n  wrap: -> a ?x").ends_with("  wrap: -> b ?x\n"));
    }

    #[test]
    fn nearest_scope_wins() {
        let src = "main\n  -> x A\n  f\n    -> x B\n    -> x C\n    g x\n  -> x D";
        let mut root = t(src);
        assert_eq!(step(&mut root), Some(Applied::Rule { at: vec![1, 0] }));
        assert_eq!(root.get(&[1, 2, 0]), &t("B"));
    }

    #[test]
    fn leftmost_rule_wins_within_a_scope() {
        let mut root = t("main\n  -> x A\n  f: g x\n  -> x D");
        assert_eq!(step(&mut root), Some(Applied::Rule { at: vec![0] }));
    }

    #[test]
    fn own_children_of_a_non_rule_position_come_first() {
        let src = "main\n  ->\n    f ?..ys\n    outer\n  f\n    ->\n      f ?..ys\n      inner\n    z";
        assert_eq!(after_one_step(src), "main\n  ->\n    f ?..ys\n    outer\n  inner\n");
    }

    #[test]
    fn rule_can_match_its_own_node() {
        let mut root = t("main\n  ->\n    -> ?p ?r\n    gone\n  x");
        assert_eq!(step(&mut root), Some(Applied::Rule { at: vec![0] }));
        assert_eq!(root.get(&[0]), &t("gone"));
    }

    #[test]
    fn nearer_rule_shadows_builtin_concat() {
        let src = "main\n  ->\n    ++ \"a\" \"b\"\n    shadowed\n  ++ \"a\" \"b\"";
        assert!(after_one_step(src).ends_with("  shadowed\n"));
    }

    #[test]
    fn builtin_concat_needs_exactly_two_strings() {
        assert_eq!(builtin_concat(&t("++ \"a\" \"b\"")), Some(Node::leaf(Token::Str("ab".into()))));
        assert_eq!(builtin_concat(&t("++ \"a\" b")), None);
        assert_eq!(builtin_concat(&t("++ \"a\" \"b\" \"c\"")), None);
        assert_eq!(builtin_concat(&t("++ \"a\"")), None);
        assert_eq!(builtin_concat(&t("++")), None);
    }

    #[test]
    fn rule_can_rewrite_another_rule() {
        let src = "main\n  -> greet hello\n  ->\n    -> greet hello\n    -> greet goodbye\n  greet";
        let mut root = t(src);
        assert_eq!(step(&mut root), Some(Applied::Rule { at: vec![1] }));
        assert_eq!(root.get(&[0]), &t("-> greet goodbye"));
    }

    #[test]
    fn rule_that_cannot_be_instantiated_is_skipped() {
        // 1 本目は ?x が文字列に束縛されてラベルにできないので、2 本目が当たる
        let src = "main\n  ->\n    h ?x\n    ?x a\n  ->\n    h ?x\n    ok\n  h \"s\"";
        assert!(after_one_step(src).ends_with("  ok\n"));
    }

    #[test]
    fn run_folds_to_an_atom() {
        let src = "main\n  ->\n    main ?..rules ?r\n    ?r\n  ++ \"a\" \"b\"";
        let mut steps = 0;
        let outcome = run(t(src), None, &mut |n, _| steps = n);
        assert_eq!(outcome, Outcome::Folded(t("\"ab\"")));
        assert_eq!(steps, 2);
    }

    #[test]
    fn run_reports_stuck_tree() {
        let outcome = run(t("main a b"), None, &mut |_, _| {});
        assert_eq!(outcome, Outcome::Stuck(t("main a b")));
    }

    #[test]
    fn variable_leaf_root_is_not_an_atom() {
        assert_eq!(run(t("?x"), None, &mut |_, _| {}), Outcome::Stuck(t("?x")));
    }

    #[test]
    fn run_stops_at_step_limit() {
        // a と b を互いに書き換え続けるので止まらない
        let src = "main\n  -> a b\n  -> b a\n  a";
        let mut last = 0;
        let outcome = run(t(src), Some(5), &mut |n, _| last = n);
        assert!(matches!(outcome, Outcome::StepLimit(_)));
        assert_eq!(last, 5);
    }

    #[test]
    fn outcome_tree_is_the_halted_tree() {
        assert_eq!(Outcome::Folded(t("yes")).tree(), &t("yes"));
        assert_eq!(Outcome::Stuck(t("main a b")).tree(), &t("main a b"));
        assert_eq!(Outcome::StepLimit(t("main: -> a b")).tree(), &t("main: -> a b"));
    }

    #[test]
    fn deep_tree_runs_without_overflow() {
        // 深さ 10 万の連鎖 2 本を探索し、非線形マッチで比べ、束縛として複製する
        let chain = format!("{}z", "s: ".repeat(100_000));
        let src = format!("main\n  ->\n    main ?r: eq ?x ?x\n    yes\n  eq\n    {chain}\n    {chain}");
        let mut steps = 0;
        let outcome = run(t(&src), None, &mut |n, _| steps = n);
        assert_eq!(outcome, Outcome::Folded(t("yes")));
        assert_eq!(steps, 1);
    }

    #[test]
    fn root_rule_has_no_scope() {
        let mut root = t("-> a a");
        assert_eq!(step(&mut root), None);
    }

    #[test]
    fn builtin_concat_applies_at_root() {
        let mut root = t("++ \"a\" \"b\"");
        assert_eq!(step(&mut root), Some(Applied::Concat));
        assert_eq!(root, t("\"ab\""));
    }
}
