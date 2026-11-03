# cargo-buildscan 設計 (v0.1)

本書は判定ルールとその根拠を定めます．CLI，出力形式，ベースライン形式，finding の重大度などの外部仕様は [README](README.md) を正とします．

## 1. 目的と範囲

依存クレートの build script を **実行せずに** 静的解析し，監視対象の操作を定型の範囲でしか行わないものを除外して，残りを根拠となるコード位置付きで報告します．

目的は **レビュー対象の削減** です．v0.1 が約束するのは「読む対象を，機械的に説明できる根拠付きで減らす」ことであり，報告されなかったものの安全ではありません．

v0.1 では以下を扱いません．

- build script 以外の解析 (proc macro，build-dependency のライブラリ側，C ソースや `CMakeLists.txt` などの外部ファイル)．
- 関数をまたぐ値の追跡，マクロ展開．
- 難読化やプロセスの切り離しの検出．それ自体は害を持たず，害のある操作は別の finding として現れるためです．
- メタデータ由来のシグナル (typosquat 名など)，diff モード，プラットフォームによる絞り込み，ワークスペースメンバーの解析，SARIF 出力．

## 2. 脅威モデル

- **攻撃者**: crates.io に悪性の build script を含むクレートを公開できます (新規公開，またはメンテナアカウントの乗っ取り)．推移的依存の奥に置かれる可能性があります．本ツールの存在とルールを知りえます．
- **信頼するもの**: `cargo`，crates.io のインデックス，利用者自身の環境変数と cargo の設定 (`RUSTFLAGS` など)．
- **前提**: 利用者は reported になった build script を実際に読むものとします．

### 2.1 既知の限界

- ファイルの **読み取り** は監視しません．読み取った情報を `OUT_DIR` に書き込み，`include!` などで成果物に埋め込めば，許可された操作しか現れません．
- 許可リスト (§3.2) の build-dependency は中身を解析せずに信頼します．悪性ロジックをそのライブラリ側に置かれると検出できません．
- 外部ファイル (C ソース，ビルドスクリプト) は解析もハッシュもしません．同梱された実行ファイルの検出は cargo-deny の `[bans.build]` に任せます．
- マクロ内の悪性コード．ただし未知のマクロの使用自体は報告します．
- 関数をまたぐ値の流れ．
- 危険な引数のパターン (§3.4) は列挙であり，網羅を保証しません．

## 3. 判定モデル

### 3.1 方針

1. build script から **fact** (監視対象の操作) を抽出します．
2. 許可パターン (§3.2) に一致しない fact は，すべて finding になります．
3. finding が 0 件の build script を **recognized** とします．

監視対象の名前 (`Command`，`TcpStream`，`set_mode` など) が現れたとき，許可パターンで説明できなければ finding にします．**説明できない出現は報告側に倒します．**

### 3.2 許可パターン

| 操作                     | 許可される条件                                                                                                                                                        |
| ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 環境変数の読み取り       | 名前が文字列リテラルで，機密パターン (§3.3) に一致しない                                                                                                              |
| プロセス起動             | program の由来 (§3.5) が `RUSTC` / `RUSTC_WRAPPER` / `RUSTC_WORKSPACE_WRAPPER` の環境変数かリテラル `"rustc"` のみで，引数が §3.4 を満たす                            |
| ファイルの書き込み・削除 | 書き込み先の由来が `OUT_DIR` 配下と判定できる．リンク作成 (`symlink`，`hard_link`) は常に finding とします                                                            |
| build-dependency の使用  | `cc`，`pkg-config`，`autocfg`，`version_check`，`rustc_version`，`bindgen`，`cfg_aliases`，`rustversion` で，source が crates.io．ただし §3.4 の API は別に検査します |
| `cargo:` ディレクティブ  | §3.4 に該当しない                                                                                                                                                     |

- 環境変数を cargo の設定するものに限定しないのは，`ZSTD_SYS_USE_PKG_CONFIG` のようなクレート固有の設定変数が一般的なためです．危険な読み取りは機密パターンで捉えます．
- `cmake` は許可しません．`CMakeLists.txt` は任意のコマンドを実行でき，その中身を解析しないためです．
- build-dependency の同一性は，コード上の名前ではなく `cargo metadata` で解決したパッケージ名と source で判定します．`cc = { package = "other" }` のような改名は許可されません．
- リンク作成を常に finding とするのは，リンク先を検査しないと `OUT_DIR` 外への書き込みの経路になるためです．

### 3.3 finding

重大度は README の表を正とします．重大度は表示順にのみ使い，終了コードには影響しません．

| ID               | 条件                                                                                                            |
| ---------------- | --------------------------------------------------------------------------------------------------------------- |
| `net`            | `std::net` の使用，既知の HTTP クレート (`reqwest`，`ureq`，`curl`，`hyper`，`attohttpc`，`minreq` など) の使用 |
| `set-executable` | `PermissionsExt::set_mode` / `from_mode`，`set_permissions`                                                     |
| `env-secret`     | 機密パターンに一致する環境変数の読み取り，`env::vars` / `vars_os`，`env::home_dir`                              |
| `exec-arg`       | 外部プログラムを指定しうる API・引数・ディレクティブ (§3.4)                                                     |
| `unanalyzable`   | 構文エラー，動的なパスの `include!` 類，パッケージルート外を指すパス，サイズ超過                                |
| `spawn`          | program が §3.2 の条件を満たさないプロセス起動 (`Command::new`，`CommandExt::exec` など)                        |
| `fs-write`       | `OUT_DIR` 配下と判定できない書き込み・削除・リネーム，リンク作成                                                |
| `env-dynamic`    | 名前が文字列リテラルでない環境変数の読み取り                                                                    |
| `build-dep`      | 許可リスト外の build-dependency の使用 (関数，マクロ，属性)                                                     |
| `unsafe`         | `unsafe` ブロック，`extern` ブロック，`asm!` / `global_asm!`                                                    |
| `macro`          | 標準・許可リスト外のマクロ呼び出し，`macro_rules!` の定義                                                       |

機密パターン: `HOME`，`USERPROFILE`，`SSH_*`，`AWS_*`，`GITHUB_*`，`*_TOKEN`，`*_KEY`，`*SECRET*`，`*PASSWORD*` など．

監視対象の名前，機密パターン，許可リスト，§3.4 の一覧は `config.rs` の 1 か所にまとめます．

### 3.4 外部プログラムを指定しうる引数 (`exec-arg`)

許可された操作 (rustc の起動，`cc` によるコンパイル，リンク) は，引数しだいで任意のプログラムを実行させられます．クレートに実行ファイルを同梱し，`CARGO_MANIFEST_DIR` 経由でそのパスを指せば，`set-executable` も `fs-write` も現れません．このため以下を finding とします．

- **プログラムを指定する API**: `cc::Build` の `compiler`，`archiver`，`ranlib` など．引数によらず常に finding．
- **フラグを渡す API**: 許可された rustc 起動の `arg` / `args`，`cc::Build` の `flag` / `flags` / `flag_if_supported` など，`bindgen` の `clang_arg` / `clang_args`．引数の由来が `Unknown` を含むか，リテラルが危険パターンに一致すれば finding．
  - rustc: `-C linker`，`linker=`，`-Z`，`--extern`，`@` で始まる引数ファイル．
  - C コンパイラ: `-B`，`-fuse-ld`，`--ld-path`，`-fplugin`，`-specs`，`-Wl,`，`-Xlinker`，`-Xclang`．
- **`cargo:` ディレクティブ**:
  - `rustc-link-arg` 系 (`-bins`，`-bin`，`-tests`，`-examples`，`-benches`，`rustc-cdylib-link-arg` を含む) は常に finding．
  - 標準出力への出力のうち，書式文字列の先頭がリテラルの `cargo:` で始まらないもの (`println!("{}", s)`，`io::stdout()` への書き込み) も，ディレクティブ名を決められないため finding．

### 3.5 由来の追跡 (関数内のみ)

式の由来を，値の出どころの **集合** として求めます．要素は `Env(名前)`，`Literal(文字列)`，`Unknown` の 3 種です．

- `env::var("X")` / `env::var_os("X")` は `{Env(X)}`，文字列リテラルは `{Literal(..)}` です．
- ローカル変数は束縛時の由来を引き継ぎます (シャドーイングは後の束縛を優先し，再代入は和集合をとる)．
- メソッド呼び出しはレシーバと引数の由来の和集合です．引数がクロージャのときは本体の由来を加えます．
- `PathBuf::from`，`Path::new`，`OsString::from`，`String::from`，`Some`，`Ok`，参照・参照外し，`?`，`format!`，`concat!` などは引数の由来の和集合です．一覧は `config.rs` に置きます．
- それ以外の関数呼び出し，フィールド，関数の引数，`static` / `const` は `{Unknown}` です．

判定:

- **program**: 集合が空でなく，`Unknown` を含まず，全要素が `Env("RUSTC")`，`Env("RUSTC_WRAPPER")`，`Env("RUSTC_WORKSPACE_WRAPPER")`，`Literal("rustc")` のいずれかです．
  - 許可: `Command::new(env::var_os("RUSTC").unwrap_or(OsString::from("rustc")))`
  - 不許可: 同一ファイル内のヘルパー関数 `cargo_env_var("RUSTC")` 経由 (`Unknown` になる)．
- **書き込み先**: `Env("OUT_DIR")` を含み，`Unknown` と他の `Env` を含まず，すべての `Literal` が無害な相対パス (先頭が `/`，`\`，ドライブ文字でなく，`..` を含まない) です．
  - 許可: `PathBuf::from(env::var("OUT_DIR")?).join("probe")`
  - 不許可: `format!("/tmp/{}", out_dir)`
- **フラグ**: §3.4 のとおりです．

### 3.6 名前の照合

syn は名前解決をしないため，保守的に近似します．

- モジュールごとに `use` と `type` エイリアスの表を作り，別名を正規化します (`use std::process::Command as C;` の後の `C::new` は `Command::new`)．
- 監視対象の名前は **パスの末尾の識別子** で照合します．ユーザー定義の同名の型も対象になりますが，過検出側に倒します．
- 監視対象のモジュールからのグロブインポートがあるモジュールでは，未修飾の名前も照合します．
- 型推論は行いません．メソッド呼び出しは名前だけで照合します．

## 4. 処理の流れ

```
Resolve -> Locate -> Load -> Parse -> Extract facts -> Judge -> Baseline -> Report
```

- **Resolve**: `cargo metadata --format-version 1 --all-features` を `--filter-platform` なしで実行します．`--all-features` が有効にするのはワークスペースメンバーのフィーチャーのみであり，結果は `Cargo.lock` のパッケージ集合に一致します (小規模な例で確認済み，§10)．
- **Locate**: kind に `custom-build` を含む target の `src_path` を得ます．`proc-macro` は未解析として一覧に記録します．build-dependency のコード上の名前 (改名を考慮) から，解決済みのパッケージ名と source への対応表を作ります．
- **Load**: `mod` と `#[path]` を再帰的に解決します．`include!` / `include_str!` / `include_bytes!` は引数がリテラルのときのみ解決します．パッケージルート外を指すパス (シンボリックリンクは実体パスで判定) とサイズ超過 (既定 1 MiB) は `unanalyzable` とします．レジストリでは他クレートが隣接しており，`../` で別クレートを読み込めるためです．
- **Parse**: `syn` (`full`，`visit`，`parsing`) と `proc-macro2` (`span-locations`) を使います．失敗は `unanalyzable` とします．

## 5. 出力の安全性

報告に含まれるクレート由来の文字列 (パス，リテラル，スニペット) は攻撃者が制御できます．

- human 出力では制御文字 (ANSI エスケープ，CR，Unicode の双方向制御文字など) をエスケープします．
- スニペットは最大長で切り詰めます．
- JSON は `serde_json` の通常のエスケープに任せます．

## 6. ベースライン

- **ハッシュ**: Load で読み込んだ全ファイル (`include_bytes!` の対象を含む) をパッケージルートからの相対パスで並べ替え，`相対パス \0 内容 \0` を連結した SHA-256 です．build script が実行時に読むファイル，C ソース，外部ビルドスクリプト，build-dependency のコードは含みません．
- **照合キー**: `(name, source, build_script_sha256)` です．version は使わず，build script が同一ならバージョンが上がっても再レビュー不要とします．source を含めるのは，別レジストリや git から取得した同名クレートに承認を流用させないためです．
- approved は「build script の Rust ソースが承認時と同一」であることだけを意味します．`spawn`，`exec-arg`，`build-dep` を持つものはハッシュ対象外のファイルを実行しうるため，承認時に注意が必要です (README に明記する)．

## 7. 依存方針

サプライチェーン対策ツール自身の依存が多いと説得力がありません．

- 本クレートは build.rs を持ちません．
- 直接の依存は `cargo_metadata`，`syn`，`proc-macro2`，`serde`，`serde_json`，`toml`，`sha2`，`lexopt` に限り，dev-dependency は `insta` のみとします．`syn` と `proc-macro2` は `default-features = false` で必要な機能だけを有効にします．
- `anyhow` / `thiserror` は直接使わず，エラー型は自前の enum とします．どちらも build script を持ち，ヘルパー関数経由の環境変数読み取りで reported になる見込みのためです．
- lib + bin 構成としますが，lib の API は安定性を保証しません (`#[doc(hidden)]`)．
- CI で自身の依存に対して実行し (dogfooding)，reported は読んだうえで理由付きでベースラインに記録します．cargo-deny も併用します．

## 8. テスト

- **単体**: finding ごとに該当・非該当の両方．名前の照合と由来の追跡は個別に．
- **fixture** (`tests/fixtures/`，誤ってビルドされないよう `.rs.txt` とする)．
  - `benign/`: cargo の環境変数を読んで `cargo:` を出力する，`RUSTC` で版を調べる，`OUT_DIR` に書く，`cc` でコンパイルする，など．
  - `malicious/`: `proc-macro1` 型 (`net`，`fs-write`，`set-executable`，`spawn` がすべて出る)，同梱した実行ファイルを `cc::Build::compiler` や `rustc-link-arg` で指すもの (`exec-arg` が出る)．**動作するペイロードは含めず**，URL は `example.invalid` にします．
  - `known-misses/`: §2.1 の限界を示すもの (機密ファイルを読んで `OUT_DIR` に書くだけ，など)．**recognized になることを** 固定し，限界が文書と一致することを保証します．
- **スナップショット**: 出力を `insta` で固定します．
- **コーパス評価**: crates.io のダウンロード数上位 N 件のうち build script を持つものをオフラインで解析し，recognized 率と finding 別の内訳を README に載せます．目標値は設けません．
  - 参考: 手元の 45 クレートを grep で調べた予備調査では，旧設計で約 24/45 が recognized の見込みでした．主な原因 (クレート固有の環境変数，`RUSTC_WRAPPER`，`cfg_aliases!` など) は本設計で許可に変更済みです．ヘルパー関数を使うもの (anyhow，proc-macro2 など) は引き続き reported になる見込みです．

## 9. v0.1 完了の定義

- [ ] README の CLI，終了コード，出力形式が動作します．
- [ ] §3.3 の全 finding が実装され，該当・非該当のテストがあります．
- [ ] ベースラインの approve と照合が動作します．
- [ ] `malicious/` と `known-misses/` の fixture が期待どおりの結果になります．
- [ ] コーパス評価と dogfooding の結果を README に載せています．

## 10. 未決事項

- `--all-features` の解決結果と `Cargo.lock` の一致を，複数メンバーのワークスペースやプラットフォーム固有の依存でも確認します．
- §3.4 の API と危険パターンの網羅性．`cc` / `bindgen` / `pkg-config` の公開 API を確認します．
- 機密パターンの範囲と，末尾一致による過検出の頻度．コーパス評価で確認します．
- 名前 `cargo-buildscan` が crates.io で空いているか，公開時に再確認します．

## 付録: v0.1 以降の候補

- 同一ファイル内のヘルパー関数の展開 (anyhow，proc-macro2 などの recognized 化)．
- `--since`，`--filter-platform`，`--include-workspace`，`--show-recognized`．
- メタデータ由来のシグナル，難読化・切り離しの検出，SARIF 出力．
- build-dependency のライブラリ側，proc macro の解析．
- cargo-vet / cargo-deny との連携．
