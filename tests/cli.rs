//! 処理系のバイナリを起動して、--burn によるソースの上書きを確かめる。
//! 元のサンプルを壊さないよう、一時ディレクトリにコピーしたファイルだけを上書きさせる。

use std::path::PathBuf;
use std::process::{Command, Output};

use foldage::engine::{run, Outcome};
use foldage::parser::parse;
use foldage::printer::to_source;

const EX1: &str = include_str!("../samples/ex1-add.fdg");

/// テストごとに別の一時ファイルを作り、`src` を書いておく。テストは並行に走るので `name` で区別する。
fn scratch_file(name: &str, src: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("foldage-cli-{}-{name}.fdg", std::process::id()));
    std::fs::write(&path, src).unwrap();
    path
}

/// 処理系を `path` と `args` で起動する。
fn foldage(path: &PathBuf, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_foldage")).arg(path).args(args).output().unwrap()
}

fn read(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// 書き戻しで使う隣のファイル `<path>.burn`。
fn burn_tmp(path: &PathBuf) -> PathBuf {
    PathBuf::from(format!("{}.burn", path.display()))
}

#[test]
fn burn_replaces_source_with_folded_atom() {
    let path = scratch_file("folded", EX1);
    let out = foldage(&path, &["--burn"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"|||");
    // 出力の字面ではなく、文字列のアトムとして書き戻す
    assert_eq!(read(&path), "\"|||\"\n");
    assert!(!burn_tmp(&path).exists());
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn burned_atom_reruns_to_same_output() {
    let path = scratch_file("rerun", EX1);
    foldage(&path, &["--burn"]);
    let out = foldage(&path, &["--burn"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"|||");
    assert_eq!(read(&path), "\"|||\"\n");
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn burn_writes_stuck_tree() {
    let path = scratch_file("stuck", "main a b");
    let out = foldage(&path, &["--burn"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert_eq!(read(&path), "main a b\n");
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn burn_at_step_limit_can_resume() {
    let path = scratch_file("limit", EX1);
    let out = foldage(&path, &["--max-steps", "5", "--burn"]);
    assert_eq!(out.status.code(), Some(1));
    let Outcome::StepLimit(expected) = run(parse(EX1).unwrap(), Some(5), &mut |_, _| {}) else { unreachable!() };
    assert_eq!(read(&path), to_source(&expected));

    // 書き戻した木から続きを実行すると、最初から実行したのと同じ出力になる
    let out = foldage(&path, &[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"|||");
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn source_is_untouched_without_burn() {
    let path = scratch_file("no-burn", EX1);
    let out = foldage(&path, &[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(read(&path), EX1);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn syntax_error_leaves_source_untouched() {
    // タブは文字列リテラルとコメントの外では使えない
    let src = "main\n\tx\n";
    let path = scratch_file("syntax", src);
    let out = foldage(&path, &["--burn"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(read(&path), src);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn burn_failure_exits_2_without_output() {
    let path = scratch_file("blocked-folded", EX1);
    // 読み取り専用にしても root なら書き込めてしまうので、一時ファイルの位置にディレクトリを置いて失敗させる
    std::fs::create_dir(burn_tmp(&path)).unwrap();

    let out = foldage(&path, &["--burn"]);
    // 上書きに失敗したら、畳み切れていても出力しない
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert_eq!(read(&path), EX1);

    std::fs::remove_dir(burn_tmp(&path)).unwrap();
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn burn_failure_still_reports_stuck_tree() {
    let path = scratch_file("blocked-stuck", "main a b");
    std::fs::create_dir(burn_tmp(&path)).unwrap();

    let out = foldage(&path, &["--burn"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr).unwrap().contains("main a b\n"));
    assert_eq!(read(&path), "main a b");

    std::fs::remove_dir(burn_tmp(&path)).unwrap();
    std::fs::remove_file(&path).unwrap();
}
