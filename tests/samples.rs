//! samples/ のサンプルが、資料どおりのステップ数と出力になることを確かめる。

use foldage::engine::{run, Outcome};
use foldage::parser::parse;
use foldage::printer::to_source;

/// 実行して、出力とステップ数と各ステップの木（正規形）を返す。
fn execute(src: &str) -> (String, usize, Vec<String>) {
    let mut trace = Vec::new();
    let outcome = run(parse(src).unwrap(), Some(1000), &mut |_, tree| trace.push(to_source(tree)));
    let Outcome::Folded(tree) = outcome else { panic!("畳み切れなかった: {outcome:?}") };
    let text = tree.atom_text().unwrap().to_string();
    (text, trace.len() - 1, trace)
}

/// トレースの各ステップが、そのまま読み戻せる正規形になっていることを確かめる。
fn assert_trace_is_source(trace: &[String]) {
    for step in trace {
        assert_eq!(&to_source(&parse(step).unwrap()), step);
    }
}

#[test]
fn ex1_add_outputs_three_bars_in_11_steps() {
    let (out, steps, trace) = execute(include_str!("../samples/ex1-add.fdg"));
    assert_eq!(out, "|||");
    assert_eq!(steps, 11);
    // how-it-works.html の例1 の step 2、3、7、10 の主要部分
    assert!(trace[2].ends_with("  show: s: s: add z: s z\n"));
    assert!(trace[3].ends_with("  show: s: s: s z\n"));
    assert!(trace[7].ends_with("  ++ \"|\": ++ \"|\": ++ \"|\" \"\"\n"));
    assert!(trace[10].ends_with("  \"|||\"\n"));
}

#[test]
fn ex2_meta_matches_the_documented_trace() {
    let (out, steps, trace) = execute(include_str!("../samples/ex2-meta.fdg"));
    assert_eq!(out, "goodbye");
    assert_eq!(steps, 3);
    let rules_after = "  -> greet goodbye\n  ->\n    -> greet hello\n    -> greet goodbye\n  ->\n    main ?..rules ?r\n    ?r\n";
    assert_eq!(trace[1], format!("main\n{rules_after}  greet\n"));
    assert_eq!(trace[2], format!("main\n{rules_after}  goodbye\n"));
    assert_eq!(trace[3], "goodbye\n");
}

#[test]
fn ex3_generated_rule_folds_id_foo() {
    let (out, steps, trace) = execute(include_str!("../samples/ex3-mkid.fdg"));
    assert_eq!(out, "foo");
    assert_eq!(steps, 3);
    assert!(trace[1].ends_with("  ->\n    id ?x\n    ?x\n  id foo\n"));
}

#[test]
fn ex4_rev_outputs_cba_in_8_steps() {
    let (out, steps, trace) = execute(include_str!("../samples/ex4-rev.fdg"));
    assert_eq!(out, "cba");
    assert_eq!(steps, 8);
    assert!(trace[3].ends_with("  ++\n    ++\n      ++ rev \"c\"\n      \"b\"\n    \"a\"\n"));
    assert!(trace[4].ends_with("  ++\n    ++\n      ++ \"\" \"c\"\n      \"b\"\n    \"a\"\n"));
}

#[test]
fn ex5_collatz_tag_outputs_a_in_131_steps() {
    let (out, steps, trace) = execute(include_str!("../samples/ex5-tag.fdg"));
    assert_eq!(out, "a");
    // タグの 128 ステップ + 停止処理 2 + main の折りたたみ 1
    assert_eq!(steps, 131);
    assert!(trace[1].ends_with("  tag a a a a a b c\n"));
    assert!(trace[128].ends_with("  tag a\n"));
    assert!(trace[129].ends_with("  show a\n"));
}

/// 書き方でステップ数が大きく変わるので、出力と正規形だけを確かめる。
#[test]
fn ex6_fizzbuzz_outputs_1_to_15() {
    let (out, _, trace) = execute(include_str!("../samples/ex6-fizzbuzz.fdg"));
    assert_eq!(out, "1\n2\nFizz\n4\nBuzz\nFizz\n7\n8\nFizz\nBuzz\n11\nFizz\n13\n14\nFizzBuzz\n");
    // ex6 が未完成の間に ex1〜ex5 の検査を巻き込まないよう、every_trace_step_is_valid_source とは分けている
    assert_trace_is_source(&trace);
}

#[test]
fn every_trace_step_is_valid_source() {
    for src in [
        include_str!("../samples/ex1-add.fdg"),
        include_str!("../samples/ex2-meta.fdg"),
        include_str!("../samples/ex3-mkid.fdg"),
        include_str!("../samples/ex4-rev.fdg"),
        include_str!("../samples/ex5-tag.fdg"),
    ] {
        let (_, _, trace) = execute(src);
        assert_trace_is_source(&trace);
    }
}
