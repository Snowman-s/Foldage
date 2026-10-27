//! インデント + 行内ネスト記号 `:` + 行内の部分木 `()` の構文（spec.md 第 2 章・第 3 章）を木に読み込む。

use std::fmt;

use crate::tree::{Node, Token};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// 1 始まりの行番号。
    pub line: usize,
    pub kind: ParseErrorKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// 木が 1 本も書かれていない。
    Empty,
    /// 文字列の外にタブがある。
    Tab,
    /// 文字列の外に CR か U+FEFF がある。
    Invisible(char),
    /// インデントがスペース 2 個の倍数でない。
    BadIndent,
    /// 親の行より 2 段以上深い、または先頭行が字下げされている。
    UnexpectedIndent,
    /// 深さ 0 の行が 2 つ以上ある。
    MultipleRoots,
    UnterminatedString,
    BadEscape(char),
    /// 閉じ引用符（と `:`）の直後に、空白・`)`・行末以外が続いている。
    TrailingAfterString,
    /// `:` の右側に何もない。
    DanglingColon,
    /// `:` を含む行に子ブロックがある。
    ColonWithBlock,
    /// シンボルにも変数にもならないトークン。
    InvalidToken(String),
    /// 文字列が子を持つノードのラベルになっている。
    StringLabel,
    /// 列変数が子を持つノードのラベルになっている。
    SeqVarLabel,
    /// `(` が同じ行で閉じていない。
    UnclosedParen,
    /// 対応する `(` のない `)` がある。
    UnmatchedParen,
    /// 中身のない `()`。
    EmptyGroup,
    /// 括弧で囲んだ部分木が、子を持つノードのラベルになっている。
    GroupLabel,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} 行目: ", self.line)?;
        match &self.kind {
            ParseErrorKind::Empty => f.write_str("木が書かれていません"),
            ParseErrorKind::Tab => f.write_str("タブは使えません。インデントはスペース 2 個です"),
            ParseErrorKind::Invisible(c) => write!(f, "U+{:04X} は文字列とコメントの中でしか使えません", u32::from(*c)),
            ParseErrorKind::BadIndent => f.write_str("インデントはスペース 2 個を 1 段としてください"),
            ParseErrorKind::UnexpectedIndent => f.write_str("インデントが深すぎます。子は親より 1 段だけ深くしてください"),
            ParseErrorKind::MultipleRoots => f.write_str("根が複数あります。プログラムは 1 本の木です"),
            ParseErrorKind::UnterminatedString => f.write_str("文字列が閉じていません"),
            ParseErrorKind::BadEscape(c) => write!(f, "文字列中のエスケープ \\{c} は使えません（\\\" \\\\ \\n のみ）"),
            ParseErrorKind::TrailingAfterString => f.write_str("文字列（と `:`）の直後には空白か `)` を置いてください"),
            ParseErrorKind::DanglingColon => f.write_str("`:` の右側に子がありません"),
            ParseErrorKind::ColonWithBlock => f.write_str("`:` を含む行は子ブロックを持てません（子の位置にある括弧の中の `:` は除く）"),
            ParseErrorKind::InvalidToken(t) => write!(f, "トークン `{t}` は使えません"),
            ParseErrorKind::StringLabel => f.write_str("文字列は子を持つノードのラベルになれません"),
            ParseErrorKind::SeqVarLabel => f.write_str("列変数は子を持つノードのラベルになれません"),
            ParseErrorKind::UnclosedParen => f.write_str("`(` が閉じていません。括弧は同じ行で閉じてください"),
            ParseErrorKind::UnmatchedParen => f.write_str("対応する `(` のない `)` があります"),
            ParseErrorKind::EmptyGroup => f.write_str("`()` の中が空です"),
            ParseErrorKind::GroupLabel => f.write_str("括弧で囲んだ部分木は子を持つノードのラベルになれません"),
        }
    }
}

impl std::error::Error for ParseError {}

/// 行内の字句。`colon` は末尾に `:` が付いていたかどうか。
enum Lexeme {
    Token { token: Token, colon: bool },
    Open,
    Close { colon: bool },
}

/// 組み立て途中の、並びの要素。`group` は括弧で囲んだ部分木かどうか。
/// `colon_last` は、`node` の最後の子が `:` で付いたかどうか。
/// `(s: z)` だけの行に子ブロックを付けると `:` の約束（`z` が最後の子）が崩れるので、括弧を外しても引き継ぐ。
struct Item {
    node: Node,
    group: bool,
    colon: bool,
    colon_last: bool,
}

/// 1 行を読んだ結果。子ブロックはまだ付いていない。
/// `has_colon` は行のノードの最後の子が `:` で付いたかどうか。子の位置にある括弧の中の `:` は `)` で閉じるので、子ブロックを妨げない。
struct Line {
    line_no: usize,
    depth: usize,
    node: Node,
    has_colon: bool,
}

/// ソース全体を 1 本の木に読み込む。
pub fn parse(src: &str) -> Result<Node, ParseError> {
    // Windows のエディタが付ける BOM が、先頭のトークンに混ざらないようにする
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut lines = Vec::new();
    for (i, text) in src.lines().enumerate() {
        let line_no = i + 1;
        let (depth, body) = split_indent(text, line_no)?;
        let lexemes = lex_line(body, line_no)?;
        if lexemes.is_empty() {
            continue;
        }
        let (node, has_colon) = build_line(lexemes, line_no)?;
        lines.push(Line { line_no, depth, node, has_colon });
    }
    assemble(lines)
}

/// 行頭のスペースを数えてインデントの段数を返す。
fn split_indent(text: &str, line_no: usize) -> Result<(usize, &str), ParseError> {
    let spaces = text.len() - text.trim_start_matches(' ').len();
    let body = &text[spaces..];
    if let Some(kind) = body.chars().next().and_then(misplaced) {
        return Err(ParseError { line: line_no, kind });
    }
    // 空行やコメントだけの行は段数を問わない。全角空白などは字句になるので空行に含めない
    let blank = body.is_empty() || body.starts_with('#');
    if !blank && !spaces.is_multiple_of(2) {
        return Err(ParseError { line: line_no, kind: ParseErrorKind::BadIndent });
    }
    Ok((spaces / 2, body))
}

/// 1 行を字句に分ける。`#` がトークンの先頭に来たら、そこから行末までを捨てる。
/// 「シンボルは `#` で始まらない」という字句規則と合わせるため、シンボルの途中の `#` はコメントにしない。
/// `z)` のように閉じ括弧をトークンに付けて書けるよう、`(` と `)` はそれ自体で区切りになる。
fn lex_line(body: &str, line_no: usize) -> Result<Vec<Lexeme>, ParseError> {
    let err = |kind| ParseError { line: line_no, kind };
    let chars: Vec<char> = body.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some(kind) = misplaced(chars[i]) {
            return Err(err(kind));
        }
        match chars[i] {
            ' ' => i += 1,
            '#' => break,
            '(' => {
                tokens.push(Lexeme::Open);
                i += 1;
            }
            ')' => {
                i += 1;
                let colon = take_colon(&chars, &mut i);
                tokens.push(Lexeme::Close { colon });
            }
            '"' => {
                let mut s = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err(err(ParseErrorKind::UnterminatedString)),
                        Some('"') => break,
                        Some('\\') => {
                            let c = match chars.get(i + 1) {
                                Some('"') => '"',
                                Some('\\') => '\\',
                                Some('n') => '\n',
                                Some(&c) => return Err(err(ParseErrorKind::BadEscape(c))),
                                None => return Err(err(ParseErrorKind::UnterminatedString)),
                            };
                            s.push(c);
                            i += 2;
                        }
                        Some(&c) => {
                            s.push(c);
                            i += 1;
                        }
                    }
                }
                i += 1;
                let colon = take_colon(&chars, &mut i);
                // タブなどはここで弾かず、ループの先頭でその文字のエラーにする
                let delimited = match chars.get(i) {
                    None | Some(' ' | ')') => true,
                    Some(&c) => misplaced(c).is_some(),
                };
                if !delimited {
                    return Err(err(ParseErrorKind::TrailingAfterString));
                }
                tokens.push(Lexeme::Token { token: Token::Str(s), colon });
            }
            _ => {
                let start = i;
                while i < chars.len() && !matches!(chars[i], ' ' | '(' | ')') && misplaced(chars[i]).is_none() {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                tokens.push(parse_name_lexeme(&text, line_no)?);
            }
        }
    }
    Ok(tokens)
}

/// 文字列とコメントの外に置けない文字なら、そのエラーの種類を返す。
/// CR と U+FEFF は、シンボルに入ると正規形の行末で行終端子に、先頭で BOM に化けて往復が壊れるので禁じる（spec.md §2.4）。
fn misplaced(c: char) -> Option<ParseErrorKind> {
    match c {
        '\t' => Some(ParseErrorKind::Tab),
        '\r' | '\u{feff}' => Some(ParseErrorKind::Invisible(c)),
        _ => None,
    }
}

/// 閉じ記号（`)` と閉じ引用符）の直後に付く行内ネストの `:` を読み、付いていたかを返す。
fn take_colon(chars: &[char], i: &mut usize) -> bool {
    let colon = chars.get(*i) == Some(&':');
    if colon {
        *i += 1;
    }
    colon
}

/// 引用符で囲まれていないトークンを、シンボル・変数・列変数のいずれかとして読む。
fn parse_name_lexeme(text: &str, line_no: usize) -> Result<Lexeme, ParseError> {
    let invalid = || ParseError { line: line_no, kind: ParseErrorKind::InvalidToken(text.to_string()) };
    let (body, colon) = match text.strip_suffix(':') {
        Some(b) => (b, true),
        None => (text, false),
    };
    // `:` 始まりを禁止するのは、`(a):c` を `(a): c` と一通りに読むため（spec.md §2.8）
    // `:` 終わりを禁止するのは、名前の末尾の `:` が行内ネストの `:` と区別できず、正規形の往復が壊れるため（spec.md §2.6）
    if body.is_empty() || body.contains('"') || body.starts_with(':') || body.ends_with(':') {
        return Err(invalid());
    }
    let level = body.len() - body.trim_start_matches('?').len();
    let rest = &body[level..];
    let token = if level == 0 {
        Token::Sym(body.to_string())
    } else if let Some(name) = rest.strip_prefix("..") {
        if name.is_empty() {
            return Err(invalid());
        }
        Token::SeqVar { level, name: name.to_string() }
    } else {
        if rest.is_empty() {
            return Err(invalid());
        }
        Token::Var { level, name: rest.to_string() }
    };
    Ok(Lexeme::Token { token, colon })
}

/// 1 行の字句列をノードにする。戻り値の bool は、行のノードの最後の子が `:` で付いたかどうか。
/// 括弧が深く入れ子になってもスタックを使わないよう、開いている括弧ごとの並びを積み、`)` で畳んで親の並びに加える。
fn build_line(lexemes: Vec<Lexeme>, line_no: usize) -> Result<(Node, bool), ParseError> {
    let err = |kind| ParseError { line: line_no, kind };
    // 先頭は括弧の外の並び
    let mut stack: Vec<Vec<Item>> = vec![Vec::new()];
    for lexeme in lexemes {
        match lexeme {
            Lexeme::Token { token, colon } => {
                let node = Node::leaf(token);
                stack.last_mut().unwrap().push(Item { node, group: false, colon, colon_last: false });
            }
            Lexeme::Open => stack.push(Vec::new()),
            Lexeme::Close { colon } => {
                if stack.len() == 1 {
                    return Err(err(ParseErrorKind::UnmatchedParen));
                }
                let items = stack.pop().unwrap();
                if items.is_empty() {
                    return Err(err(ParseErrorKind::EmptyGroup));
                }
                let (node, colon_last) = build_seq(items, line_no)?;
                stack.last_mut().unwrap().push(Item { node, group: true, colon, colon_last });
            }
        }
    }
    if stack.len() > 1 {
        return Err(err(ParseErrorKind::UnclosedParen));
    }
    build_seq(stack.pop().unwrap(), line_no)
}

/// 空でない並びをノードにする。`:` の付いた要素より右側全体が、最後の子になる。要素が 1 つだけならその要素自身になる。
/// 戻り値の bool は、できたノードの最後の子が `:` で付いたかどうか（`Item::colon_last`）。
/// `s: s: … z` のような長い連鎖でもスタックを使わないよう、`:` で区切った組を右から順に畳む。
fn build_seq(items: Vec<Item>, line_no: usize) -> Result<(Node, bool), ParseError> {
    let err = |kind| ParseError { line: line_no, kind };
    // 各組は「ラベル + 子」で、最後の組以外は `:` の付いた要素で終わる
    let mut groups: Vec<Vec<Item>> = vec![Vec::new()];
    let mut last_colon = false;
    for item in items {
        if last_colon {
            groups.push(Vec::new());
        }
        last_colon = item.colon;
        groups.last_mut().unwrap().push(item);
    }
    if last_colon {
        return Err(err(ParseErrorKind::DanglingColon));
    }
    let mut nested: Option<(Node, bool)> = None;
    for group in groups.into_iter().rev() {
        let mut group = group.into_iter();
        let head = group.next().unwrap();
        let mut children: Vec<Node> = group.map(|item| item.node).collect();
        // 右の組があれば、それは `:` で付く最後の子になる
        let colon_last = nested.is_some();
        children.extend(nested.take().map(|(node, _)| node));
        if children.is_empty() {
            nested = Some((head.node, head.colon_last));
            continue;
        }
        if head.group {
            return Err(err(ParseErrorKind::GroupLabel));
        }
        let node = Node::branch(head.node.token.clone(), children);
        check_label(&node, line_no)?;
        nested = Some((node, colon_last));
    }
    Ok(nested.unwrap())
}

/// 子を持つノードのラベルがシンボルか変数であることを確かめる。
/// 変数を許すのは、パターンのラベル位置に置く変数（`?f ...`）や、ルールが生成するそれを書けるようにするため。
fn check_label(node: &Node, line_no: usize) -> Result<(), ParseError> {
    if node.is_leaf() || node.token.can_be_label() {
        return Ok(());
    }
    let kind = match node.token {
        Token::Str(_) => ParseErrorKind::StringLabel,
        Token::SeqVar { .. } => ParseErrorKind::SeqVarLabel,
        Token::Sym(_) | Token::Var { .. } => unreachable!("can_be_label が真になるトークン"),
    };
    Err(ParseError { line: line_no, kind })
}

/// インデントの段数に従って、行を親子関係のある 1 本の木に組み立てる。
/// 深くインデントされたソース（トレースに出る深い木など）でもスタックを使わないよう、
/// まだ子ブロックが続きうる行を `open` に積んで組み立てる。`open[d]` は深さ d の行。
fn assemble(lines: Vec<Line>) -> Result<Node, ParseError> {
    let mut open: Vec<Line> = Vec::new();
    let mut root: Option<Node> = None;
    for line in lines {
        if line.depth > open.len() {
            return Err(ParseError { line: line.line_no, kind: ParseErrorKind::UnexpectedIndent });
        }
        while open.len() > line.depth {
            close_line(&mut open, &mut root)?;
        }
        match open.last() {
            Some(parent) if parent.has_colon => {
                return Err(ParseError { line: line.line_no, kind: ParseErrorKind::ColonWithBlock });
            }
            None if root.is_some() => {
                return Err(ParseError { line: line.line_no, kind: ParseErrorKind::MultipleRoots });
            }
            _ => open.push(line),
        }
    }
    while !open.is_empty() {
        close_line(&mut open, &mut root)?;
    }
    root.ok_or(ParseError { line: 1, kind: ParseErrorKind::Empty })
}

/// 子ブロックが終わった最も深い行を閉じ、親の子に加える。親がなければ根にする。
fn close_line(open: &mut Vec<Line>, root: &mut Option<Node>) -> Result<(), ParseError> {
    let line = open.pop().unwrap();
    check_label(&line.node, line.line_no)?;
    match open.last_mut() {
        Some(parent) => parent.node.children.push(line.node),
        None => *root = Some(line.node),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(name: &str) -> Node {
        Node::leaf(Token::sym(name))
    }

    fn br(name: &str, children: Vec<Node>) -> Node {
        Node::branch(Token::sym(name), children)
    }

    fn var(level: usize, name: &str) -> Node {
        Node::leaf(Token::Var { level, name: name.into() })
    }

    fn kind(src: &str) -> ParseErrorKind {
        parse(src).unwrap_err().kind
    }

    #[test]
    fn inline_tokens_become_leaf_children() {
        assert_eq!(parse("add z ?n").unwrap(), br("add", vec![sym("z"), var(1, "n")]));
    }

    #[test]
    fn colon_nests_the_rest_of_the_line() {
        assert_eq!(parse("s: add ?m ?n").unwrap(), br("s", vec![br("add", vec![var(1, "m"), var(1, "n")])]));
        assert_eq!(parse("s: s: s z").unwrap(), br("s", vec![br("s", vec![br("s", vec![sym("z")])])]));
        assert_eq!(parse("a b: c d").unwrap(), br("a", vec![sym("b"), br("c", vec![sym("d")])]));
    }

    #[test]
    fn colon_can_follow_a_string() {
        let t = parse("++ \"|\": show ?n").unwrap();
        assert_eq!(t, br("++", vec![Node::leaf(Token::Str("|".into())), br("show", vec![var(1, "n")])]));
    }

    #[test]
    fn block_children_follow_inline_children() {
        let t = parse("main a\n  b\n  c d\n    e\n").unwrap();
        assert_eq!(t, br("main", vec![sym("a"), sym("b"), br("c", vec![sym("d"), sym("e")])]));
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let t = parse("# 先頭のコメント\nmain\n\n  ->   # ルール\n    greet\n    hello\n").unwrap();
        assert_eq!(t, br("main", vec![br("->", vec![sym("greet"), sym("hello")])]));
    }

    #[test]
    fn deep_colon_chain_and_deep_indentation_parse_without_overflow() {
        let depth = 100_000;
        let t = parse(&format!("{}z", "s: ".repeat(depth))).unwrap();
        assert_eq!(t.get(&vec![0; depth]), &sym("z"));
        // インデントの総文字数は深さの 2 乗になるので、こちらは浅めにする
        let depth = 3_000;
        let indented: String = (0..depth).map(|d| format!("{}f a\n", "  ".repeat(d))).collect();
        let t = parse(&indented).unwrap();
        assert_eq!(t.get(&vec![1; depth - 1]), &br("f", vec![sym("a")]));
    }

    #[test]
    fn closing_lines_attach_to_the_right_parents() {
        let t = parse("r\n  a\n    b\n  c\nr2").unwrap_err();
        assert_eq!(t, ParseError { line: 5, kind: ParseErrorKind::MultipleRoots });
        let t = parse("r\n  a\n    b\n  c\n").unwrap();
        assert_eq!(t, br("r", vec![br("a", vec![sym("b")]), sym("c")]));
    }

    #[test]
    fn leading_bom_is_ignored() {
        assert_eq!(parse("\u{feff}# コメント\nmain a").unwrap(), br("main", vec![sym("a")]));
    }

    #[test]
    fn cr_and_bom_are_only_allowed_in_strings_and_comments() {
        assert_eq!(kind("s a\r b"), ParseErrorKind::Invisible('\r'));
        assert_eq!(kind("s a\u{feff}"), ParseErrorKind::Invisible('\u{feff}'));
        assert_eq!(kind("main\n  \u{feff}a"), ParseErrorKind::Invisible('\u{feff}'));
        assert_eq!(kind("s \"a\"\r b"), ParseErrorKind::Invisible('\r'));
        let t = parse("s \"a\rb\u{feff}\" # \r\u{feff}").unwrap();
        assert_eq!(t, br("s", vec![Node::leaf(Token::Str("a\rb\u{feff}".into()))]));
        assert_eq!(parse("main\r\n  a\r\n").unwrap(), br("main", vec![sym("a")]));
    }

    #[test]
    fn hash_inside_a_symbol_is_not_a_comment() {
        assert_eq!(parse("a b#c").unwrap(), br("a", vec![sym("b#c")]));
    }

    #[test]
    fn variables_carry_levels() {
        let t = parse("f ?x ??y ?..xs ??..ys").unwrap();
        assert_eq!(
            t.children,
            vec![
                var(1, "x"),
                var(2, "y"),
                Node::leaf(Token::SeqVar { level: 1, name: "xs".into() }),
                Node::leaf(Token::SeqVar { level: 2, name: "ys".into() }),
            ]
        );
    }

    #[test]
    fn label_variable_is_allowed() {
        assert_eq!(parse("?f ??x").unwrap(), Node::branch(Token::Var { level: 1, name: "f".into() }, vec![var(2, "x")]));
    }

    #[test]
    fn string_escapes_are_decoded() {
        let t = parse(r#"s "a\"b\\c\nd""#).unwrap();
        assert_eq!(t.children[0], Node::leaf(Token::Str("a\"b\\c\nd".into())));
    }

    #[test]
    fn parens_put_a_subtree_in_any_child_position() {
        assert_eq!(parse("add (s: s z) (s z)").unwrap(), parse("add\n  s: s z\n  s z").unwrap());
        let t = parse("f (g (h a) b) c").unwrap();
        assert_eq!(t, br("f", vec![br("g", vec![br("h", vec![sym("a")]), sym("b")]), sym("c")]));
    }

    #[test]
    fn single_item_parens_are_the_item_itself() {
        assert_eq!(parse("(a)").unwrap(), sym("a"));
        assert_eq!(parse("f (x)").unwrap(), parse("f x").unwrap());
        assert_eq!(parse("s: (a b)").unwrap(), parse("s: a b").unwrap());
        assert_eq!(parse("(f a b)").unwrap(), parse("f a b").unwrap());
    }

    #[test]
    fn colon_inside_parens_ends_at_the_close() {
        assert_eq!(parse("a (b: c d) e").unwrap(), br("a", vec![br("b", vec![br("c", vec![sym("d")])]), sym("e")]));
        assert_eq!(parse("a (b c): d e").unwrap(), br("a", vec![br("b", vec![sym("c")]), br("d", vec![sym("e")])]));
    }

    #[test]
    fn parens_delimit_tokens_and_strings() {
        assert_eq!(parse("f(g x)").unwrap(), parse("f (g x)").unwrap());
        let t = parse("f (g \"a\") \"b\"").unwrap();
        assert_eq!(t, br("f", vec![Node::branch(Token::sym("g"), vec![Node::leaf(Token::Str("a".into()))]), Node::leaf(Token::Str("b".into()))]));
        assert_eq!(parse("-> (-> ?p ?r) ?r").unwrap(), parse("->\n  -> ?p ?r\n  ?r").unwrap());
    }

    #[test]
    fn colon_only_inside_parens_allows_a_block() {
        assert_eq!(parse("a (b c)\n  d").unwrap(), br("a", vec![br("b", vec![sym("c")]), sym("d")]));
        assert_eq!(parse("a (b: c)\n  d").unwrap(), br("a", vec![br("b", vec![sym("c")]), sym("d")]));
        assert_eq!(kind("a (b c): d\n  e"), ParseErrorKind::ColonWithBlock);
        // 行全体を囲む括弧は外れて `s: z` と同じになるので、`z` の後ろに子を付けられない
        assert_eq!(kind("(s: z)\n  y"), ParseErrorKind::ColonWithBlock);
        assert_eq!(kind("((s: z))\n  y"), ParseErrorKind::ColonWithBlock);
        assert_eq!(parse("(a b)\n  c").unwrap(), br("a", vec![sym("b"), sym("c")]));
    }

    #[test]
    fn deep_parens_parse_without_overflow() {
        let depth = 100_000;
        let src = format!("{}z{}", "(s ".repeat(depth), ")".repeat(depth));
        let t = parse(&src).unwrap();
        assert_eq!(t.get(&vec![0; depth]), &sym("z"));
    }

    #[test]
    fn paren_needs_no_delimiter_after() {
        assert_eq!(parse("f (a)b").unwrap(), parse("f a b").unwrap());
        assert_eq!(parse("f (a b)(c d)").unwrap(), br("f", vec![br("a", vec![sym("b")]), br("c", vec![sym("d")])]));
        assert_eq!(parse("f (a b):c d").unwrap(), parse("f (a b): c d").unwrap());
        assert_eq!(parse("f (a)\"s\"").unwrap(), br("f", vec![sym("a"), Node::leaf(Token::Str("s".into()))]));
    }

    #[test]
    fn paren_errors_are_reported() {
        assert_eq!(parse("main\n  f (a b").unwrap_err(), ParseError { line: 2, kind: ParseErrorKind::UnclosedParen });
        assert_eq!(kind("f (a # c)"), ParseErrorKind::UnclosedParen);
        assert_eq!(kind("f a)"), ParseErrorKind::UnmatchedParen);
        assert_eq!(kind("f ()"), ParseErrorKind::EmptyGroup);
        assert_eq!(kind("(a b) c"), ParseErrorKind::GroupLabel);
        assert_eq!(kind("f: (a b) c"), ParseErrorKind::GroupLabel);
        assert_eq!(kind("f (a:)"), ParseErrorKind::DanglingColon);
        assert_eq!(kind("f (\"a\" b)"), ParseErrorKind::StringLabel);
    }

    #[test]
    fn errors_are_reported_with_line_numbers() {
        assert_eq!(parse("main\n\tx").unwrap_err(), ParseError { line: 2, kind: ParseErrorKind::Tab });
        assert_eq!(kind("main\n   x"), ParseErrorKind::BadIndent);
        assert_eq!(kind("main\n   \u{3000}"), ParseErrorKind::BadIndent);
        assert_eq!(kind("main\n    x"), ParseErrorKind::UnexpectedIndent);
        assert_eq!(kind("  main"), ParseErrorKind::UnexpectedIndent);
        assert_eq!(kind("a\nb"), ParseErrorKind::MultipleRoots);
        assert_eq!(kind("# だけ\n"), ParseErrorKind::Empty);
        assert_eq!(kind("s \"abc"), ParseErrorKind::UnterminatedString);
        assert_eq!(kind(r#"s "\t""#), ParseErrorKind::BadEscape('t'));
        assert_eq!(kind("s \"a\"b"), ParseErrorKind::TrailingAfterString);
        assert_eq!(kind("s a:"), ParseErrorKind::DanglingColon);
        assert_eq!(kind("s: a\n  b"), ParseErrorKind::ColonWithBlock);
        assert_eq!(kind("s ?"), ParseErrorKind::InvalidToken("?".into()));
        assert_eq!(kind("s :"), ParseErrorKind::InvalidToken(":".into()));
        assert_eq!(kind("s :a"), ParseErrorKind::InvalidToken(":a".into()));
        assert_eq!(kind("s ::"), ParseErrorKind::InvalidToken("::".into()));
        assert_eq!(kind("s a:: b"), ParseErrorKind::InvalidToken("a::".into()));
        assert_eq!(kind("s ?x:: b"), ParseErrorKind::InvalidToken("?x::".into()));
        assert_eq!(kind("s ?..xs:: b"), ParseErrorKind::InvalidToken("?..xs::".into()));
        assert_eq!(kind("s \"a\"\tb"), ParseErrorKind::Tab);
        assert_eq!(kind("s \"a\":\tb"), ParseErrorKind::Tab);
        assert_eq!(kind("\"a\" b"), ParseErrorKind::StringLabel);
        assert_eq!(kind("?..xs\n  b"), ParseErrorKind::SeqVarLabel);
    }
}
