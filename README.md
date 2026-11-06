# cargo-buildscan

[![CI](https://github.com/sl91994/cargo-buildscan/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/sl91994/cargo-buildscan/actions/workflows/ci.yml)

依存クレートの **build script (`build.rs`)** を **実行せずに** 静的解析し，ネットワーク・実行権限の付与・rustc 以外のプロセス起動・`OUT_DIR` 外への書き込みといった **既知の危険な操作** を，根拠となるコード位置付きで報告する `cargo` サブコマンドです．

目的は防御ではなく **レビューの手がかり** です．報告されたものを人間が読むための一次選別であり，報告されなかったものが安全であることは保証しません．

> [!NOTE]
> 現在開発中 (v0.1) であり，以下の機能はまだ実装されていません．判定ルールの詳細は [DESIGN.md](DESIGN.md) を参照してください．
>
> v0.1 は既知の危険な操作を検出します (拒否リスト方式)．定型の操作しかしないことの確認 (許可リスト方式) とベースラインは v0.2 で導入します．

## 背景

2026年8月20日，乗っ取られたメンテナアカウントから `arrayref v0.3.10` などが公開され，マニフェストに typosquat 依存 (`proc-macro1`) が1行追加されました．悪性コードは `proc-macro1` の `build.rs` にあり，`cargo build` を実行しただけで，ペイロードを取得し，一時ディレクトリに書き込み，実行権限を付与して起動しました ([JFrog による解説](https://research.jfrog.com/post/arrayref-proc-macro1-crates-io/))．

この build script は，cargo-buildscan の `net`，`fs-write`，`set-executable`，`spawn` に該当します．

## できること・できないこと

**できること**

- `Cargo.lock` に載るすべての依存クレートの build script を，ビルドせずに解析します．
- 既知の危険な操作 (「[finding](#finding)」) をファイル・行・列付きで報告します．
- 解析できない入力 (外部ファイルの `mod`，`include!` など) は読み込まずに `unanalyzable` として報告し，見落としにせず人間に渡します．
- 終了コードで CI から使えます．

**できないこと**

- 悪性コードの検出の保証．v0.1 は本ツールを意識せずに書かれた悪性コードを想定しており，ルールを知る攻撃者には容易に回避されます (「[既知の限界](#既知の限界)」)．
- 「報告されなかったもの = 定型の操作しかしない」という確認．v0.2 で導入します．
- build script 以外の解析．proc macro は件数を出すのみです．build-dependency のライブラリ側，C ソース，`CMakeLists.txt` などは解析しません．
- マクロの中身の解析，変数をたどった値の追跡．

## 既存ツールとの関係

| ツール                      | 何をするか                                                            | cargo-buildscan との関係                                                 |
| --------------------------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| cargo-audit                 | 既知アドバイザリとの照合                                              | 未報告の悪性クレートは検出できません．補完関係です．                     |
| cargo-deny (`[bans.build]`) | build script を持ってよいクレートの許可リスト，実行ファイル同梱の検出 | 「持ってよいか」を決めます．buildscan は「中身が何をするか」を示します． |
| cargo-vet / cargo-crev      | 人間によるレビュー記録の共有                                          | buildscan はレビューの手がかりを示す前段です．                           |
| cargo-audit-build           | 全 `build.rs` を人間が読んで判断                                      | 同じ問題設定です．buildscan は機械的な一次選別を加えます．               |

同梱された実行ファイルやスクリプトは解析しないため，cargo-deny の `[bans.build]` を併用するとよいでしょう．

## 使い方

```sh
cargo install --path .

cargo buildscan [OPTIONS]
```

| オプション               | 既定値   | 説明                                  |
| ------------------------ | -------- | ------------------------------------- |
| `--manifest-path <PATH>` | カレント | 対象プロジェクト．                    |
| `--locked` / `--offline` | なし     | `cargo metadata` にそのまま渡します． |

依存は全ワークスペースメンバーのフィーチャーを有効にし，プラットフォームで絞らずに解決します．ワークスペースメンバー自身の build script は対象外です．

**終了コード**: 0 は reported と error がない，1 は reported か error がある，2 はツール自体のエラーです．重大度による閾値は設けません (reported はすべて読む対象であるため)．

**副作用**: build script は実行しませんが，`cargo metadata` が依存のソースをダウンロードし，`Cargo.lock` がなければ生成します．避けたい場合は `--offline` / `--locked` を使ってください．

## 判定

各 build script は次のいずれかに分類されます．

| 状態       | 意味                                                                                       |
| ---------- | ------------------------------------------------------------------------------------------ |
| recognized | finding が 0 件．既知の危険な操作が見つからなかったことだけを意味し，安全を意味しません． |
| reported   | finding が 1 件以上．**人間がレビューすべきものです．**                                    |
| error      | 読み込みなどに失敗しました．                                                               |

### finding

| ID               | 内容                                                                                                    | 重大度 |
| ---------------- | ------------------------------------------------------------------------------------------------------- | ------ |
| `net`            | ネットワークアクセス (`std::net` や既知の HTTP クレート)                                                | high   |
| `set-executable` | 実行権限の付与 (`set_mode`，`from_mode`，`set_permissions`)                                             | high   |
| `unanalyzable`   | 構文エラー，外部ファイルの `mod`，`include!` 類，`std::fs` などからのグロブインポート，サイズ超過     | high   |
| `spawn`          | rustc と判定できないプロセス起動                                                                        | medium |
| `fs-write`       | `env::temp_dir` / `env::home_dir` の使用，絶対パスや `..` を含むパスへの書き込み・削除                  | medium |

重大度は表示順にのみ使います．条件の詳細は [DESIGN.md](DESIGN.md) の §3 を参照してください．

`spawn` は program の式だけを見て判定するため，変数を経由した rustc の起動 (`let rustc = env::var("RUSTC")?; Command::new(rustc)`) も報告されます．

## 出力形式

```
cargo-buildscan: 47 build scripts, 44 recognized, 3 reported, 0 errors
                 12 proc-macro crates not analyzed

[HIGH] example-dropper 1.0.0 (crates.io)
  build.rs:12:5   net              ureq::get
  build.rs:19:15  fs-write         env::temp_dir
  build.rs:21:9   set-executable   set_mode
  build.rs:22:9   spawn            Command::new (program is not rustc)

[HIGH] vendored-sys 0.2.0 (crates.io)
  build.rs:3:1    unanalyzable     mod util; (external module is not loaded)

[MEDIUM] some-crate 0.3.1 (crates.io)
  build.rs:31:5   spawn            Command::new("git")
```

数値は表示例です．クレート由来の文字列に含まれる制御文字 (ANSI エスケープ，Unicode の双方向制御文字など) はエスケープして表示します．JSON 出力は v0.2 で導入します．

## 既知の限界

v0.1 は拒否リスト方式であり，攻撃者は本ツールのルールを知りえます．以下は報告されません．

- 拒否リストにない操作．ファイルや環境変数の読み取り，`OpenOptions` による書き込み，リンク作成，FFI，外部プログラムを指定する引数 (`cc::Build::compiler`，`cargo:rustc-link-arg` など)．
- マクロの中の操作 (`println!("{:?}", Command::new("sh").spawn())` など)．
- 変数を経由した書き込み先 (`let p = "/tmp/x"; fs::write(p, ..)`)，`type` エイリアスを経由した名前．
- build-dependency のライブラリ側や，外部ファイル (C ソース，ビルドスクリプト) に置かれた悪性処理．

## ライセンス

本リポジトリは以下のデュアルライセンスを採用しています．

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))
