//! ツール自体のエラー定義 (終了コード 2)
//!
//! `anyhow` / `thiserror` はどちらも build script を持つため使いません (極力 build script を持つ依存関係を減らす)

use std::fmt;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub enum Error {
    /// コマンドライン引数の誤り
    Usage(String),
    /// `cargo metadata` の起動，実行，出力の解釈失敗
    Metadata(cargo_metadata::Error),
    /// TODO: 未実装の機能．スキャンのパイプライン (#31) の実装時に削除します．
    Unimplemented(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage(msg) => write!(f, "{msg}\nRun 'cargo buildscan --help' for usage."),
            // TODO(#34): クレート由来の文字列を含みうるため，エスケープする
            Error::Metadata(cargo_metadata::Error::CargoMetadata { stderr }) => {
                write!(f, "cargo metadata failed:\n{}", stderr.trim_end())
            }
            Error::Metadata(err) => write!(f, "{err}"),
            Error::Unimplemented(what) => write!(f, "{what} is not implemented yet"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Metadata(err) => Some(err),
            _ => None,
        }
    }
}

impl From<cargo_metadata::Error> for Error {
    fn from(err: cargo_metadata::Error) -> Self {
        Error::Metadata(err)
    }
}

impl From<lexopt::Error> for Error {
    fn from(err: lexopt::Error) -> Self {
        Error::Usage(err.to_string())
    }
}
