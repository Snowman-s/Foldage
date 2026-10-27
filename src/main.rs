use std::io::Write;
use std::process::ExitCode;

use foldage::engine::{run, Outcome};
use foldage::parser::parse;
use foldage::printer::to_source;
use foldage::tree::Node;

const USAGE: &str = "使い方: foldage <ファイル> [--trace] [--max-steps N]";

struct Options {
    path: String,
    trace: bool,
    max_steps: Option<usize>,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut path = None;
    let mut trace = false;
    let mut max_steps = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--trace" => trace = true,
            "--max-steps" => {
                let n = args.next().ok_or("--max-steps には数を指定してください")?;
                max_steps = Some(n.parse().map_err(|_| format!("--max-steps の値 `{n}` は数ではありません"))?);
            }
            a if a.starts_with("--") => return Err(format!("不明なオプション `{a}`")),
            _ if path.is_some() => return Err("ファイルは 1 つだけ指定してください".to_string()),
            _ => path = Some(arg),
        }
    }
    let path = path.ok_or("ファイルを指定してください")?;
    Ok(Options { path, trace, max_steps })
}

fn main() -> ExitCode {
    // 処理系のバグでパニックしても、終了コードは 0/1/2 の取り決めに収める
    std::panic::catch_unwind(execute).unwrap_or_else(|_| {
        eprintln!("処理系の内部エラーで止まりました");
        ExitCode::from(2)
    })
}

/// 引数を読み、ソースを木にする。失敗はどれも終了コード 2 なので、メッセージだけを返す。
fn load() -> Result<(Options, Node), String> {
    let opts = parse_args(std::env::args().skip(1)).map_err(|e| format!("{e}\n{USAGE}"))?;
    let src = std::fs::read_to_string(&opts.path).map_err(|e| format!("{}: 読み込めません: {e}", opts.path))?;
    let root = parse(&src).map_err(|e| format!("{}: {e}", opts.path))?;
    Ok((opts, root))
}

fn execute() -> ExitCode {
    let (opts, root) = match load() {
        Ok(loaded) => loaded,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };

    // 途中経過は出力と混ざらないよう標準エラーに書く
    let mut trace = |n: usize, tree: &Node| {
        if opts.trace {
            eprint!("{}# step {n}\n{}", if n == 0 { "" } else { "\n" }, to_source(tree));
        }
    };
    match run(root, opts.max_steps, &mut trace) {
        Outcome::Folded(text) => {
            // 出力は根のアトムそのものなので、改行は足さない
            let mut out = std::io::stdout();
            let _ = out.write_all(text.as_bytes());
            let _ = out.flush();
            ExitCode::SUCCESS
        }
        Outcome::Stuck(tree) => fail("折りたたみ失敗: 書き換えられる箇所がなくなりましたが、根が単一のアトムになっていません", &tree),
        Outcome::StepLimit(tree) => fail("ステップ数の上限に達したので止めました", &tree),
    }
}

/// 畳み切れなかった理由と、止まった時点の木を標準エラーに書く。
fn fail(msg: &str, tree: &Node) -> ExitCode {
    eprintln!("{msg}");
    eprint!("{}", to_source(tree));
    ExitCode::from(1)
}
