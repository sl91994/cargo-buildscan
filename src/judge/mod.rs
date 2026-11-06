use proc_macro2::Span;

use crate::source::{Location, Source};

/// finding の分類，出力用に "set-executable" 等の文字列へ変換する
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    /// `std::net`，`std::os::unix::net` の使用．
    /// 既知のHTTPクレート (`reqwest`, `ureq`, `curl`, `hyper`, `attohttpc`, `minreq` 等) の使用と build-dependency
    Net,
    /// 名前が `set_mode`, `from_mode`, `set_permissions` の関数・メソッド・パス
    SetExecutable,
    /// 構文エラー，外部ファイルの `mod`，`include!` 類，監視対象モジュールからのグロブインポート，サイズ超過
    Unanalyzable,
    /// program が rustc と判定できない `Command::new`，呼び出し以外の形での出現
    Spawn,
    /// `env::temp_dir` / `env::home_dir` の使用，危険なリテラルを書き込み先に含む書き込み・削除
    FsWrite,
}

impl Rule {
    /// 出力に使うID (`"set-executable"` 等)
    pub fn id(self) -> &'static str {
        match self {
            Rule::Net => "net",
            Rule::SetExecutable => "set-executable",
            Rule::Unanalyzable => "unanalyzable",
            Rule::Spawn => "spawn",
            Rule::FsWrite => "fs-write",
        }
    }

    /// ルール違反の重大度
    pub fn severity(self) -> Severity {
        match self {
            Rule::Net => Severity::High,
            Rule::SetExecutable => Severity::High,
            Rule::Unanalyzable => Severity::High,
            Rule::Spawn => Severity::Medium,
            Rule::FsWrite => Severity::Medium,
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
            (Rule::Unanalyzable, "unanalyzable", Severity::High),
            (Rule::Spawn, "spawn", Severity::Medium),
            (Rule::FsWrite, "fs-write", Severity::Medium),
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
