//! プログラムでありデータでもある、ラベル付き順序木。

/// ノードのラベル、または葉そのものになるトークン。
///
/// 変数もトークンの一種として木に置かれる。変数として解釈されるのは、
/// ルールのパターンや置換として使われるときだけである。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Token {
    /// シンボル。名前で区別されるだけで、分解も連結もできない。
    Sym(String),
    /// 文字列。中身を持ち、組込みの連結の対象になる。
    Str(String),
    /// `?x`（level 1）、`??x`（level 2）…。
    Var { level: usize, name: String },
    /// `?..xs`（level 1）、`??..xs`（level 2）…。
    SeqVar { level: usize, name: String },
}

impl Token {
    pub fn sym(name: &str) -> Token {
        Token::Sym(name.to_string())
    }

    /// 名前が `name` のシンボルかどうか。毎ステップ全位置で呼ばれるので、比較用のトークンを作らずに確かめる。
    pub fn is_sym(&self, name: &str) -> bool {
        matches!(self, Token::Sym(s) if s == name)
    }

    /// 子を持つノードのラベルに置けるかどうか。
    /// 文字列は値なのでラベルにならない。列変数は並びを表すので 1 つのラベルにならない。
    pub fn can_be_label(&self) -> bool {
        matches!(self, Token::Sym(_) | Token::Var { .. })
    }
}

/// 木のノード。子を持たないノードがアトム（または変数トークンの葉）になる。
///
/// ペアノ数のように木は深くなりやすく、derive した `Clone`・`PartialEq` や既定の破棄は深さだけ再帰して
/// スタックを溢れさせる。そのため、この 3 つは明示的なスタックで実装している。
/// `Debug` だけは derive のままで、深い木を表示するとスタックが溢れうる（使うのはテストの失敗時くらい）。
#[derive(Debug)]
pub struct Node {
    pub token: Token,
    pub children: Vec<Node>,
}

impl Clone for Node {
    fn clone(&self) -> Node {
        // 帰りがけ順に複製する。各段は「複製元」と「複製し終えた子」の組
        let mut stack: Vec<(&Node, Vec<Node>)> = vec![(self, Vec::with_capacity(self.children.len()))];
        loop {
            let (src, done) = stack.last_mut().unwrap();
            let src = *src;
            if done.len() < src.children.len() {
                let child = &src.children[done.len()];
                stack.push((child, Vec::with_capacity(child.children.len())));
                continue;
            }
            let (src, done) = stack.pop().unwrap();
            let node = Node { token: src.token.clone(), children: done };
            match stack.last_mut() {
                Some((_, parent_done)) => parent_done.push(node),
                None => return node,
            }
        }
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Node) -> bool {
        let mut stack = vec![(self, other)];
        while let Some((a, b)) = stack.pop() {
            if a.token != b.token || a.children.len() != b.children.len() {
                return false;
            }
            stack.extend(a.children.iter().zip(&b.children));
        }
        true
    }
}

impl Eq for Node {}

impl Drop for Node {
    fn drop(&mut self) {
        // 子孫を平らな列に移してから捨てる。移した後の各ノードは子を持たないので、破棄は再帰しない
        let mut pending = std::mem::take(&mut self.children);
        while let Some(mut node) = pending.pop() {
            pending.append(&mut node.children);
        }
    }
}

impl Node {
    pub fn leaf(token: Token) -> Node {
        Node { token, children: Vec::new() }
    }

    pub fn branch(token: Token, children: Vec<Node>) -> Node {
        Node { token, children }
    }

    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    /// 出力になれるアトム（子のないシンボルか文字列）なら、出力する字面を返す（spec.md §6.6）。
    /// 文字列は引用符を外した中身、シンボルは名前。
    pub fn atom_text(&self) -> Option<&str> {
        match &self.token {
            Token::Sym(s) | Token::Str(s) if self.is_leaf() => Some(s),
            _ => None,
        }
    }

    /// 根からの子の番号の列で示した位置のノード。
    pub fn get(&self, path: &[usize]) -> &Node {
        path.iter().fold(self, |n, &i| &n.children[i])
    }

    /// 指定した位置の部分木を置き換える。空のパスなら木全体を置き換える。
    pub fn replace_at(&mut self, path: &[usize], new: Node) {
        let mut n = self;
        for &i in path {
            n = &mut n.children[i];
        }
        *n = new;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str) -> Node {
        Node::leaf(Token::sym(name))
    }

    #[test]
    fn atom_is_leaf_symbol_or_string() {
        assert_eq!(s("z").atom_text(), Some("z"));
        assert_eq!(Node::leaf(Token::Var { level: 1, name: "x".into() }).atom_text(), None);
        assert_eq!(Node::branch(Token::sym("s"), vec![s("z")]).atom_text(), None);
    }

    #[test]
    fn atom_text_is_unquoted_content() {
        assert_eq!(Node::leaf(Token::Str("|||".into())).atom_text(), Some("|||"));
        assert_eq!(Node::leaf(Token::Str(String::new())).atom_text(), Some(""));
    }

    #[test]
    fn get_and_replace_at_follow_paths() {
        let mut t = Node::branch(Token::sym("a"), vec![s("b"), Node::branch(Token::sym("c"), vec![s("d")])]);
        assert_eq!(t.get(&[1, 0]), &s("d"));
        t.replace_at(&[1, 0], s("e"));
        assert_eq!(t.get(&[1, 0]), &s("e"));
        t.replace_at(&[], s("z"));
        assert_eq!(t, s("z"));
    }

    /// `s` が `depth` 段重なった `s: s: … z`。
    fn chain(depth: usize) -> Node {
        (0..depth).fold(s("z"), |inner, _| Node::branch(Token::sym("s"), vec![inner]))
    }

    #[test]
    fn deep_trees_clone_compare_and_drop_without_overflow() {
        let a = chain(100_000);
        let b = a.clone();
        assert!(a == b);
        assert!(a != chain(99_999));
        drop(a);
        drop(b);
    }

    #[test]
    fn equality_checks_tokens_and_shape() {
        let wide = Node::branch(Token::sym("f"), vec![s("a"), s("b")]);
        assert_eq!(wide.clone(), wide);
        assert_ne!(wide, Node::branch(Token::sym("f"), vec![s("a"), s("c")]));
        assert_ne!(wide, Node::branch(Token::sym("f"), vec![s("a")]));
        assert_ne!(wide, Node::branch(Token::sym("g"), vec![s("a"), s("b")]));
    }
}
