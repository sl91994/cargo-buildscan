use cargo_metadata::{CargoOpt, Metadata, MetadataCommand, Package};

use crate::{cli::ScanOpts, error::Result};

/// `cargo metadata` の実行
///
/// - `cargo metadata --format-version 1 --all-features` を `--filter-platform` 無しで実行
pub fn run(opts: &ScanOpts) -> Result<Metadata> {
    let mut cmd = MetadataCommand::new();

    if let Some(path) = &opts.manifest_path {
        cmd.manifest_path(path);
    }

    cmd.features(CargoOpt::AllFeatures);

    let mut extra = Vec::new();

    if opts.locked {
        extra.push("--locked".to_string());
    }
    if opts.offline {
        extra.push("--offline".to_string());
    }

    cmd.other_options(extra);

    Ok(cmd.exec()?)
}

/// ワークスペース外の path 依存も source が None になるため，source ではなく ID でメンバーを判定する
pub fn dependencies(meta: &Metadata) -> impl Iterator<Item = &Package> {
    meta.packages.iter().filter(|p| !meta.workspace_members.contains(&p.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use serde_json::{Value, json};

    const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

    fn package(name: &str, id: &str, source: Option<&str>) -> Value {
        json!({
            "name": name,
            "version": "0.1.0",
            "id": id,
            "source": source,
            "dependencies": [],
            "targets": [],
            "features": {},
            "manifest_path": format!("/{name}/Cargo.toml"),
        })
    }

    fn metadata(packages: Vec<Value>, members: &[&str]) -> Metadata {
        serde_json::from_value(json!({
            "packages": packages,
            "workspace_members": members,
            "resolve": null,
            "workspace_root": "/ws",
            "target_directory": "/ws/target",
            "version": 1,
        }))
        .unwrap()
    }

    fn names<'a>(packages: impl Iterator<Item = &'a Package>) -> Vec<&'a str> {
        packages.map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn offline_run_on_self_excludes_root_package_and_includes_syn() {
        let opts = ScanOpts {
            manifest_path: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").into()),
            offline: true,
            ..Default::default()
        };
        let meta = run(&opts).unwrap();
        let deps = names(dependencies(&meta));

        assert!(!deps.contains(&"cargo-buildscan"));
        assert!(deps.contains(&"syn"));
    }

    #[test]
    fn run_with_missing_manifest_returns_cargo_stderr() {
        let opts = ScanOpts {
            manifest_path: Some(
                concat!(env!("CARGO_MANIFEST_DIR"), "/no-such-dir/Cargo.toml").into(),
            ),
            offline: true,
            ..Default::default()
        };
        let err = run(&opts).unwrap_err();

        let Error::Metadata(cargo_metadata::Error::CargoMetadata { stderr }) = &err else {
            panic!("unexpected error: {err:?}");
        };
        assert!(stderr.contains("no-such-dir"), "{stderr}");
        assert!(err.to_string().starts_with("cargo metadata failed:\n"));
    }

    #[test]
    fn dependencies_excludes_all_workspace_members() {
        let meta = metadata(
            vec![
                package("app", "path+file:///ws/app#0.1.0", None),
                package("helper", "path+file:///ws/helper#0.1.0", None),
                package(
                    "serde",
                    "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0",
                    Some(REGISTRY),
                ),
            ],
            &["path+file:///ws/app#0.1.0", "path+file:///ws/helper#0.1.0"],
        );

        assert_eq!(names(dependencies(&meta)), ["serde"]);
    }

    /// ワークスペース外の path 依存も source が None になるため，source ではなく ID でメンバーを判定する
    #[test]
    fn dependencies_keeps_path_dependency_outside_workspace() {
        let meta = metadata(
            vec![
                package("app", "path+file:///ws/app#0.1.0", None),
                package("vendored", "path+file:///vendor/vendored#0.1.0", None),
            ],
            &["path+file:///ws/app#0.1.0"],
        );

        assert_eq!(names(dependencies(&meta)), ["vendored"]);
    }
}
