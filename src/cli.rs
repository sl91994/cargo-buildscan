use std::ffi::OsString;
use std::path::PathBuf;

use lexopt::prelude::*;

use crate::error::Result;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScanOpts {
    pub manifest_path: Option<PathBuf>,
    pub locked: bool,
    pub offline: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Scan(ScanOpts),
    Help,
    Version,
}

pub const HELP: &str = "\
Scan build scripts in the dependency graph

Usage: cargo buildscan [OPTIONS]

Options:
      --manifest-path <PATH>  Path to Cargo.toml
      --locked                Require Cargo.lock to be up to date
      --offline               Run without accessing the network
  -h, --help                  Print help
  -V, --version               Print version
";

/// `args` はプログラム名を含まない引数列 (`std::env::args_os().skip(1)`) を受け取る
pub fn parse_args<I>(args: I) -> Result<Command>
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    let mut args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    // cargo buildscan 経由なら先頭に buildscan が入る．直接起動なら入らない
    if args.first().is_some_and(|a| a == "buildscan") {
        args.remove(0);
    }

    let mut parser = lexopt::Parser::from_args(args);
    let mut opts = ScanOpts::default();

    while let Some(arg) = parser.next()? {
        match arg {
            Long("manifest-path") => opts.manifest_path = Some(parser.value()?.into()),
            Long("locked") => opts.locked = true,
            Long("offline") => opts.offline = true,

            Short('h') | Long("help") => return Ok(Command::Help),
            Short('V') | Long("version") => return Ok(Command::Version),

            _ => return Err(arg.unexpected().into()),
        }
    }

    Ok(Command::Scan(opts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn skips_cargo_subcommand_name() {
        let cmd = parse_args(["buildscan", "--locked"]).unwrap();
        assert_eq!(cmd, Command::Scan(ScanOpts { locked: true, ..Default::default() }));
    }

    #[test]
    fn works_without_cargo_prefix() {
        assert!(matches!(parse_args(["--offline"]).unwrap(), Command::Scan(_)));
    }

    #[test]
    fn unknown_option_and_positional_are_errors() {
        assert!(parse_args(["buildscan", "--frobnicate"]).is_err());
        assert!(parse_args(["buildscan", "extra"]).is_err());
        assert!(parse_args(["buildscan", "--locked=yes"]).is_err());
    }

    /// v0.2 で導入する機能の引数は，v0.1 では未知の引数として扱う
    #[test]
    fn deferred_features_are_usage_errors() {
        let cases: [&[&str]; 4] = [
            &["buildscan", "approve"],
            &["buildscan", "--format", "json"],
            &["buildscan", "--baseline=b.toml"],
            &["buildscan", "--package", "serde"],
        ];

        for args in cases {
            assert!(matches!(parse_args(args), Err(Error::Usage(_))), "{args:?}");
        }
    }

    #[test]
    fn path_options_take_values() {
        let cmd =
            parse_args(["buildscan", "--manifest-path", "a/Cargo.toml", "--offline"]).unwrap();
        assert_eq!(
            cmd,
            Command::Scan(ScanOpts {
                manifest_path: Some("a/Cargo.toml".into()),
                offline: true,
                ..Default::default()
            })
        );
    }

    #[test]
    fn missing_value_is_usage_error() {
        assert!(matches!(parse_args(["buildscan", "--manifest-path"]), Err(Error::Usage(_))));
    }

    #[test]
    fn help_and_version() {
        assert_eq!(parse_args(["buildscan", "--locked", "-h"]).unwrap(), Command::Help);
        assert_eq!(parse_args(["buildscan", "--version"]).unwrap(), Command::Version);
    }
}
