use std::{
    fmt::Display,
    fs::File,
    io::{self, Read},
};

use proc_macro2::TokenTree;
use syn::{Expr, Lit, Meta, MetaNameValue, visit::Visit};

use crate::{
    config::{self, unanalyzable},
    judge::{Finding, Rule},
    locate::BuildScript,
    source::Source,
};

/// Load / Parse の結果
pub enum Loaded {
    /// 構文解析できた場合
    Parsed { source: Source, file: syn::File, findings: Vec<Finding> },
    /// これ以上解析できない場合 (サイズ超過，UTF-8でない，構文エラー)
    Unanalyzable(Finding),
}

/// ファイルを読み込んで parse する．
/// I/O エラーは呼び出し側 (#31) で status error にする
pub fn load(script: &BuildScript) -> io::Result<Loaded> {
    let mut buf: Vec<u8> = Vec::new();
    File::open(&script.src_path)?.take(config::MAX_FILE_BYTES + 1).read_to_end(&mut buf)?;

    // サイズ上限チェック
    if buf.len() as u64 > config::MAX_FILE_BYTES {
        return Ok(Loaded::Unanalyzable(Finding::without_location(
            Rule::Unanalyzable,
            script.display_path.clone(),
            format!("file exceeds {} bytes (not analyzed)", config::MAX_FILE_BYTES),
        )));
    }

    let text = match String::from_utf8(buf) {
        Ok(s) => s,
        Err(e) => {
            return Ok(Loaded::Unanalyzable(Finding::without_location(
                Rule::Unanalyzable,
                script.display_path.clone(),
                format!("file is not valid UTF-8 ({e})"),
            )));
        }
    };

    Ok(parse(Source::new(script.display_path.clone(), text)))
}

/// 読み込み済みのテキストを解析する．
/// 単体テストと fixture (#37) から使用する
pub fn parse(source: Source) -> Loaded {
    let parsed = match syn::parse_file(&source.text) {
        Ok(f) => f,
        Err(e) => {
            return Loaded::Unanalyzable(Finding::at(
                Rule::Unanalyzable,
                &source,
                e.span(),
                format!("syntax error: {e}"),
            ));
        }
    };

    // パース結果を渡して，visitor から findings を受け取る
    let mut v = ExternalInputs { source: &source, findings: Vec::new() };
    v.visit_file(&parsed);
    let findings = v.findings;

    Loaded::Parsed { source, file: parsed, findings }
}

/// `message` の構築: `mod util; (external module is not loaded)` (path は `#[path]` の値)
fn external_mod_message(name: impl Display, path: Option<&str>) -> String {
    match path {
        Some(p) => {
            format!("#[path = {p:?}] mod {name}; (external module is not loaded)")
        }
        None => format!("mod {name}; (external module is not loaded)"),
    }
}

/// `message` の構築: `include_str!("x.md") (included file is not loaded)` (arg が無ければ `(..)`)
fn include_message(name: &syn::Ident, arg: Option<&str>) -> String {
    match arg {
        Some(a) => format!("{name}!({a:?}) (included file is not loaded)"),
        None => format!("{name}!(..) (included file is not loaded)"),
    }
}

/// トークン列の先頭が文字列リテラルなら，その値を返す
fn first_str_lit(tokens: proc_macro2::TokenStream) -> Option<String> {
    let token = tokens.into_iter().next()?;

    let TokenTree::Literal(lit) = token else { return None };

    match syn::Lit::new(lit) {
        Lit::Str(s) => Some(s.value()),
        _ => None,
    }
}

/// 外部 mod と `include!` 類を集める visitor
struct ExternalInputs<'a> {
    source: &'a Source,
    findings: Vec<Finding>,
}

impl ExternalInputs<'_> {
    /// unanalyzable の finding を位置付きで追加する
    fn report(&mut self, span: proc_macro2::Span, message: String) {
        self.findings.push(Finding::at(Rule::Unanalyzable, self.source, span, message));
    }

    /// `#[path = "..."]` の値を返す．なければ None
    fn path_attr(attrs: &[syn::Attribute]) -> Option<String> {
        attrs.iter().filter(|a| a.path().is_ident("path")).find_map(|a| match &a.meta {
            Meta::NameValue(MetaNameValue {
                value: Expr::Lit(syn::ExprLit { lit: Lit::Str(s), .. }),
                ..
            }) => Some(s.value()),
            _ => None, // `#[path]` や `#[path(...)]` など想定外の形は無視
        })
    }

    /// トークン列から `include!` 類と `mod x;` の並びを探す (Group 内に再帰する)
    fn scan_tokens(&mut self, tokens: proc_macro2::TokenStream) {
        let tokens = tokens.into_iter().collect::<Vec<TokenTree>>();

        for i in 0..tokens.len() {
            match &tokens[i..] {
                // include_str!
                [TokenTree::Ident(name), TokenTree::Punct(bang), rest @ ..]
                    if unanalyzable::INCLUDE_MACROS.iter().any(|im| name == im)
                        && bang.as_char() == '!' =>
                {
                    // include! の引数を取得
                    let arg = match rest.first() {
                        Some(TokenTree::Group(g)) => first_str_lit(g.stream()),
                        _ => None,
                    };

                    self.report(name.span(), include_message(name, arg.as_deref()));
                }
                // mod foo ;
                [TokenTree::Ident(kw), TokenTree::Ident(name), TokenTree::Punct(semi), ..]
                    if kw == "mod" && semi.as_char() == ';' =>
                {
                    self.report(kw.span(), external_mod_message(name, None));
                }
                // macro_rules! の中の mod $n;
                [
                    TokenTree::Ident(kw),
                    TokenTree::Punct(dollar), // メタ変数の接頭辞
                    TokenTree::Ident(name),   // メタ変数の名前
                    TokenTree::Punct(semi),
                    ..,
                ] if kw == "mod" && dollar.as_char() == '$' && semi.as_char() == ';' => {
                    self.report(kw.span(), external_mod_message(format_args!("${name}"), None));
                }
                // それ以外は再帰で潜る: (..)[..]{..}
                [TokenTree::Group(g), ..] => {
                    self.scan_tokens(g.stream());
                }
                _ => {}
            }
        }
    }
}

impl<'ast> syn::visit::Visit<'ast> for ExternalInputs<'_> {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        // mod宣言 (mod <...>;) のみで，中身は別ファイルにある場合
        if m.content.is_none() {
            // #[path = ".."] の値を探す
            let path_attr_value: Option<String> = Self::path_attr(&m.attrs);

            let message = external_mod_message(&m.ident, path_attr_value.as_deref());

            self.report(m.mod_token.span, message);
        }

        syn::visit::visit_item_mod(self, m);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        // mac.path の末尾が INCLUDE_MACROS なら report
        if let Some(seg) = mac.path.segments.last()
            && unanalyzable::INCLUDE_MACROS.iter().any(|im| seg.ident == im)
        {
            // include! の引数を取得
            let arg = first_str_lit(mac.tokens.clone());
            self.report(
                seg.ident.span(), // マクロ名の位置
                include_message(&seg.ident, arg.as_deref()),
            );
        }

        // トークン列の走査
        self.scan_tokens(mac.tokens.clone());

        syn::visit::visit_macro(self, mac);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        fs::{self, OpenOptions},
        io::Write,
        process,
    };

    use crate::{config::MAX_FILE_BYTES, load::first_str_lit};

    use super::*;
    use cargo_metadata::camino::Utf8PathBuf;
    use proc_macro2::TokenStream;

    /// parse して findings を返す．Unanalyzable なら panic
    fn findings_of(text: &str) -> Vec<Finding> {
        let source = Source::new("build.rs".into(), text.into());

        match parse(source) {
            Loaded::Parsed { findings, .. } => findings,
            Loaded::Unanalyzable(f) => panic!("expected Parsed, got {f:?}"),
        }
    }

    /// parse して Unanalyzable の Finding を返す．Parsed なら panic
    fn unanalyzable_of(text: &str) -> Finding {
        let source = Source::new("build.rs".into(), text.into());

        match parse(source) {
            Loaded::Parsed { findings, .. } => panic!("expected Unanalyzable, got {findings:?}"),
            Loaded::Unanalyzable(finding) => finding,
        }
    }

    /// finding の行と列．位置無しなら None
    fn loc(f: &Finding) -> Option<(usize, usize)> {
        f.site.as_ref().map(|s| (s.location.line, s.location.column))
    }

    // load 用
    /// テスト用の一時ファイルのパス (ファイルは作らない)
    fn temp_path(name: &str) -> Utf8PathBuf {
        let temp_dir = env::temp_dir();
        let path = temp_dir.join(format!("cargo-buildscan-{}-{name}", process::id()));
        Utf8PathBuf::from_path_buf(path).unwrap()
    }

    /// 一時ファイルを書き，そのパスを返す
    fn write_temp(name: &str, bytes: &[u8]) -> Utf8PathBuf {
        let utf8_path = temp_path(name);

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true) // 既存のファイルがあれば失敗する (リンクも辿らない)
            .open(&utf8_path)
            .unwrap();
        file.write_all(bytes).unwrap();

        utf8_path
    }

    /// 一時ファイルに書いて load し，ファイルを消してから結果を返す
    fn load_bytes(name: &str, bytes: &[u8]) -> io::Result<Loaded> {
        let path = write_temp(name, bytes);
        let script = script_at(path);
        let loaded = load(&script);
        let _ = fs::remove_file(&script.src_path);

        loaded
    }

    /// テスト用の `BuildScript`
    fn script_at(path: Utf8PathBuf) -> BuildScript {
        BuildScript {
            root: path.parent().unwrap().to_owned(),
            src_path: path,
            display_path: "build.rs".to_owned(),
        }
    }

    /// load して Unanalyzable の Finding を返す．それ以外なら panic
    fn load_unanalyzable(name: &str, bytes: &[u8]) -> Finding {
        match load_bytes(name, bytes) {
            Ok(Loaded::Unanalyzable(f)) => f,
            Ok(Loaded::Parsed { findings, .. }) => {
                panic!("{name}: expected Unanalyzable, got Parsed with {findings:?}")
            }
            Err(e) => panic!("{name}: expected Unanalyzable, got Err({e})"),
        }
    }

    /// load して Parsed の findings を返す．それ以外なら panic
    fn load_parsed(name: &str, bytes: &[u8]) -> Vec<Finding> {
        match load_bytes(name, bytes) {
            Ok(Loaded::Parsed { findings, .. }) => findings,
            Ok(Loaded::Unanalyzable(f)) => panic!("{name}: expected Parsed, got {f:?}"),
            Err(e) => panic!("{name}: expected Parsed, got Err({e})"),
        }
    }

    /// テーブル形式のテストの1ケース: (入力, 期待する finding の一覧: ((行, 列), message))
    type Case = (&'static str, &'static [((usize, usize), &'static str)]);

    /// テーブル形式のテスト．各入力の finding がすべて unanalyzable で，位置と message が期待どおりか
    ///
    /// - 期待 message は前方一致させ，厳密一致させない (変更する可能性があるため)
    fn check_cases_using_findings_of(cases: &[Case]) {
        for (text, expected) in cases {
            let findings = findings_of(text);
            assert!(
                findings.iter().all(|f| f.rule == Rule::Unanalyzable),
                "input: {text}, findings: {findings:?}"
            );

            let actual: Vec<((usize, usize), &str)> = findings
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let loc = loc(f).expect("finding without location");
                    let msg = f.message.as_str();
                    // 期待 message で前方一致すれば期待値に揃える (一致しなければ実際の値のまま diff に出る)
                    let msg = match expected.get(i) {
                        Some(&(_, exp)) if msg.starts_with(exp) => exp,
                        _ => msg,
                    };
                    ((loc.0, loc.1), msg)
                })
                .collect();

            assert_eq!(actual, *expected, "input: {text}");
        }
    }

    /// テーブル形式のテストの1ケース: (入力, 期待する単体 finding: ((行, 列), message))
    type UnitCase<'a> = (&'a str, ((usize, usize), &'a str));

    /// テーブル形式のテスト．finding が unanalyzable で，位置と message が期待どおりか
    ///
    /// - 期待 message は前方一致させ，厳密一致させない (変更する可能性があるため)
    fn check_cases_using_unanalyzable_of(cases: &[UnitCase]) {
        for &(text, expected) in cases {
            let finding = unanalyzable_of(text);
            assert!(finding.rule == Rule::Unanalyzable, "input: {text}, finding: {finding:?}");

            let pos = loc(&finding).expect("finding without location");
            let msg = finding.message.as_str();
            // 期待 message で前方一致すれば期待値に揃える (一致しなければ実際の値のまま diff に出る)
            let msg = if msg.starts_with(expected.1) { expected.1 } else { msg };

            assert_eq!((pos, msg), expected, "input: {text}");
        }
    }

    // parse: external mod
    #[test]
    fn external_mod() {
        // #[path] 付きの場合，位置は mod キーワードになる
        check_cases_using_findings_of(&[
            // 一般的な外部mod
            ("mod util;", &[((1, 1), "mod util;")]),
            // ネストされた外部mod (インラインmod の中，関数の中)
            ("mod a { mod b; }", &[((1, 9), "mod b;")]),
            ("fn main() { mod c; }", &[((1, 13), "mod c;")]),
            // インラインmod が report されないこと
            ("mod a { fn f() {} }", &[]),
            // パス属性を持つ外部mod
            (r#"#[path = "p.rs"] mod p;"#, &[((1, 18), r#"#[path = "p.rs"] mod p;"#)]),
            (
                r#"#[cfg(unix)] #[path = "u.rs"] mod sys;"#,
                &[((1, 31), r#"#[path = "u.rs"] mod sys;"#)],
            ),
            // 予期しないパス属性が report されること
            (r#"#[path = 1] mod x;"#, &[((1, 13), "mod x;")]),
            (r#"#[cfg_attr(unix, path = "u.rs")] mod sys;"#, &[((1, 34), "mod sys;")]),
            // 複数の findings
            ("mod a; mod b;", &[((1, 1), "mod a;"), ((1, 8), "mod b;")]),
            // shebang (先頭の `#!` 行) は syn が読み飛ばし，構文エラーにならない
            ("#!/usr/bin/env x\nmod x;", &[((2, 1), "mod x;")]),
        ]);
    }

    // parse: include!
    #[test]
    fn include_macros() {
        check_cases_using_findings_of(&[
            // 一般的な include! 類のマクロ
            (r#"include!("x");"#, &[((1, 1), r#"include!("x")"#)]),
            (r#"include_str!("x");"#, &[((1, 1), r#"include_str!("x")"#)]),
            (r#"include_bytes!("x");"#, &[((1, 1), r#"include_bytes!("x")"#)]),
            // パスを持つ include! 類のマクロ
            (r#"fn f() { ::std::include!("x"); }"#, &[((1, 17), r#"include!("x")"#)]),
            (r#"fn f() { core::include_bytes!("x"); }"#, &[((1, 16), r#"include_bytes!("x")"#)]),
            // 引数を持つ include! 類のマクロ
            (r#"fn f() { include_str!("x"); }"#, &[((1, 10), r#"include_str!("x")"#)]),
            (r#"fn f() { include_str!(r"x"); }"#, &[((1, 10), r#"include_str!("x")"#)]),
            (r#"fn f() { include_str!("x",); }"#, &[((1, 10), r#"include_str!("x")"#)]),
            (r#"include!(concat!(env!("OUT_DIR"), "/b.rs"));"#, &[((1, 1), r#"include!(..)"#)]),
            // 属性内の include! 類のマクロ
            (
                r#"#[doc = include_str!("README.md")] fn f() {}"#,
                &[((1, 9), r#"include_str!("README.md")"#)],
            ),
        ]);
    }

    // parse: macro の中 (scan_tokens)
    #[test]
    fn include_inside_macro() {
        check_cases_using_findings_of(&[
            // マクロ内の include! 類のマクロ
            (r#"println!("{}", include_str!("x"));"#, &[((1, 16), r#"include_str!("x")"#)]),
            (r#"vec![format!("{}", include_str!("x"))];"#, &[((1, 20), r#"include_str!("x")"#)]),
            // マクロ内の外部mod
            (r#"cfg_if! { if #[cfg(unix)] { mod unix; } }"#, &[((1, 29), r#"mod unix;"#)]),
            (r#"m! { pub mod x; }"#, &[((1, 10), r#"mod x;"#)]),
            // マクロ内のインラインmod
            (r#"m! {mod x { } }"#, &[]),
            // macro_rules! 内の外部mod (名前がメタ変数: mod $n;)
            (r#"macro_rules! m { ($n:ident) => { mod $n; } }"#, &[((1, 34), r#"mod $n;"#)]),
            // 可視性もメタ変数 ($v mod $n;)
            (
                r#"macro_rules! m { ($v:vis $n:ident) => { $v mod $n; } }"#,
                &[((1, 44), r#"mod $n;"#)],
            ),
            // 繰り返し $( .. )* の中 (Group への再帰)
            (
                r#"macro_rules! m { ($($n:ident),*) => { $(mod $n;)* } }"#,
                &[((1, 41), r#"mod $n;"#)],
            ),
            // macro_rules! 内の item フラグメント指定子
            (
                r#"macro_rules! m { ($i:item) => { $i } } m!(mod payload;);"#,
                &[((1, 43), r#"mod payload;"#)],
            ),
            // macro_rules! 内の include! 類のマクロ
            (
                r#"macro_rules! m { () => { include_str!("x") } }"#,
                &[((1, 26), r#"include_str!("x")"#)],
            ),
        ]);
    }

    // parse: 非該当
    #[test]
    fn not_reported() {
        check_cases_using_findings_of(&[
            ("", &[]),
            (r#"fn f() { println!("mod x;"); }"#, &[]),
            ("/// mod x;\n// include!(\"y\")\nfn f() {}", &[]),
            (r#"fn f() { my_include!("x"); include_strs!("x"); }"#, &[]),
            (r#"fn f() { let include = 1; }"#, &[]),
            (r#"macro_rules! m { ($n:ident) => { mod $n { } } }"#, &[]),
        ]);
    }

    // parse: 構文エラーと位置
    #[test]
    fn syntax_error() {
        check_cases_using_unanalyzable_of(&[
            // 構文エラーの位置は syn のエラーの span (式が来るべき `;` の位置)
            ("fn f() {\n let x = ;\n}", ((2, 10), "syntax error:")),
            // 外部 mod があっても，構文エラーなら Parsed ではなく Unanalyzable になる (閉じていない括弧は字句解析の段階で失敗する)
            ("mod x;\nfn (", ((2, 4), "syntax error:")),
        ]);
    }

    #[test]
    fn test_first_str_lit() {
        let cases: &[(&str, Option<&str>); 5] = &[
            ("\"x\"", Some("x")),
            ("\"x\", y", Some("x")),
            ("concat!(\"x\")", None),
            ("42", None),
            ("", None),
        ];

        for case in cases {
            let input = case.0;
            let tokens = input
                .parse::<TokenStream>()
                .unwrap_or_else(|e| panic!("failed to parse {input}: {e}"));

            let is_lit = first_str_lit(tokens);

            assert_eq!(is_lit, case.1.map(|x| x.to_string()));
        }
    }

    // load: 一時ファイル
    /// 上限を1バイト超えると，読み込まずに位置なしの `unanalyzable` になる
    #[test]
    fn load_too_large() {
        let max = usize::try_from(MAX_FILE_BYTES).unwrap();
        let f = load_unanalyzable("too_large", &vec![b' '; max + 1]);

        assert_eq!(f.rule, Rule::Unanalyzable);
        assert_eq!(f.site, None);
        // finding の file は src_path (絶対パス) ではなく display_path
        assert_eq!(f.file, "build.rs");
        assert!(f.message.contains("exceeds"), "{f:?}");
    }

    /// 上限ちょうどは解析される (境界: `take(MAX + 1)` と `>` の off-by-one を防ぐ)
    #[test]
    fn load_exactly_max_size() {
        let max = usize::try_from(MAX_FILE_BYTES).unwrap();
        let mut bytes = b"//".to_vec();
        bytes.resize(max, b'a');

        let findings = load_parsed("exactly_max_size", &bytes);

        assert!(findings.is_empty(), "{findings:?}");
    }

    /// UTF-8 でないものは I/O エラーではなく位置なしの `unanalyzable` になる
    #[test]
    fn load_not_utf8() {
        let f = load_unanalyzable("not_utf8", &[0xff, 0xfe]);

        assert_eq!(f.rule, Rule::Unanalyzable);
        assert_eq!(f.site, None);
        assert_eq!(f.file, "build.rs");
        assert!(f.message.contains("UTF-8"), "{f:?}");
    }

    /// load は `Source::new` を通して BOM を除く (位置が BOM の分ずれない)
    #[test]
    fn load_strips_bom() {
        let findings = load_parsed("strips_bom", b"\xEF\xBB\xBFmod x;");

        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(loc(&findings[0]), Some((1, 1)));
        assert_eq!(findings[0].file, "build.rs");
        assert!(findings[0].message.starts_with("mod x;"), "{:?}", findings[0]);
    }

    /// 読み込みの失敗は Err (status error は呼び出し側の #31 で決める)
    #[test]
    fn load_missing_file() {
        let script = script_at(temp_path("missing_file"));

        assert!(load(&script).is_err());
    }
}
