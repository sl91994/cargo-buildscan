//! ツール自体のエラー定義 (終了コード 2)
//!
//! `anyhow` / `thiserror` はどちらも build script を持つため使いません (極力 build script を持つ依存関係を減らす)

use std::fmt;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub enum Error {
    /// TODO: 未実装の機能．スキャンのパイプライン (#31) の実装時に削除します．
    Unimplemented(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unimplemented(what) => write!(f, "{what} is not implemented yet"),
        }
    }
}

impl std::error::Error for Error {}
