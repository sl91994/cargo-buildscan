use cargo_metadata::{
    DependencyKind, Package,
    camino::{Utf8Component, Utf8Path, Utf8PathBuf},
};

/// パッケージの Locate の結果
pub struct Located<'a> {
    pub package: &'a Package,
    /// build script (なければ None)
    pub build_script: Option<BuildScript>,
    /// kind に proc-macro を含む target を持つ
    pub is_proc_macro: bool,
    /// kind が build の依存パッケージ名
    pub build_deps: Vec<&'a str>,
}

pub struct BuildScript {
    /// パッケージルート (`manifest_path` の親)
    pub root: Utf8PathBuf,
    /// ルートファイルの絶対パス (`Target::src_path`)
    pub src_path: Utf8PathBuf,
    /// 出力に使用するパス．root からの相対パスで区切りは `/`．
    /// root 外なら，src_path をそのまま使用する
    pub display_path: String,
}

/// パッケージの build script，proc-macro，build 依存を特定する
pub fn locate(package: &Package) -> Located<'_> {
    // kind に1つでも proc-macro を含むものがあれば true
    let is_proc_macro = package.targets.iter().any(|t| t.is_proc_macro());

    let build_script = package.targets.iter().find(|t| t.is_custom_build()).map(|t| {
        let root = package.manifest_path.parent().expect("manifest_path has a parent").to_owned();
        let display_path = display_path(&root, &t.src_path);
        BuildScript { root, src_path: t.src_path.clone(), display_path }
    });

    let build_deps = package
        .dependencies
        .iter()
        .filter(|d| d.kind == DependencyKind::Build)
        .map(|d| d.name.as_str())
        .collect();

    Located { package, build_script, is_proc_macro, build_deps }
}

/// ソースパスをルートからの相対パスに変換し，ルート外なら絶対パスを返す
fn display_path(root: &Utf8Path, src_path: &Utf8Path) -> String {
    let Ok(rel) = src_path.strip_prefix(root) else {
        return src_path.to_string();
    };
    let mut parts = Vec::new();
    for c in rel.components() {
        match c {
            Utf8Component::Normal(s) => parts.push(s),
            // `..` などが残っていれば root の外を指しうるため，そのまま使う
            _ => return src_path.to_string(),
        }
    }
    parts.join("/")
}
