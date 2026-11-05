use proc_macro2::Span;

use crate::source::{Location, Source};

/// finding の分類，出力用に "set-executable" 等の文字列へ変換する
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    /// `std::net` の使用．
    /// 既知のHTTPクレート (`reqwest`, `ureq`, `curl`, `hyper`, `attohttpc`, `minreq` 等) の使用
    Net,
    /// `PermissionsExt::set_mode`, `from_mode`, `set_permissions`
    SetExecutable,
    /// 機密パターンに一致する環境変数の読み取り．
    /// `env::vars`, `vars_os`, `env::home_dir`
    EnvSecret,
    /// 外部プログラムを指定しうるAPI・引数・ディレクティブ
    ExecArg,
    /// 構文エラー，動的なパスの `include!` 類，パッケージルート外を指すパスやサイズ超過
    Unanalyzable,
    /// 条件を満たさないプロセス起動 (`Command::new`, `CommandExt::exec` 等)
    Spawn,
    /// `OUT_DIR` 配下と判断できない書き込み・削除・リネームやリンク作成
    FsWrite,
    /// 名前が文字列リテラルでない環境変数の読み取り
    EnvDynamic,
    /// 許可リスト外の build-dependency の使用 (関数，マクロ，属性)
    BuildDep,
    /// `unsafe` ブロック，`extern` ブロック，`asm!`，`global_asm!`
    Unsafe,
    /// 標準・許可リスト外のマクロ呼び出し，`macro_rules!` の定義
    Macro,
}

impl Rule {
    /// 出力に使うID (`"set-executable"` 等)
    pub fn id(self) -> &'static str {
        match self {
            Rule::Net => "net",
            Rule::SetExecutable => "set-executable",
            Rule::EnvSecret => "env-secret",
            Rule::ExecArg => "exec-arg",
            Rule::Unanalyzable => "unanalyzable",
            Rule::Spawn => "spawn",
            Rule::FsWrite => "fs-write",
            Rule::EnvDynamic => "env-dynamic",
            Rule::BuildDep => "build-dep",
            Rule::Unsafe => "unsafe",
            Rule::Macro => "macro",
        }
    }

    /// ルール違反の重大度
    pub fn severity(self) -> Severity {
        match self {
            Rule::Net => Severity::High,
            Rule::SetExecutable => Severity::High,
            Rule::EnvSecret => Severity::High,
            Rule::ExecArg => Severity::High,
            Rule::Unanalyzable => Severity::High,
            Rule::Spawn => Severity::Medium,
            Rule::FsWrite => Severity::Medium,
            Rule::EnvDynamic => Severity::Low,
            Rule::BuildDep => Severity::Low,
            Rule::Unsafe => Severity::Low,
            Rule::Macro => Severity::Low,
        }
    }
}

/// 表示順にのみ使用する．`Rule` から一意に定まる (High < Medium < Low)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    High,
    Medium,
    Low,
}

/// finding の位置と，その行のスニペット (ソースコード)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub location: Location,
    /// 検出した位置のソースコード
    pub snippet: String,
}

/// 単一の検出結果を表す構造体
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub rule: Rule,
    /// パッケージルートからの相対パス (区切り文字は `/`)
    pub file: String,
    /// 位置を持たない finding (ルートファイルのサイズ超過等) では `None`
    pub site: Option<Site>,
    /// 検出した操作と理由 (`ureq::get`，`std::fs::write (destination not under OUT_DIR)` 等)
    pub message: String,
}

impl Finding {
    /// span の位置にある finding を作る．span は `source` を解析したものに限る
    pub fn at(rule: Rule, source: &Source, span: Span, message: String) -> Self {
        let location = source.location(span);
        // 取得できない場合は panic を避けて，空文字列にフォールバック
        let snippet = source.line_text(location.line).unwrap_or_default().trim().to_owned();
        let site: Site = Site { location, snippet };

        Self { rule, file: source.path.to_string(), site: Some(site), message }
    }

    /// 位置を持たない finding を作る．(ルートファイルのサイズ超過等)
    pub fn without_location(rule: Rule, file: String, message: String) -> Self {
        Self { rule, file, site: None, message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::tests::span_of;

    #[test]
    fn rule_ids_and_severities_match_readme() {
        let table = [
            (Rule::Net, "net", Severity::High),
            (Rule::SetExecutable, "set-executable", Severity::High),
            (Rule::EnvSecret, "env-secret", Severity::High),
            (Rule::ExecArg, "exec-arg", Severity::High),
            (Rule::Unanalyzable, "unanalyzable", Severity::High),
            (Rule::Spawn, "spawn", Severity::Medium),
            (Rule::FsWrite, "fs-write", Severity::Medium),
            (Rule::EnvDynamic, "env-dynamic", Severity::Low),
            (Rule::BuildDep, "build-dep", Severity::Low),
            (Rule::Unsafe, "unsafe", Severity::Low),
            (Rule::Macro, "macro", Severity::Low),
        ];

        for (rule, id, severity) in table {
            assert_eq!(rule.id(), id);
            assert_eq!(rule.severity(), severity, "{id}");
        }
    }

    #[test]
    fn severities_sort_high_first() {
        let mut severities = [Severity::Low, Severity::High, Severity::Medium];
        severities.sort();
        assert_eq!(severities, [Severity::High, Severity::Medium, Severity::Low]);
    }

    #[test]
    fn at_records_location_and_trimmed_snippet() {
        let source = Source::new(
            "src/build/mod.rs".to_owned(),
            "fn main() {\n    let url = \"日本語\"; ureq::get(url);\n}\n".to_owned(),
        );
        let span = span_of(&source, "ureq");

        let finding = Finding::at(Rule::Net, &source, span, "ureq::get".to_owned());

        assert_eq!(
            finding,
            Finding {
                rule: Rule::Net,
                file: "src/build/mod.rs".to_owned(),
                site: Some(Site {
                    location: Location { line: 2, column: 22 },
                    snippet: "let url = \"日本語\"; ureq::get(url);".to_owned(),
                }),
                message: "ureq::get".to_owned(),
            }
        );
    }

    #[test]
    fn without_location_has_no_site() {
        let finding = Finding::without_location(
            Rule::Unanalyzable,
            "build.rs".to_owned(),
            "file exceeds size limit".to_owned(),
        );

        assert_eq!(finding.file, "build.rs");
        assert_eq!(finding.site, None);
    }
}
