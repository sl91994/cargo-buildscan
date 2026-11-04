use std::process::ExitCode;

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
    Err(Error::Unimplemented("scan"))
}
