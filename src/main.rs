use std::process::ExitCode;

use cargo_buildscan::cli::{self, Command};
use cargo_buildscan::error::{Error, Result};

const EXIT_ERROR: u8 = 2;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn run() -> Result<ExitCode> {
    match cli::parse_args(std::env::args_os().skip(1))? {
        Command::Help => {
            print!("{}", cli::HELP);
            Ok(ExitCode::SUCCESS)
        }
        Command::Version => {
            println!("cargo-buildscan {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        // MEMO: 未実装のまま 0 (reported なし) を返すと CI で素通りするため，エラー (2) にする
        Command::Scan(_) => Err(Error::Unimplemented("scan")),
    }
}
