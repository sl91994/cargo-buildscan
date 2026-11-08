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

/// メッセージの構築: `mod util; (external module is not loaded)` (path は `#[path]` の値)
fn external_mod_message(name: impl Display, path: Option<&str>) -> String {
    match path {
        Some(p) => {
            format!("#[path = {p:?}] mod {name}; (external module is not loaded)")
        }
        None => format!("mod {name}; (external module is not loaded)"),
    }
}

/// メッセージの構築: `include_str!("x.md") (included file is not loaded)` (arg が無ければ `(..)`)
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
