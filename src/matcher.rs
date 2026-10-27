//! パターンと対象の照合、および束縛を使った置換の組み立て（spec.md §5.2〜§5.5）。

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::tree::{Node, Token};

/// レベル 1 の変数 `?x` に束縛した値。
/// 照合は失敗して捨てられることが多いので、対象の木を指すだけにして、複製は置換を組み立てるときまで遅らせる。
#[derive(Clone, Copy, Debug)]
pub enum Bound<'a> {
    /// 部分木そのもの（葉の位置の `?x`）。
    Tree(&'a Node),
    /// ノードのラベルだけ（`?f ...` のラベル位置）。子のないノードとして扱う。
    Label(&'a Token),
}

impl<'a> Bound<'a> {
    /// 非線形マッチで同じ値とみなすか。ラベルは、そのラベルを持つ子のないノードと等しい。
    fn same(self, other: Bound<'a>) -> bool {
        match (self, other) {
            (Bound::Tree(a), Bound::Tree(b)) => a == b,
            (Bound::Label(a), Bound::Label(b)) => a == b,
            (Bound::Tree(n), Bound::Label(l)) | (Bound::Label(l), Bound::Tree(n)) => n.is_leaf() && n.token == *l,
        }
    }

    /// ラベル位置に置くときのトークン。子を持つ部分木や文字列など、ラベルになれなければ `None`。
    fn as_label(self) -> Option<&'a Token> {
        let token = match self {
            Bound::Tree(n) if n.is_leaf() => &n.token,
            Bound::Tree(_) => return None,
            Bound::Label(t) => t,
        };
        token.can_be_label().then_some(token)
    }

    /// 葉の位置に置くときの部分木。
    fn to_node(self) -> Node {
        match self {
            Bound::Tree(n) => n.clone(),
            Bound::Label(t) => Node::leaf(t.clone()),
        }
    }
}

/// 照合で得た束縛。キーはパターンの変数名、値は対象の木を指す。
/// `?x` と `?..x` は別の変数なので、表を分けている。
#[derive(Debug, Default)]
pub struct Bindings<'a> {
    one: HashMap<&'a str, Bound<'a>>,
    seq: HashMap<&'a str, &'a [Node]>,
}

/// どちらの表に足した束縛か。分岐点へ戻るときに取り消すため記録する。
enum Added<'a> {
    One(&'a str),
    Seq(&'a str),
}

/// 照合中の束縛と、それを足した順の記録。
/// 分岐点へ戻るたびに束縛表を丸ごと複製しないよう、記録の後ろから取り消して分岐した時点の状態に戻す。
#[derive(Default)]
struct Trail<'a> {
    bindings: Bindings<'a>,
    added: Vec<Added<'a>>,
}

impl<'a> Trail<'a> {
    /// 束縛済みなら同じ値かを確かめ（非線形マッチ）、なければ束縛を足して記録する。
    fn bind_one(&mut self, name: &'a str, value: Bound<'a>) -> bool {
        match self.bindings.one.entry(name) {
            Entry::Occupied(e) => e.get().same(value),
            Entry::Vacant(e) => {
                e.insert(value);
                self.added.push(Added::One(name));
                true
            }
        }
    }

    /// 列変数を束縛して記録する。未束縛であることは呼び出し側で確かめておく。
    fn bind_seq(&mut self, name: &'a str, value: &'a [Node]) {
        self.bindings.seq.insert(name, value);
        self.added.push(Added::Seq(name));
    }

    /// 記録の長さが `len` になるまで、後ろから束縛を取り消す。
    fn undo_to(&mut self, len: usize) {
        for added in self.added.drain(len..) {
            match added {
                Added::One(name) => {
                    self.bindings.one.remove(name);
                }
                Added::Seq(name) => {
                    self.bindings.seq.remove(name);
                }
            }
        }
    }
}

/// 照合の残りの仕事。スタックの末尾から片付ける。
///
/// パターンはメタ書き換えで対象の木と同じくらい深くなりうるので、再帰ではなくスタックで照合する。
#[derive(Clone, Copy)]
enum Goal<'a> {
    /// パターンのノード 1 つを対象のノード 1 つに当てる。
    Node(&'a Node, &'a Node),
    /// パターンの子の列を対象の子の列に当てる。
    Children(&'a [Node], &'a [Node]),
}

/// 列変数の長さを選んだ分岐点。後の照合が失敗したら、ここへ戻って 1 つ長い長さを試す。
struct ChoicePoint<'a> {
    /// 分岐した時点の残りの仕事。列変数より後ろの子の列は含まない。
    goals: Vec<Goal<'a>>,
    /// 分岐した時点の束縛の記録の長さ。この列変数はまだ束縛していない。
    trail_len: usize,
    /// 列変数の名前。
    name: &'a str,
    /// 列変数より後ろのパターンの子。
    rest: &'a [Node],
    /// 列変数以降の対象の子。
    tgts: &'a [Node],
    next_len: usize,
}

/// パターンを対象に当て、最初に見つかった束縛を返す。
/// 仕事は行きがけ順に片付き、列変数は短い長さから試すので、左から最短一致の束縛が返る。
pub fn find_match<'a>(pattern: &'a Node, target: &'a Node) -> Option<Bindings<'a>> {
    let mut goals = vec![Goal::Node(pattern, target)];
    let mut trail = Trail::default();
    let mut choices = Vec::new();
    while let Some(goal) = goals.pop() {
        if !run_goal(goal, &mut goals, &mut trail, &mut choices) && !resume_choice(&mut choices, &mut goals, &mut trail) {
            return None;
        }
    }
    Some(trail.bindings)
}

/// 仕事を 1 つ片付け、続きの仕事を積む。当たらなければ `false`。
fn run_goal<'a>(goal: Goal<'a>, goals: &mut Vec<Goal<'a>>, trail: &mut Trail<'a>, choices: &mut Vec<ChoicePoint<'a>>) -> bool {
    match goal {
        Goal::Node(pat, tgt) => {
            let label_ok = match &pat.token {
                // 位置は 1 つのノードなので、並びを表す列変数は当たらない
                Token::SeqVar { level: 1, .. } => false,
                Token::Var { level: 1, name } if pat.is_leaf() => return trail.bind_one(name, Bound::Tree(tgt)),
                Token::Var { level: 1, name } => {
                    trail.bind_one(name, Bound::Label(&tgt.token))
                }
                token => lowers_to(token, &tgt.token),
            };
            if label_ok {
                goals.push(Goal::Children(&pat.children, &tgt.children));
            }
            label_ok
        }
        Goal::Children(pats, tgts) => {
            let Some((p, rest)) = pats.split_first() else { return tgts.is_empty() };
            if let (Token::SeqVar { level: 1, name }, true) = (&p.token, p.is_leaf()) {
                if let Some(&seq) = trail.bindings.seq.get(name.as_str()) {
                    if !tgts.starts_with(seq) {
                        return false;
                    }
                    goals.push(Goal::Children(rest, &tgts[seq.len()..]));
                    return true;
                }
                // 今の仕事と束縛が分岐した時点の状態そのものなので、長さ 0 は書き戻さずにこの場で試す
                choices.push(ChoicePoint { goals: goals.clone(), trail_len: trail.added.len(), name, rest, tgts, next_len: 1 });
                trail.bind_seq(name, &tgts[..0]);
                goals.push(Goal::Children(rest, tgts));
                return true;
            }
            let Some((t, trest)) = tgts.split_first() else { return false };
            // 先頭の子を先に片付けるため、残りの列を先に積む
            goals.push(Goal::Children(rest, trest));
            goals.push(Goal::Node(p, t));
            true
        }
    }
}

/// 直近の分岐点で次の長さを選び、仕事と束縛をそこから再開する状態に戻す。
/// 試し尽くした分岐点は捨てて 1 つ前へ戻る。分岐点が残っていなければ `false`。
fn resume_choice<'a>(choices: &mut Vec<ChoicePoint<'a>>, goals: &mut Vec<Goal<'a>>, trail: &mut Trail<'a>) -> bool {
    while let Some(cp) = choices.last_mut() {
        let (len, tgts) = (cp.next_len, cp.tgts);
        if len > tgts.len() {
            choices.pop();
            continue;
        }
        cp.next_len += 1;
        goals.clone_from(&cp.goals);
        goals.push(Goal::Children(cp.rest, &tgts[len..]));
        trail.undo_to(cp.trail_len);
        trail.bind_seq(cp.name, &tgts[..len]);
        return true;
    }
    false
}

/// レベル 2 以上の変数の `?` を 1 つ減らす。それ以外のトークンはそのまま。
pub fn lower_level(token: &Token) -> Token {
    match token {
        Token::Var { level, name } if *level >= 2 => Token::Var { level: level - 1, name: name.clone() },
        Token::SeqVar { level, name } if *level >= 2 => Token::SeqVar { level: level - 1, name: name.clone() },
        t => t.clone(),
    }
}

/// `lower_level(pat) == *tgt` と同じ判定。照合のたびに呼ばれるので、トークンを作らずに比べる。
fn lowers_to(pat: &Token, tgt: &Token) -> bool {
    match (pat, tgt) {
        (Token::Var { level: lp, name: np }, Token::Var { level: lt, name: nt })
        | (Token::SeqVar { level: lp, name: np }, Token::SeqVar { level: lt, name: nt })
            if *lp >= 2 =>
        {
            lp - 1 == *lt && np == nt
        }
        _ => pat == tgt,
    }
}

/// 置換のノード 1 つについて、子を見る前に決まる組み立て方。
enum Start<'b> {
    /// 束縛した部分木そのもの。子を組み立てる必要はない。
    Whole(Node),
    /// 親の子の列に展開して埋め込む並び（レベル 1 の列変数）。
    Splice(&'b [Node]),
    /// このラベルで、子を組み立ててからノードにする。
    Open(Token),
}

/// 組み立て途中のノード。
struct Frame<'a> {
    src: &'a Node,
    label: Token,
    /// 組み立て終えた子。列変数を展開するので、`src` の子の数とは一致しない。
    done: Vec<Node>,
    /// 次に組み立てる `src` の子の番号。
    next: usize,
}

/// 置換に束縛を代入して新しい部分木を作る。レベル 2 以上の変数は `?` を 1 つ減らす。
/// ラベル位置の変数がラベルになれない値に束縛されているときなど、組み立てられない場合は `None`。
pub fn instantiate(replacement: &Node, b: &Bindings<'_>) -> Option<Node> {
    let label = match start(replacement, b)? {
        Start::Whole(n) => return Some(n),
        // 根は 1 つのノードなので、並びには置き換えられない
        Start::Splice(_) => return None,
        Start::Open(label) => label,
    };
    // 置換も深くなりうるので、Node::clone と同じく帰りがけ順にスタックで組み立てる
    let mut stack = vec![Frame { src: replacement, label, done: Vec::new(), next: 0 }];
    loop {
        let frame = stack.last_mut().unwrap();
        let src = frame.src;
        if let Some(child) = src.children.get(frame.next) {
            frame.next += 1;
            match start(child, b)? {
                Start::Whole(n) => frame.done.push(n),
                Start::Splice(seq) => frame.done.extend(seq.iter().cloned()),
                Start::Open(label) => stack.push(Frame { src: child, label, done: Vec::new(), next: 0 }),
            }
            continue;
        }
        let frame = stack.pop().unwrap();
        let node = Node::branch(frame.label, frame.done);
        match stack.last_mut() {
            Some(parent) => parent.done.push(node),
            None => return Some(node),
        }
    }
}

/// 置換のノード 1 つの組み立て方を決める。束縛がない・ラベルになれないなど組み立てられなければ `None`。
fn start<'b>(rep: &Node, b: &'b Bindings<'_>) -> Option<Start<'b>> {
    match &rep.token {
        Token::SeqVar { level: 1, name } if rep.is_leaf() => b.seq.get(name.as_str()).map(|&seq| Start::Splice(seq)),
        Token::SeqVar { level: 1, .. } => None,
        Token::Var { level: 1, name } => {
            let v = *b.one.get(name.as_str())?;
            if rep.is_leaf() {
                return Some(Start::Whole(v.to_node()));
            }
            v.as_label().map(|label| Start::Open(label.clone()))
        }
        token => Some(Start::Open(lower_level(token))),
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

    fn shown(n: &Node) -> String {
        to_source(n).trim_end().to_string()
    }

    /// 照合で得た束縛を正規形の文字列にしたもの。束縛は木を借用するので、木を捨てる前に文字列にしておく。
    struct Shown {
        one: HashMap<String, String>,
        seq: HashMap<String, Vec<String>>,
    }

    /// `pat` を `tgt` に当て、当たれば束縛を文字列にして返す。
    fn matched(pat: &str, tgt: &str) -> Option<Shown> {
        let (pat, tgt) = (t(pat), t(tgt));
        let b = find_match(&pat, &tgt)?;
        Some(Shown {
            one: b.one.iter().map(|(k, v)| (k.to_string(), shown(&v.to_node()))).collect(),
            seq: b.seq.iter().map(|(k, ns)| (k.to_string(), ns.iter().map(shown).collect())).collect(),
        })
    }

    /// `pat` を `tgt` に当て（当たるものとする）、`rep` を組み立てた木の正規形を返す。組み立てられなければ `None`。
    fn rewrite(pat: &str, tgt: &str, rep: &str) -> Option<String> {
        let (pat, tgt) = (t(pat), t(tgt));
        let b = find_match(&pat, &tgt).expect("当たるはず");
        instantiate(&t(rep), &b).map(|r| to_source(&r))
    }

    #[test]
    fn level1_variable_binds_a_subtree() {
        let b = matched("add\n  s ?m\n  ?n", "add\n  s: s z\n  s z").unwrap();
        assert_eq!(b.one["m"], "s z");
        assert_eq!(b.one["n"], "s z");
    }

    #[test]
    fn symbols_and_strings_must_match_exactly() {
        assert!(matched("f abc", "f abc").is_some());
        assert!(matched("f abc", "f \"abc\"").is_none());
        assert!(matched("f a", "f a b").is_none());
    }

    #[test]
    fn repeated_variable_requires_equal_structure() {
        assert!(matched("eq ?x ?x", "eq\n  s z\n  s z").is_some());
        assert!(matched("eq ?x ?x", "eq z: s z").is_none());
    }

    #[test]
    fn sequence_variable_takes_the_rest() {
        let b = matched("rev ?c ?..rest", "rev \"a\" \"b\" \"c\"").unwrap();
        assert_eq!(b.one["c"], "\"a\"");
        assert_eq!(b.seq["rest"], vec!["\"b\"", "\"c\""]);
        let b = matched("rev ?c ?..rest", "rev \"c\"").unwrap();
        assert!(b.seq["rest"].is_empty());
        assert!(matched("rev ?c ?..rest", "rev").is_none());
    }

    #[test]
    fn multiple_sequence_variables_use_shortest_match_from_the_left() {
        let b = matched("f ?..a ?..b", "f 1 2").unwrap();
        assert!(b.seq["a"].is_empty());
        assert_eq!(b.seq["b"], vec!["1", "2"]);
        let b = matched("f ?..a x ?..b", "f 1 x 2 x 3").unwrap();
        assert_eq!(b.seq["a"], vec!["1"]);
        assert_eq!(b.seq["b"], vec!["2", "x", "3"]);
    }

    #[test]
    fn shortest_match_backtracks_across_siblings() {
        // ?..a を空にすると後ろの ?x ?x が一致しないので、1 個取る
        let b = matched("f ?..a ?x ?x", "f 1 2 2").unwrap();
        assert_eq!(b.seq["a"], vec!["1"]);
        assert_eq!(b.one["x"], "2");
    }

    #[test]
    fn backtracking_returns_to_a_choice_in_an_earlier_child_list() {
        // g の中の分け方 a=[] b=[1 2] は h で失敗するので、g の中へ戻って a を 1 個取る
        let b = matched("f\n  g ?..a ?..b\n  h ?..b", "f\n  g 1 2\n  h 2").unwrap();
        assert_eq!(b.seq["a"], vec!["1"]);
        assert_eq!(b.seq["b"], vec!["2"]);
    }

    #[test]
    fn backtracking_undoes_only_later_bindings() {
        // ?m は分岐より前、?x は分岐より後に束縛される。a=[] で ?x=1 と束縛した後に失敗して戻ると、
        // ?x の束縛だけが消え、?m は残ったまま a=[1] で ?x=2 を束縛し直せる
        let b = matched("f\n  m ?m\n  g ?..a ?x ?x", "f\n  m k\n  g 1 2 2").unwrap();
        assert_eq!(b.one["m"], "k");
        assert_eq!(b.seq["a"], vec!["1"]);
        assert_eq!(b.one["x"], "2");
        assert_eq!(b.one.len(), 2);
    }

    #[test]
    fn repeated_sequence_variable_requires_equal_sequence() {
        let b = matched("f ?..a ?..a", "f 1 2 1 2").unwrap();
        assert_eq!(b.seq["a"], vec!["1", "2"]);
        assert!(matched("f ?..a ?..a", "f 1 2 1").is_none());
    }

    #[test]
    fn label_variable_binds_only_the_label() {
        let b = matched("?f ?x", "id foo").unwrap();
        assert_eq!(b.one["f"], "id");
        assert_eq!(b.one["x"], "foo");
        assert!(matched("?f ?x", "g a b").is_none());
        assert!(matched("?f ?x", "foo").is_none());
    }

    #[test]
    fn label_and_leaf_occurrences_compare_equal() {
        assert!(matched("pair ?f: ?f x", "pair g: g x").is_some());
        assert!(matched("pair ?f: ?f x", "pair h: g x").is_none());
    }

    #[test]
    fn label_binding_equals_leaf_binding() {
        // label_and_leaf_occurrences_compare_equal とは逆に、ラベル位置で先に束縛してから葉の位置で比べる向き
        let pat = "pair\n  ?f x\n  ?f";
        assert!(matched(pat, "pair\n  g x\n  g").is_some());
        assert!(matched(pat, "pair\n  g x\n  g y").is_none());
        assert!(matched(pat, "pair\n  g x\n  \"g\"").is_none());
    }

    #[test]
    fn level2_variable_matches_level1_token_literally() {
        assert!(matched("->\n  id ??x\n  ??x", "->\n  id ?x\n  ?x").is_some());
        assert!(matched("->\n  id ??x\n  ??x", "->\n  id ?y\n  ?y").is_none());
    }

    #[test]
    fn root_sequence_variable_never_matches() {
        assert!(matched("?..xs", "a").is_none());
    }

    #[test]
    fn instantiate_substitutes_and_lowers_levels() {
        assert_eq!(rewrite("mkid ?f", "mkid id", "->\n  ?f ??x\n  ??x").unwrap(), "->\n  id ?x\n  ?x\n");
    }

    #[test]
    fn instantiate_expands_sequence_variables() {
        let rep = "++\n  rev ?..rest\n  ?c";
        assert_eq!(rewrite("rev ?c ?..rest", "rev \"a\" \"b\" \"c\"", rep).unwrap(), "++\n  rev \"b\" \"c\"\n  \"a\"\n");
        assert_eq!(rewrite("rev ?c ?..rest", "rev \"c\"", rep).unwrap(), "++ rev \"c\"\n");
    }

    #[test]
    fn label_variable_bound_to_non_label_fails() {
        assert_eq!(rewrite("h ?x", "h \"s\"", "?x a"), None);
        assert_eq!(rewrite("h ?x", "h: g a", "?x a"), None);
        assert_eq!(rewrite("h ?x", "h g", "?x a").unwrap(), "g a\n");
    }

    #[test]
    fn label_binding_instantiates_as_leaf_and_label() {
        assert_eq!(rewrite("?f a", "g a", "pair ?f: ?f b").unwrap(), "pair g: g b\n");
    }

    #[test]
    fn root_sequence_variable_in_replacement_fails() {
        assert_eq!(rewrite("f ?..xs", "f a", "?..xs"), None);
    }

    /// `s` が `depth` 段重なり、一番下が `bottom` の連鎖のソース。
    fn chain(depth: usize, bottom: &str) -> String {
        format!("{}{bottom}", "s: ".repeat(depth))
    }

    #[test]
    fn deep_pattern_matches_without_overflow() {
        let pat = t(&chain(100_000, "?x"));
        let tgt = t(&chain(100_000, "z"));
        let b = find_match(&pat, &tgt).unwrap();
        assert!(b.one["x"].to_node() == t("z"));
        assert!(find_match(&pat, &t(&chain(99_999, "z"))).is_none());
    }

    #[test]
    fn wide_pattern_matches_without_overflow() {
        // 兄弟の照合も積み重なる形だと、子の数だけ深くなる
        let n = 100_000;
        let pat = t(&format!("f{}", " ?x".repeat(n)));
        assert!(find_match(&pat, &t(&format!("f{}", " a".repeat(n)))).is_some());
        assert!(find_match(&pat, &t(&format!("f{} b", " a".repeat(n - 1)))).is_none());
    }

    #[test]
    fn deep_replacement_instantiates_without_overflow() {
        let (pat, tgt) = (t("f ?x"), t("f z"));
        let b = find_match(&pat, &tgt).unwrap();
        let r = instantiate(&t(&chain(100_000, "?x")), &b).unwrap();
        assert!(r == t(&chain(100_000, "z")));
    }
}
