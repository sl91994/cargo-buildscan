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
    /// kind が build の改名前依存パッケージ名
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// target 1つ分のJSON
    fn target(kind: &str, src_path: &str) -> Value {
        json!({
            "name": "target_name",
            "kind": [kind],
            "src_path": src_path
        })
    }

    /// 依存1つ分のJSON．
    /// kind は None ("normal") / Some("build") / Some("dev")
    fn dep(name: &str, kind: Option<&str>, rename: Option<&str>) -> Value {
        json!({
            "name": name,
            "req": "^1.0",
            "kind": kind,
            "optional": false,
            "rename": rename,
            "uses_default_features": true,
            "features": [],
        })
    }

    /// `manifest_path` は "/pkg/Cargo.toml" に固定: root は "/pkg"
    fn package(targets: Vec<Value>, dependencies: Vec<Value>) -> Package {
        let json_str = json!({
            "name": "package",
            "version": "0.1.0",
            "id": "path+file:///pkg#0.1.0",
            "dependencies": dependencies,
            "targets": targets,
            "features": {},
            "manifest_path": "/pkg/Cargo.toml"
        });

        serde_json::from_value(json_str).unwrap()
    }

    /// `build_script` が Some(`root` / `src_path` / `display_path`) の3つであり，`is_proc_macro()` が false であること
    #[test]
    fn build_script_only() {
        let pkg = package(
            vec![target("lib", "/pkg/src/lib.rs"), target("custom-build", "/pkg/build.rs")],
            vec![],
        );
        let located = locate(&pkg);

        assert!(!located.is_proc_macro);

        let Some(BuildScript { root, src_path, display_path }) = &located.build_script else {
            panic!("build_script is None");
        };

        assert_eq!(root, "/pkg");
        assert_eq!(src_path, "/pkg/build.rs");
        assert_eq!(display_path, "build.rs");
    }

    /// `build_script` が None，`is_proc_macro()` が true であること
    #[test]
    fn proc_macro_only() {
        let pkg = package(vec![target("proc-macro", "/pkg/src/lib.rs")], vec![]);
        let located = locate(&pkg);

        assert!(located.is_proc_macro);
        assert!(located.build_script.is_none());
    }

    /// `build_script` が Some，`is_proc_macro()` が true であること (両方)
    #[test]
    fn proc_macro_with_build_script() {
        let pkg = package(
            vec![target("proc-macro", "/pkg/src/lib.rs"), target("custom-build", "/pkg/build.rs")],
            vec![],
        );
        let located = locate(&pkg);

        assert!(located.is_proc_macro);
        assert!(located.build_script.is_some());
    }

    /// None かつ false
    #[test]
    fn neither() {
        let pkg = package(vec![target("lib", "/pkg/src/lib.rs")], vec![]);
        let located = locate(&pkg);

        assert!(!located.is_proc_macro);
        assert!(located.build_script.is_none());
    }

    /// 名称変更された依存関係の元の名前を取得できるかつ，名称変更後が含まれていないこと
    #[test]
    fn build_deps_use_original_name_of_renamed_dependency() {
        let pkg = package(vec![], vec![dep("reqwest", Some("build"), Some("r"))]);
        let located = locate(&pkg);

        assert_eq!(located.build_deps, vec!["reqwest"]);
        assert!(!located.build_deps.contains(&"r"))
    }

    /// normal / dev / build を混ぜ，build のみ残ること
    #[test]
    fn build_deps_exclude_normal_and_dev_dependencies() {
        let pkg = package(
            vec![],
            vec![
                dep("normal_dep", None, None),
                dep("dev_dep", Some("dev"), None),
                dep("build_dep", Some("build"), None),
            ],
        );
        let located = locate(&pkg);

        assert_eq!(located.build_deps, vec!["build_dep"]);
    }

    /// root の外を指す build script (`build = "../x.rs"`) は絶対パスで表示すること
    #[test]
    fn build_script_outside_root_uses_absolute_path() {
        let pkg = package(vec![target("custom-build", "/pkg/../build.rs")], vec![]);
        let located = locate(&pkg);

        let Some(BuildScript { root, src_path: _, display_path }) = &located.build_script else {
            panic!("build_script is None");
        };

        assert_eq!(root, "/pkg");
        assert_eq!(display_path, "/pkg/../build.rs");
    }

    /// `display_path()` の各分岐
    #[test]
    fn display_path_cases() {
        let root = Utf8Path::new("/pkg");

        // (src_path, 期待値)
        let list = [
            ("/pkg/build.rs", "build.rs"),
            ("/pkg/build/main.rs", "build/main.rs"), // 区切りが "/" になること
            ("/other/build.rs", "/other/build.rs"),  // strip_prefix が失敗する経路
            ("/pkg/../build.rs", "/pkg/../build.rs"), // strip_prefix は成功するが，ParentDir が残る経路
            ("/pkgfoo/build.rs", "/pkgfoo/build.rs"), // "/pkg" が文字列として前方一致するが，コンポーネント単位で一致しないこと
        ];

        for (src_path, expected_value) in list {
            assert_eq!(
                display_path(root, src_path.into()),
                expected_value,
                "src_path = {src_path}"
            );
        }
    }
}
