use std::ffi::OsString;
use std::path::PathBuf;

use lexopt::prelude::*;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Human,
    Json,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScanOpts {
    pub manifest_path: Option<PathBuf>,
    pub format: Format,
    pub baseline: Option<PathBuf>,
    pub locked: bool,
    pub offline: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Scan(ScanOpts),
    Approve { opts: ScanOpts, packages: Vec<String> },
    Help { approve: bool },
    Version,
}

pub const HELP: &str = "\
Scan build scripts in the dependency graph

Usage: cargo buildscan [OPTIONS]
       cargo buildscan approve [OPTIONS] [--package <NAME>]...

Commands:
  approve                 Record the current build scripts as reviewed

Options:
      --manifest-path <PATH>  Path to Cargo.toml
      --format <human|json>   Output format [default: human]
      --baseline <PATH>       Baseline file [default: buildscan-baseline.toml if it exists]
      --locked                Require Cargo.lock to be up to date
      --offline               Run without accessing the network
  -h, --help                  Print help
  -V, --version               Print version
";

pub const APPROVE_HELP: &str = "\
Record the current build scripts as reviewed

Usage: cargo buildscan approve [OPTIONS] [--package <NAME>]...

Options:
      --package <NAME>        Approve only this package (repeatable)
      --manifest-path <PATH>  Path to Cargo.toml
      --format <human|json>   Output format [default: human]
      --baseline <PATH>       Baseline file [default: buildscan-baseline.toml]
      --locked                Require Cargo.lock to be up to date
      --offline               Run without accessing the network
  -h, --help                  Print help
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
    let mut approve = false;
    let mut packages = Vec::new();

    while let Some(arg) = parser.next()? {
        match arg {
            Long("manifest-path") => opts.manifest_path = Some(parser.value()?.into()),
            Long("format") => opts.format = parse_format(parser.value()?)?,
            Long("baseline") => opts.baseline = Some(parser.value()?.into()),
            Long("locked") => opts.locked = true,
            Long("offline") => opts.offline = true,

            Long("package") => packages.push(parser.value()?.string()?),

            Value(v) if v == "approve" && !approve => approve = true,

            Short('h') | Long("help") => return Ok(Command::Help { approve }),
            Short('V') | Long("version") => return Ok(Command::Version),

            _ => return Err(arg.unexpected().into()),
        }
    }

    if !approve && !packages.is_empty() {
        return Err(Error::Usage("'--package' can only be used with 'approve'".into()));
    }

    Ok(if approve { Command::Approve { opts, packages } } else { Command::Scan(opts) })
}

fn parse_format(v: OsString) -> Result<Format> {
    match v.to_str() {
        Some("human") => Ok(Format::Human),
        Some("json") => Ok(Format::Json),
        _ => Err(Error::Usage(format!(
            "invalid value '{}' for '--format': expected 'human' or 'json'",
            v.to_string_lossy()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn approve_with_multiple_packages_and_equals_form() {
        let cmd = parse_args([
            "buildscan",
            "--offline",
            "approve",
            "--package",
            "serde",
            "--package=proc-macro2",
            "--format=json",
        ])
        .unwrap();
        assert_eq!(
            cmd,
            Command::Approve {
                opts: ScanOpts { offline: true, format: Format::Json, ..Default::default() },
                packages: vec!["serde".into(), "proc-macro2".into()],
            }
        );
    }

    #[test]
    fn package_without_approve_is_error() {
        assert!(parse_args(["buildscan", "--package", "serde"]).is_err());
    }

    #[test]
    fn unknown_option_and_extra_positional_are_errors() {
        assert!(parse_args(["buildscan", "--frobnicate"]).is_err());
        assert!(parse_args(["buildscan", "approve", "approve"]).is_err());
        assert!(parse_args(["buildscan", "--locked=yes"]).is_err());
        assert!(parse_args(["buildscan", "--format", "xml"]).is_err());
    }

    #[test]
    fn path_options_take_values() {
        let cmd = parse_args(["buildscan", "--manifest-path", "a/Cargo.toml", "--baseline=b.toml"])
            .unwrap();
        assert_eq!(
            cmd,
            Command::Scan(ScanOpts {
                manifest_path: Some("a/Cargo.toml".into()),
                baseline: Some("b.toml".into()),
                ..Default::default()
            })
        );
    }

    #[test]
    fn missing_values_are_usage_errors() {
        for opt in ["--manifest-path", "--baseline", "--format"] {
            assert!(matches!(parse_args(["buildscan", opt]), Err(Error::Usage(_))), "{opt}");
        }
        assert!(matches!(parse_args(["buildscan", "approve", "--package"]), Err(Error::Usage(_))));
    }

    #[test]
    fn package_before_approve() {
        let cmd = parse_args(["buildscan", "--package", "serde", "approve"]).unwrap();
        assert_eq!(
            cmd,
            Command::Approve { opts: ScanOpts::default(), packages: vec!["serde".into()] }
        );
    }

    #[test]
    fn invalid_format_message() {
        let Err(Error::Usage(msg)) = parse_args(["buildscan", "--format", "xml"]) else {
            panic!("expected usage error");
        };
        assert!(msg.contains("'xml'"), "{msg}");
    }

    #[test]
    fn approve_help() {
        assert_eq!(
            parse_args(["buildscan", "approve", "--help"]).unwrap(),
            Command::Help { approve: true }
        );
    }
}
