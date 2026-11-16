use std::collections::HashMap;

use syn::UseTree;

use crate::{judge::Finding, source::Source};

/// パスのセグメント列 (識別子のみ．ジェネリクス引数と先頭の `::` は除く)
pub type Segments = Vec<String>;

/// `syn::Path` をセグメント列にする (例: `std::string::String` -> `vec!["std", "string", "String"]`)
///
/// - 先頭の `::` は除く
pub fn segments(path: &syn::Path) -> Segments {
    path.segments.iter().map(|s| s.ident.to_string()).collect::<Segments>()
}

/// 名前 -> 完全パスの表 (例: `String` -> `std::string::String`)
#[derive(Debug, Default)]
pub struct Aliases {
    map: HashMap<String, Vec<Segments>>,
}

impl Aliases {
    /// ファイル全体 (関数内，インライン mod 内を含む) の use / extern crate から表を作る
    pub fn build(file: &syn::File) -> Self {
        todo!()
    }

    /// 先頭のセグメントを表で置き換えた候補を全て返す．元のパスも候補に含める
    pub fn expand(&self, path: &[String]) -> Vec<Segments> {
        todo!()
    }
}

/// 単一の `use` の絶対パス
pub struct UseEntry {
    /// 完全パス．`self` は親のパスになる (`use std::process::{self}` → `std::process`)
    pub path: Segments,
    /// `use` 変数の種類
    pub kind: UseKind,
    /// 末尾のセグメント (`self` ならそのトークン，グロブなら `*`) の span
    pub span: proc_macro2::Span,
}

/// `use` の変数の種類
pub enum UseKind {
    /// `use a::b` (束縛名はパスの末尾)
    Name,
    /// `use a::b as c;`
    Rename(String),
    /// `use a::b::*` (path は `a::b`)
    Glob,
}

/// `UseTree` を prefix のスタックを持って再帰で辿り，`UseEntry` の一覧を返す
pub fn flatten_use(item: &syn::ItemUse) -> Vec<UseEntry> {
    fn walk(tree: &UseTree, prefix: &mut Segments, entries: &mut Vec<UseEntry>) {
        match tree {
            UseTree::Path(use_path) => {
                prefix.push(use_path.ident.to_string());
                walk(&use_path.tree, prefix, entries);
                prefix.pop();
            }
            UseTree::Name(use_name) => {
                let path = if use_name.ident == "self" {
                    prefix.clone()
                } else {
                    let mut path = prefix.clone();
                    path.push(use_name.ident.to_string()); // ["std", "process", "Command"]
                    path
                };
                entries.push(UseEntry { path, kind: UseKind::Name, span: use_name.ident.span() });
            }
            UseTree::Rename(use_rename) => {
                let path = if use_rename.ident == "self" {
                    prefix.clone()
                } else {
                    let mut path = prefix.clone();
                    path.push(use_rename.ident.to_string()); // ["std", "process", "Command"]
                    path
                };
                entries.push(UseEntry {
                    path,
                    kind: UseKind::Rename(use_rename.rename.to_string()),
                    span: use_rename.ident.span(), // as <別名> の位置ではなく，元のパス名の位置を使用する
                });
            }
            UseTree::Glob(use_glob) => {
                entries.push(UseEntry {
                    path: prefix.clone(),
                    kind: UseKind::Glob,
                    span: use_glob.star_token.span,
                });
            }
            UseTree::Group(use_group) => {
                for tree in &use_group.items {
                    walk(tree, prefix, entries);
                }
            }
        }
    }

    let mut entries = Vec::new();
    walk(&item.tree, &mut Vec::new(), &mut entries);
    entries
}

/// 禁止されたモジュールからのグロブインポートを `unanalyzable` にする
pub fn forbidden_globs(file: &syn::File, aliases: &Aliases, source: &Source) -> Vec<Finding> {
    todo!()
}

/// ファイル内の全ての use / extern crate への参照を集める
#[derive(Default)]
struct UseItems<'ast> {
    uses: Vec<&'ast syn::ItemUse>,
    externs: Vec<&'ast syn::ItemExternCrate>,
}

impl<'ast> syn::visit::Visit<'ast> for UseItems<'ast> {
    // push するだけ
    fn visit_item_use(&mut self, i: &'ast syn::ItemUse) {
        self.uses.push(i);
    }

    fn visit_item_extern_crate(&mut self, i: &'ast syn::ItemExternCrate) {
        self.externs.push(i);
    }
}

fn collect(file: &syn::File) -> UseItems<'_> {
    let mut items = UseItems::default();
    // ファイルの全 item を既定の走査で巡回し，use のときだけ visit_item_use() が呼ばれる
    syn::visit::visit_file(&mut items, file);
    items
}

#[cfg(test)]
mod tests {
    use crate::names::{Segments, collect, flatten_use};

    /// ソース文字列からセグメントを生成する
    fn paths(src: &str) -> Vec<Segments> {
        let item: syn::ItemUse = syn::parse_str(src).unwrap();
        flatten_use(&item).into_iter().map(|e| e.path).collect()
    }

    #[test]
    fn test_flatten_use() {
        // 一時的なテスト．#12実装完了後はテーブルリスト形式に変更する
        let segments: Vec<Vec<String>> = paths("use std::process::{self, Command as C};");

        assert_eq!(
            segments,
            vec![
                vec!["std".to_string(), "process".to_string()],
                vec!["std".to_string(), "process".to_string(), "Command".to_string()],
            ]
        );
    }

    /// `collect()` が関数内とインライン mod まで到達すること
    #[test]
    fn collect_finds_nested_uses() {
        let file = syn::parse_file(
            r#"
        use a::b;
        fn main() { use c::d; }
        mod m { use e::f; extern crate g as h; }
        "#,
        )
        .unwrap();
        let items = collect(&file);
        
        assert!(items.uses.len() == 3);
        assert!(items.externs.len() == 1);
    }
}
