use proc_macro2::LineColumn;

/// 読み込んだ 1 ファイルを表す構造体
#[derive(Debug)]
pub struct Source {
    /// パッケージルートからの相対パス (区切り文字は `/`)
    pub path: String,
    /// ファイルの内容．先頭の BOM は除いておく
    pub text: String,
}

/// finding の位置 (いずれも 1 始まり)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Location {
    pub line: usize,
    /// 文字数で数えた列
    pub column: usize,
}

impl Source {
    /// 先頭 BOM を除く
    pub fn new(path: String, mut text: String) -> Self {
        // rustc / syn と同様，先頭の BOM を 1 つだけ除く
        if text.starts_with('\u{feff}') {
            text.drain(..'\u{feff}'.len_utf8());
        }

        Self { path, text }
    }

    /// span から finding の位置を求める．
    /// span は `self.text` を解析したものに限る
    pub fn location(&self, span: proc_macro2::Span) -> Location {
        let LineColumn { line, column } = span.start();

        // line が 1 始まり，column は 0 始まりの文字数なため，1 始まりに合わせる
        Location { line, column: column + 1 }
    }

    /// 指定した 1 始まりの行番号に対応する行のテキストを返す
    pub fn line_text(&self, line: usize) -> Option<&str> {
        self.text.lines().nth(line.checked_sub(1)?)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use proc_macro2::{Span, TokenStream, TokenTree};
    use std::str::FromStr;

    fn source(text: &str) -> Source {
        Source::new("build.rs".to_owned(), text.to_owned())
    }

    /// `source.text` をトークン化し，名前が `name` の最初の識別子の span を返す (グループの中も探す)
    pub(crate) fn span_of(source: &Source, name: &str) -> Span {
        fn find(stream: TokenStream, name: &str) -> Option<Span> {
            stream.into_iter().find_map(|tree| match tree {
                TokenTree::Ident(ident) if ident == name => Some(ident.span()),
                TokenTree::Group(group) => find(group.stream(), name),
                _ => None,
            })
        }
        let stream = TokenStream::from_str(&source.text).unwrap();
        find(stream, name).unwrap_or_else(|| panic!("`{name}` not found"))
    }

    #[test]
    fn location_and_snippet_on_later_lines() {
        let src = source("fn main() {\n    let first = 1;\n    second(first);\n}\n");

        assert_eq!(src.location(span_of(&src, "first")), Location { line: 2, column: 9 });
        assert_eq!(src.location(span_of(&src, "second")), Location { line: 3, column: 5 });
        assert_eq!(src.line_text(3), Some("    second(first);"));
    }

    #[test]
    fn column_counts_chars_not_bytes() {
        // 日本語は 3 文字 (9 バイト)．バイト数で数えると Command の列は 22 になる
        let src = source("let s = \"日本語\"; Command::new();");

        assert_eq!(src.location(span_of(&src, "Command")), Location { line: 1, column: 16 });
        assert_eq!(src.line_text(1), Some("let s = \"日本語\"; Command::new();"));
    }

    #[test]
    fn multibyte_on_earlier_and_same_line() {
        // 前の行のマルチバイト文字は後の行の列に影響せず，同じ行では文字数で数える
        let src = source("// 日本語のコメント\nlet s = \"🦀é\"; target();\n");

        assert_eq!(src.location(span_of(&src, "target")), Location { line: 2, column: 15 });
        assert_eq!(src.line_text(2), Some("let s = \"🦀é\"; target();"));
    }

    #[test]
    fn strips_bom_and_crlf() {
        let src = source("\u{feff}fn a() {}\r\nfn b() {}");

        assert!(!src.text.starts_with('\u{feff}'));
        // BOM を列に数えない
        assert_eq!(src.location(span_of(&src, "a")), Location { line: 1, column: 4 });
        assert_eq!(src.location(span_of(&src, "b")), Location { line: 2, column: 4 });
        // スニペットに BOM と \r を含まない
        assert_eq!(src.line_text(1), Some("fn a() {}"));
        assert_eq!(src.line_text(2), Some("fn b() {}"));
    }

    #[test]
    fn strips_only_one_bom() {
        let src = source("\u{feff}\u{feff}fn a() {}");
        assert_eq!(src.text, "\u{feff}fn a() {}");
    }

    #[test]
    fn line_text_out_of_range_is_none() {
        let src = source("fn a() {}\n");

        assert_eq!(src.line_text(0), None);
        assert_eq!(src.line_text(2), None);
    }
}
