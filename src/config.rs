//! 監視対象の名前，HTTP クレート，危険なリテラル，解析しない入力の一覧

/// パスの末尾のセグメント列．正規化後のパスと末尾で照合する．
///
/// 例: `&["Command", "new"]` は `std::process::Command::new` や `C::new` (`use ... Command as C;`) に一致する．
pub type PathSuffix = &'static [&'static str];

/// モジュールのパス．パスの先頭で照合する
///
/// 例: `&["std", "net"]` は `std::net::TcpStream` や `use std::net::*;` に一致する．
pub type ModulePath = &'static [&'static str];

/// 読み込む 1 ファイルのサイズ上限．超えるものは `unanalyzable`
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// 出力するスニペットの最大長 (文字数)．文字境界で切り詰める．
///
/// TODO: #34 で値を確定する．
pub const MAX_SNIPPET_CHARS: usize = 200;

/// `unanalyzable`: 解析しない入力 (#12)
pub mod unanalyzable {
    use super::ModulePath;

    /// 読み込まずに `unanalyzable` にするマクロ
    pub const INCLUDE_MACROS: &[&str] = &["include", "include_str", "include_bytes"];

    /// そのモジュール自体からのグロブインポートを `unanalyzable` にするモジュール (`std`)
    pub const GLOB_FORBIDDEN_MODULES: &[ModulePath] = &[]; // TODO: #12

    /// そのモジュールと配下からのグロブインポートを `unanalyzable` にするモジュール (`std::process`，`std::fs`，`std::env`，`std::os`)
    pub const GLOB_FORBIDDEN_PREFIXES: &[ModulePath] = &[]; // TODO: #12
}

/// `net`: ネットワークの使用 (#18)
pub mod net {
    use super::ModulePath;

    /// 使用 (`use` を含む) を finding にするモジュール (`std::net`，`std::os::unix::net`)
    pub const MODULES: &[ModulePath] = &[]; // TODO: #18

    /// 既知の HTTP クレート (`reqwest`，`ureq`，`curl`，`hyper`，`attohttpc`，`minreq` など)．
    ///
    /// コード上の名前で書き，`-` は `_` に正規化しておく．build-dependency のパッケージ名とも，`-` を `_` に正規化して照合する．
    pub const HTTP_CRATES: &[&str] = &[]; // TODO: #18
}

/// `set-executable`: 実行権限の付与 (#19)
pub mod set_executable {
    /// 名前だけで照合する関数・メソッド・パスの末尾 (`set_mode`，`from_mode`，`set_permissions`)
    pub const NAMES: &[&str] = &[]; // TODO: #19
}

/// `spawn`: プロセス起動 (#22)
pub mod spawn {
    use super::PathSuffix;

    /// プロセス起動として検査する関数 (`Command::new`)．第 1 引数が program
    pub const FNS: &[PathSuffix] = &[]; // TODO: #22

    /// program の式で許可する環境変数を読む関数 (`env::var`，`env::var_os`)．第 1 引数が名前
    pub const ENV_READ_FNS: &[PathSuffix] = &[]; // TODO: #22

    /// program の式で許可する環境変数 (`RUSTC`，`RUSTC_WRAPPER`，`RUSTC_WORKSPACE_WRAPPER`)
    pub const ALLOWED_PROGRAM_ENVS: &[&str] = &[]; // TODO: #22

    /// program の式で許可するリテラル (`"rustc"`)
    pub const ALLOWED_PROGRAM_LITERALS: &[&str] = &[]; // TODO: #22

    /// 引数を同じ規則で検査する関数 (`OsString::from`，`PathBuf::from`，`String::from`，`Some`，`Ok`)
    ///
    /// ここにない関数呼び出しを program の式に含めば finding になる．
    pub const PASSTHROUGH_FNS: &[PathSuffix] = &[]; // TODO: #22
}

/// `fs-write`: `OUT_DIR` 外への書き込み・削除・リネーム (#23)
pub mod fs_write {
    use super::PathSuffix;

    /// 書き込みに使うかどうかによらず，使用を finding にする関数 (`env::temp_dir`，`env::home_dir`)
    pub const OUTSIDE_DIR_FNS: &[PathSuffix] = &[]; // TODO: #23

    /// 書き込み・削除・リネームを行う関数と，書き込み先を検査する引数の位置
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct WriteFn {
        pub path: PathSuffix,
        /// 0 始まりの引数の位置 (`fs::copy` は書き込み先の `[1]`，`fs::rename` は `[0, 1]`)
        pub targets: &'static [usize],
    }

    /// 書き込み先を検査する関数 (`fs::write`，`File::create`，`fs::copy`，`fs::rename`，`remove_*`，`create_dir*`)
    pub const WRITE_FNS: &[WriteFn] = &[]; // TODO: #23

    /// 書き込み先の危険なリテラルの接頭辞 (`/`，`\`，`~`)．ドライブ文字 (`C:`) は別に判定する
    pub const DANGEROUS_PREFIXES: &[&str] = &[]; // TODO: #23

    /// 含むと書き込み先の危険なリテラルとする部分文字列 (`..`)
    pub const DANGEROUS_SUBSTRINGS: &[&str] = &[]; // TODO: #23
}
