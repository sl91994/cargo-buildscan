//! 監視対象の名前，機密パターン，許可リスト，危険パターンの一覧

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

/// 環境変数名のパターン
///
/// グロブ文字列 (`"SSH_*"`) ではなく照合の種類を型で表し，パーサーを持たないようにする．
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamePattern {
    /// 名前全体が一致 (`HOME`)
    Exact(&'static str),
    /// 前方一致 (`SSH_*`)
    Prefix(&'static str),
    /// 後方一致 (`*_TOKEN`)
    Suffix(&'static str),
    /// 部分一致 (`*SECRET*`)
    Contains(&'static str),
}

/// 外部プログラムを指定しうる引数のパターン
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgPattern {
    /// 引数全体が一致
    Exact(&'static str),
    /// 前方一致 (`-fuse-ld=...`，`-Wl,...`，`@file` など)
    Prefix(&'static str),
    /// オプションとその値の組．`-C linker=x` のように別の引数に分かれる形と，`-Clinker=x` のように連結した形の両方に一致する
    WithValue { flag: &'static str, value_prefix: &'static str },
}

/// 名前の正規化 (#12)
pub mod names {
    use super::ModulePath;

    /// グロブインポート (`use std::process::*;`) があると，そのモジュール内で未修飾の名前も照合対象にするモジュール
    pub const GLOB_WATCHED_MODULES: &[ModulePath] = &[]; // TODO: #12
}

/// 由来の追跡 (#14)
pub mod origin {
    use super::PathSuffix;

    /// 引数の由来の和集合を返す関数 (`PathBuf::from`，`Path::new`，`OsString::from`，`String::from`，`Some`，`Ok` など)
    ///
    /// ここにない関数呼び出しの由来は `Unknown` になる．
    pub const PROPAGATING_FNS: &[PathSuffix] = &[]; // TODO: #14

    /// 引数の由来の和集合を返すマクロ (`format!`，`concat!` など)
    pub const PROPAGATING_MACROS: &[&str] = &[]; // TODO: #14
}

/// 標準マクロ (#13，#30)
pub mod macros {
    /// 書式文字列と引数に分けて解析するマクロ (`println!`，`format!`，`write!`，`panic!` など)
    pub const FORMAT_MACROS: &[&str] = &[]; // TODO: #13

    /// `macro` の finding にしない標準マクロ．許可リストの build-dependency のマクロは [`super::build_dep::ALLOWED`] に置く
    pub const STD_MACROS: &[&str] = &[]; // TODO: #30
}

/// `net`: ネットワークの使用 (#18)
pub mod net {
    use super::ModulePath;

    /// 使用 (`use` を含む) を finding にするモジュール (`std::net` など)
    pub const MODULES: &[ModulePath] = &[]; // TODO: #18

    /// 既知の HTTP クレート (`reqwest`，`ureq`，`curl`，`hyper` など)．コード上の名前で書き，`-` は `_` に正規化しておく
    pub const HTTP_CRATES: &[&str] = &[]; // TODO: #18
}

/// `set-executable`: 実行権限の付与 (#19)
pub mod set_executable {
    use super::PathSuffix;

    /// finding にする関数 (`fs::set_permissions` など)
    pub const FNS: &[PathSuffix] = &[]; // TODO: #19

    /// 名前だけで照合するメソッド (`set_mode`，`from_mode`，`set_permissions` など)
    pub const METHODS: &[&str] = &[]; // TODO: #19
}

/// `env-dynamic`，`env-secret`: 環境変数の読み取り (#20，#21)
pub mod env {
    use super::{NamePattern, PathSuffix};

    /// 環境変数の読み取りとして抽出する関数 (`env::var`，`env::var_os`)．第 1 引数が名前
    pub const READ_FNS: &[PathSuffix] = &[]; // TODO: #20

    /// 環境変数の読み取りとして抽出するマクロ (`env!`，`option_env!`)
    pub const READ_MACROS: &[&str] = &[]; // TODO: #20

    /// 名前によらず `env-secret` にする関数 (`env::vars`，`env::vars_os`，`env::home_dir`)
    pub const SECRET_FNS: &[PathSuffix] = &[]; // TODO: #21

    /// 機密パターン (`HOME`，`SSH_*`，`*_TOKEN`，`*SECRET*` など)
    pub const SECRET_PATTERNS: &[NamePattern] = &[]; // TODO: #21
}

/// `spawn`: プロセス起動 (#16，#22)
pub mod spawn {
    use super::PathSuffix;

    /// プロセス起動として抽出する関数 (`Command::new` など)．第 1 引数が program
    pub const FNS: &[PathSuffix] = &[]; // TODO: #22

    /// 名前だけで照合するプロセス起動のメソッド (`CommandExt::exec` の `exec` など)
    pub const METHODS: &[&str] = &[]; // TODO: #22

    /// program の由来として許可する環境変数 (`RUSTC`，`RUSTC_WRAPPER`，`RUSTC_WORKSPACE_WRAPPER`)
    pub const ALLOWED_PROGRAM_ENVS: &[&str] = &[]; // TODO: #16

    /// program の由来として許可するリテラル (`"rustc"`)
    pub const ALLOWED_PROGRAM_LITERALS: &[&str] = &[]; // TODO: #16
}

/// `fs-write`: ファイルの書き込み・削除・リネーム，リンク作成 (#16，#23，#24)
pub mod fs_write {
    use super::PathSuffix;

    /// 書き込み先として許可するディレクトリの環境変数
    pub const OUT_DIR_ENV: &str = "OUT_DIR";

    /// 書き込み・削除・リネームを行う関数と，由来を検査する引数の位置
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct WriteFn {
        pub path: PathSuffix,
        /// 0 始まりの引数の位置 (`fs::copy` は書き込み先の `[1]`，`fs::rename` は `[0, 1]`)
        pub targets: &'static [usize],
    }

    /// 書き込み先の由来を検査する関数 (`fs::write`，`File::create`，`fs::copy`，`fs::rename`，`remove_*`，`create_dir*`)
    pub const WRITE_FNS: &[WriteFn] = &[]; // TODO: #23

    /// 書き込み先によらず finding にするリンク作成の関数 (`symlink`，`symlink_file`，`symlink_dir`，`hard_link`)
    pub const LINK_FNS: &[PathSuffix] = &[]; // TODO: #24

    // TODO: #24 `OpenOptions::...open(path)` の扱い
}

/// `exec-arg`: 外部プログラムを指定しうる API・引数・ディレクティブ (#25〜#27)
pub mod exec_arg {
    use super::ArgPattern;

    /// 引数によらず finding にする `cc::Build` のメソッド (`compiler`，`archiver`，`ranlib` など)
    pub const CC_PROGRAM_METHODS: &[&str] = &[]; // TODO: #25

    /// C コンパイラへのフラグを渡すメソッド (`cc` の `flag` / `flags` / `flag_if_supported`，`bindgen` の `clang_arg` / `clang_args` など)
    pub const C_FLAG_METHODS: &[&str] = &[]; // TODO: #25

    /// C コンパイラの危険な引数 (`-B`，`-fuse-ld`，`--ld-path`，`-fplugin`，`-specs`，`-Wl,`，`-Xlinker`，`-Xclang`)
    pub const C_DANGEROUS_ARGS: &[ArgPattern] = &[]; // TODO: #25

    /// 許可された rustc 起動に引数を渡すメソッド (`arg`，`args`)
    pub const RUSTC_ARG_METHODS: &[&str] = &[]; // TODO: #26

    /// rustc の危険な引数 (`-C linker`，`linker=`，`-Z`，`--extern`，`@` で始まる引数ファイル)
    pub const RUSTC_DANGEROUS_ARGS: &[ArgPattern] = &[]; // TODO: #26

    /// `cargo:` ディレクティブの接頭辞 (`cargo:`，`cargo::`)
    pub const DIRECTIVE_PREFIXES: &[&str] = &[]; // TODO: #27

    /// 常に finding にするディレクティブ名 (`rustc-link-arg` 系，`rustc-cdylib-link-arg`)
    pub const LINK_ARG_DIRECTIVES: &[&str] = &[]; // TODO: #27
}

/// `build-dep`: build-dependency の使用 (#28，#30)
pub mod build_dep {
    /// 許可する build-dependency
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AllowedBuildDep {
        /// `cargo metadata` で解決したパッケージ名．Cargo.toml の表記のまま書く (`pkg-config`，`version_check`)
        pub package: &'static str,
        /// `macro` の finding にしないこのクレートのマクロ (`cfg_aliases` の `cfg_aliases` など)
        pub macros: &'static [&'static str],
    }

    /// 許可リスト (`cc`，`pkg-config`，`autocfg`，`version_check`，`rustc_version`，`bindgen`，`cfg_aliases`，`rustversion`)
    ///
    /// source が crates.io であることは `cargo_metadata::Source::is_crates_io` で判定する．
    pub const ALLOWED: &[AllowedBuildDep] = &[]; // TODO: #28，#30
}
