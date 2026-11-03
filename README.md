# cargo-buildscan

依存クレートの **build script (`build.rs`)** を **実行せずに** 静的解析し，ネットワーク・プロセス起動・ファイル書き込みなどの操作を定型の範囲でしか行わないものを除外して，残りを根拠となるコード位置付きで報告する `cargo` サブコマンドです．

目的は防御ではなく **レビュー対象の削減** です．報告されたものを人間が読むための **一次選別** であり，報告されなかったものが安全であることは保証しません．

> [!NOTE]
> 現在開発中 (v0.1) であり，以下の機能はまだ実装されていません．判定ルールの詳細は [DESIGN.md](DESIGN.md) を参照してください．

## 背景

2026年8月20日，乗っ取られたメンテナアカウントから `arrayref v0.3.10` などが公開され，マニフェストに typosquat 依存 (`proc-macro1`) が1行追加されました．悪性コードは `proc-macro1` の `build.rs` にあり，`cargo build` を実行しただけで，ペイロードを取得し，一時ディレクトリに書き込み，実行権限を付与して起動しました ([JFrog による解説](https://research.jfrog.com/post/arrayref-proc-macro1-crates-io/))．

この build script は，cargo-buildscan の `net`，`fs-write`，`set-executable`，`spawn` に該当します．

## できること・できないこと

**できること**

- `Cargo.lock` に載るすべての依存クレートの build script を，ビルドせずに解析します．
- 定型の操作 (cargo の環境変数を読む，rustc で機能を調べる，`OUT_DIR` に書く，`cc` で C をコンパイルする，など) しか行わないものを除外し，残りをファイル・行・列付きで報告します．
- レビュー済みの build script をハッシュで記録し，内容が変わらない限り再報告しません．
- CI 向けの JSON 出力と終了コード．

**できないこと**

- 悪性コードの検出の保証．静的解析は回避可能であり，攻撃者は本ツールのルールを知りえます (「[既知の限界](#既知の限界)」)．
- build script 以外の解析．proc macro は一覧を出すのみです．build-dependency のライブラリ側，C ソース，`CMakeLists.txt` などは解析しません．
- ファイルの読み取りの監視．
- 関数をまたぐ値の追跡．そのため，ヘルパー関数を使う正当な build script (anyhow，proc-macro2 など) も報告されます．これらはベースラインで承認して扱います．

## 既存ツールとの関係

| ツール                      | 何をするか                                                            | cargo-buildscan との関係                                                 |
| --------------------------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| cargo-audit                 | 既知アドバイザリとの照合                                              | 未報告の悪性クレートは検出できません．補完関係です．                     |
| cargo-deny (`[bans.build]`) | build script を持ってよいクレートの許可リスト，実行ファイル同梱の検出 | 「持ってよいか」を決めます．buildscan は「中身が何をするか」を示します． |
| cargo-vet / cargo-crev      | 人間によるレビュー記録の共有                                          | buildscan はレビュー対象を絞る前段です．                                 |
| cargo-audit-build           | 全 `build.rs` を人間が読んで判断                                      | 同じ問題設定です．buildscan は機械的な一次選別を加えます．               |

同梱された実行ファイルやスクリプトは解析しないため，cargo-deny の `[bans.build]` を併用するとよいでしょう．

## 使い方

```sh
cargo install --path .

cargo buildscan [OPTIONS]            # スキャン
cargo buildscan approve [OPTIONS]    # 現在の reported をベースラインに記録
```

| オプション               | 既定値                                 | 説明                                  |
| ------------------------ | -------------------------------------- | ------------------------------------- |
| `--manifest-path <PATH>` | カレント                               | 対象プロジェクト．                    |
| `--format <human\|json>` | `human`                                | 出力形式．                            |
| `--baseline <PATH>`      | `buildscan-baseline.toml` (存在すれば) | 承認済みハッシュの記録ファイル．      |
| `--locked` / `--offline` | なし                                   | `cargo metadata` にそのまま渡します． |

依存は全ワークスペースメンバーのフィーチャーを有効にし，プラットフォームで絞らずに解決します．ワークスペースメンバー自身の build script は対象外です．

**終了コード**: 0 は reported なし，1 は reported あり，2 はツール自体のエラーです．重大度による閾値は設けません (reported はすべて読む対象であるため)．

**副作用**: build script は実行しませんが，`cargo metadata` が依存のソースをダウンロードし，`Cargo.lock` がなければ生成します．避けたい場合は `--offline` / `--locked` を使ってください．

## 判定

各 build script は次のいずれかに分類されます．

| 状態       | 意味                                                                        |
| ---------- | --------------------------------------------------------------------------- |
| recognized | finding が 0 件．報告から除外されます．                                     |
| approved   | ベースラインに記録済みで，内容 (ハッシュ) が変わっていません．              |
| reported   | recognized でも approved でもありません．**人間がレビューすべきものです．** |
| error      | 読み込みなどに失敗しました．                                                |

### finding

| ID               | 内容                                                                                        | 重大度 |
| ---------------- | ------------------------------------------------------------------------------------------- | ------ |
| `net`            | ネットワークアクセス (`std::net` や既知の HTTP クレート)                                    | high   |
| `set-executable` | 実行権限の付与                                                                              | high   |
| `env-secret`     | 機密情報を含みうる環境変数 (`HOME`，`SSH_*`，`AWS_*`，`*_TOKEN` など) の読み取り，全列挙    | high   |
| `exec-arg`       | 外部プログラムを指定しうる設定 (`cc` のコンパイラ指定，危険なフラグ，`rustc-link-arg` など) | high   |
| `unanalyzable`   | 構文エラー，動的なパスの `include!`，パッケージルート外を指すパス，サイズ超過               | high   |
| `spawn`          | rustc 以外のプロセス起動                                                                    | medium |
| `fs-write`       | `OUT_DIR` 配下と判定できない書き込み・削除，リンク作成                                      | medium |
| `env-dynamic`    | 名前がリテラルでない環境変数の読み取り                                                      | low    |
| `build-dep`      | 許可リスト外の build-dependency の使用                                                      | low    |
| `unsafe`         | `unsafe` ブロック，`extern` ブロック，インラインアセンブリ                                  | low    |
| `macro`          | 標準・許可リスト外のマクロ呼び出し，`macro_rules!`                                          | low    |

重大度は表示順にのみ使います．許可される条件は [DESIGN.md](DESIGN.md) の §3 を参照してください．

## ベースライン

```sh
cargo buildscan approve                       # 現在の reported をすべて承認
cargo buildscan approve --package openssl-sys # 個別に承認
```

ベースライン (`buildscan-baseline.toml`) はリポジトリにコミットすることを想定しています．

```toml
version = 1

[[approved]]
name = "openssl-sys"
version = "0.9.104"
source = "registry+https://github.com/rust-lang/crates.io-index"
build_script_sha256 = "..."
note = "C ライブラリ検出．目視確認済み．"
```

照合には `(name, source, build_script_sha256)` を使い，**version は使いません**．バージョンが上がっても build script が同一なら再レビューは不要です．

> [!IMPORTANT]
> ハッシュの対象は build script の Rust ソース (`mod` / `include!` 類で読み込まれるファイルを含む) のみです．C ソースや `CMakeLists.txt`，build script が実行時に読むファイルは含みません．`spawn`，`exec-arg`，`build-dep` を持つ build script を承認すると，これらの外部ファイルが後のバージョンで変わっても再報告されません．

## 出力形式

### human

```
cargo-buildscan: 47 build scripts, 38 recognized, 6 approved, 3 reported
                 12 proc-macro crates not analyzed

[HIGH] example-dropper 1.0.0 (crates.io)
  build.rs:12:5   net              ureq::get
  build.rs:20:9   fs-write         std::fs::write (destination not under OUT_DIR)
  build.rs:21:9   set-executable   PermissionsExt::set_mode
  build.rs:22:9   spawn            Command::new (program not derived from RUSTC)

[MEDIUM] some-crate 0.3.1 (crates.io)
  build.rs:31:5   spawn            Command::new("git")
```

数値は表示例です．クレート由来の文字列に含まれる制御文字 (ANSI エスケープ，Unicode の双方向制御文字など) はエスケープして表示します．

### JSON

```json
{
  "schema_version": 1,
  "tool_version": "0.1.0",
  "summary": {
    "build_scripts": 47,
    "recognized": 38,
    "approved": 6,
    "reported": 3,
    "proc_macros_not_analyzed": 12
  },
  "packages": [
    {
      "name": "example-dropper",
      "version": "1.0.0",
      "source": "registry+https://github.com/rust-lang/crates.io-index",
      "status": "reported",
      "build_script_sha256": "...",
      "files": ["build.rs"],
      "findings": [
        {
          "rule": "net",
          "severity": "high",
          "file": "build.rs",
          "line": 12,
          "column": 5,
          "message": "ureq::get",
          "snippet": "let resp = ureq::get(url).call()?;"
        }
      ]
    }
  ],
  "proc_macros": [
    { "name": "serde_derive", "version": "1.0.0", "source": "..." }
  ]
}
```

recognized も含めた全パッケージを出力します．`schema_version` は互換性のない変更時に上げます．

## 既知の限界

攻撃者は本ツールのルールを知りえます．以下の手口は回避されえます．

- 許可された操作の範囲内に収まる悪性コード (例: 機密ファイルを読んで `OUT_DIR` に書き込み，成果物に埋め込む)．
- 許可リストの build-dependency のライブラリ側や，外部ファイル (C ソース，ビルドスクリプト) に悪性処理を置きます．
- マクロ内や関数をまたぐ値の流れに悪性コードを隠します (未知のマクロの使用自体は報告する)．
- `exec-arg` の危険なフラグは列挙であり，網羅を保証しません．

一部は現在の挙動をテストで固定しています (`tests/fixtures/known-misses/`)．

## 評価

<!-- v0.1 完了時に掲載します -->

- **コーパス評価**: crates.io のダウンロード数上位クレートに対する recognized 率と finding 別の内訳．未実施．
- **dogfooding**: cargo-buildscan 自身の依存に対する結果と，承認理由．未実施．

## ライセンス

本リポジトリは以下のデュアルライセンスを採用しています．

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))
