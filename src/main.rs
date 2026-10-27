use std::io::Write;
use std::process::ExitCode;

use foldage::engine::{run, Outcome};
use foldage::parser::parse;
use foldage::printer::to_source;
use foldage::tree::Node;

const USAGE: &str = "使い方: foldage <ファイル> [--trace] [--max-steps N] [--burn]";

struct Options {
    path: String,
    trace: bool,
    max_steps: Option<usize>,
    /// 止まった時点の木で、ソースファイルを上書きする。
    burn: bool,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut path = None;
    let mut trace = false;
    let mut max_steps = None;
    let mut burn = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--trace" => trace = true,
            "--burn" => burn = true,
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
    Ok(Options { path, trace, max_steps, burn })
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
    let outcome = run(root, opts.max_steps, &mut trace);
    // 失敗の報告と書き戻しで同じ正規形を使い回す。深い木では書き出しが一番重い
    let source = to_source(outcome.tree());
    let failure = match outcome {
        Outcome::Folded(_) => None,
        Outcome::Stuck(_) => Some("折りたたみ失敗: 書き換えられる箇所がなくなりましたが、根が単一のアトムになっていません"),
        Outcome::StepLimit(_) => Some("ステップ数の上限に達したので止めました"),
    };
    let burned = if opts.burn { burn(&opts.path, &source) } else { Ok(()) };
    // 書き戻せなくても、止まった木は失わないよう報告する
    if let Some(msg) = failure {
        eprintln!("{msg}");
        eprint!("{source}");
    }
    match (burned, failure) {
        // 出力だけ済んで書き戻せていない状態を避けるため、正常終了でも出力しない
        (Err(msg), _) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
        (Ok(()), Some(_)) => ExitCode::from(1),
        (Ok(()), None) => {
            let text = outcome.tree().atom_text().expect("Folded の木はアトム");
            // 出力は根のアトムそのものなので、改行は足さない
            let mut out = std::io::stdout();
            let _ = out.write_all(text.as_bytes());
            let _ = out.flush();
            ExitCode::SUCCESS
        }
    }
}

/// 止まった時点の木の正規形 `source` で、ソースファイル `path` を上書きする。
/// 書き出したものはそのまま実行でき、ステップ上限で止まった木なら続きから再開できる。
/// 失敗は終了コード 2 にするので、パス付きのメッセージだけを返す。
fn burn(path: &str, source: &str) -> Result<(), String> {
    // 書き込みが途中で失敗しても元のソースを壊さないよう、`<path>.burn` に書き切ってから置き換える
    let tmp = format!("{path}.burn");
    std::fs::write(&tmp, source).and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{path}: 書き込めません: {e}")
    })
}
