//! docs/turing-completeness.md の翻訳規則 T を確かめる。
//! T で作った Foldage プログラムの実行結果を、タグシステムを直接動かした結果と突き合わせる。

use std::fmt::Write;

use foldage::engine::{run, Outcome};
use foldage::parser::parse;

/// 2-タグシステム。組の 1 つ目が記号、2 つ目がその生成規則（語の末尾に足す記号の列）。
struct TagSystem {
    productions: Vec<(&'static str, Vec<&'static str>)>,
}

impl TagSystem {
    fn production(&self, x: &str) -> &[&'static str] {
        let (_, p) = self.productions.iter().find(|(y, _)| *y == x).expect("生成規則のない記号");
        p
    }
}

/// De Mol の Collatz 系。語 a^n が Collatz の列に沿って縮み、a^1 で止まる。
fn collatz() -> TagSystem {
    TagSystem { productions: vec![("a", vec!["b", "c"]), ("b", vec!["a"]), ("c", vec!["a", "a", "a"])] }
}

/// タグシステムを直接動かす。止まったら、そのときの語とステップ数を返す。`max_steps` で止まらなければ `None`。
fn simulate(sys: &TagSystem, word: &[&'static str], max_steps: usize) -> Option<(Vec<&'static str>, usize)> {
    let mut word = word.to_vec();
    let mut steps = 0;
    while word.len() >= 2 {
        if steps == max_steps {
            return None;
        }
        let p = sys.production(word[0]);
        word.drain(..2);
        word.extend_from_slice(p);
        steps += 1;
    }
    Some((word, steps))
}

/// 翻訳規則 T に従って、タグシステムと初期語から Foldage のソースを作る。
/// 記号名は文字列リテラルにそのまま埋め込むので、`\` を含まないこと（文書の記号名の制約と同じ）。
fn compile(sys: &TagSystem, word: &[&str]) -> String {
    let mut src = String::from("main\n");
    for (x, p) in &sys.productions {
        writeln!(src, "  ->\n    tag {x} ?y ?..w\n    {}", line("tag ?..w", p)).unwrap();
    }
    src.push_str("  -> tag \"\"\n  ->\n    tag ?x\n    show ?x\n");
    for (x, _) in &sys.productions {
        writeln!(src, "  ->\n    show {x}\n    \"{x}\"").unwrap();
    }
    src.push_str("  ->\n    main ?..rules ?r\n    ?r\n");
    writeln!(src, "  {}", line("tag", word)).unwrap();
    src
}

fn line(head: &str, symbols: &[&str]) -> String {
    [&[head], symbols].concat().join(" ")
}

/// Foldage で実行して、出力とステップ数を返す。
fn execute(src: &str) -> (String, usize) {
    let mut steps = 0;
    let outcome = run(parse(src).unwrap(), Some(100_000), &mut |n, _| steps = n);
    let Outcome::Folded(text) = outcome else { panic!("畳み切れなかった: {outcome:?}") };
    (text, steps)
}

/// T の補題どおりなら、Foldage はタグのステップに加えて、停止処理（語が空なら 1、そうでなければ 2）と main の折りたたみ 1 だけ進む。
fn assert_simulates(sys: &TagSystem, word: &[&'static str]) {
    let (halted, tag_steps) = simulate(sys, word, 100_000).expect("止まるはずの系");
    let tail = if halted.is_empty() { 1 } else { 2 } + 1;
    assert_eq!(execute(&compile(sys, word)), (halted.concat(), tag_steps + tail), "初期語 {word:?}");
}

#[test]
fn collatz_system_is_simulated_step_by_step() {
    for n in 1..=12 {
        assert_simulates(&collatz(), &vec!["a"; n]);
    }
}

#[test]
fn halting_on_the_empty_word_and_empty_production_are_simulated() {
    let sys = TagSystem { productions: vec![("a", vec![]), ("b", vec!["a", "a", "a"])] };
    // b a a -> a a a a -> a a -> （空）
    assert_eq!(simulate(&sys, &["b", "a", "a"], 10), Some((vec![], 3)));
    assert_simulates(&sys, &["b", "a", "a"]);
    assert_simulates(&sys, &[]);
}

#[test]
fn non_halting_system_never_folds() {
    // a a -> a a -> ... と同じ語を繰り返す
    let sys = TagSystem { productions: vec![("a", vec!["a", "a"])] };
    assert_eq!(simulate(&sys, &["a", "a"], 1000), None);
    let outcome = run(parse(&compile(&sys, &["a", "a"])).unwrap(), Some(1000), &mut |_, _| {});
    assert!(matches!(outcome, Outcome::StepLimit(_)));
}

#[test]
fn sample_is_an_instance_of_the_translation() {
    let sample = parse(include_str!("../samples/ex5-tag.fdg")).unwrap();
    assert_eq!(parse(&compile(&collatz(), &["a"; 7])).unwrap(), sample);
}
