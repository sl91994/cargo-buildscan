# cargo-buildscan 設計 (v0.1)

本書は判定ルールとその根拠を定めます．CLI，出力形式，finding の重大度などの外部仕様は [README](README.md) を正とします．

## 1. 目的と範囲

依存クレートの build script を **実行せずに** 静的解析し，**既知の危険な操作** (ネットワーク，実行権限の付与，rustc 以外のプロセス起動，`OUT_DIR` 外と明らかな書き込み) を根拠となるコード位置付きで報告します．

v0.1 は **拒否リスト方式** です．明らかに怪しい操作だけを報告し，それ以外は報告しません．このため「報告されなかったもの = 定型の操作しかしない」とは言えません．定型の操作しかしないことの確認 (許可リスト方式) は v0.2 で導入します (付録)．

v0.1 では以下を扱いません．

- build script 以外の解析 (proc macro，build-dependency のライブラリ側，C ソースや `CMakeLists.txt` などの外部ファイル)．
- build script のルートファイル以外の読み込み (外部の `mod`，`include!` 類)．読み込まずに `unanalyzable` とします (§3.4)．
- マクロの中身の解析，値の流れの追跡 (変数をたどらない)．
- 拒否リスト (§3.3) にない操作．環境変数の読み取り，`OpenOptions`，リンク作成，`exec-arg`，`build-dep`，`macro` などのルールは v0.2 で扱います．
- ベースライン，JSON 出力，メタデータ由来のシグナル (typosquat 名など)，diff モード，プラットフォームによる絞り込み，ワークスペースメンバーの解析，SARIF 出力．

## 2. 脅威モデル

- **攻撃者**: crates.io に悪性の build script を含むクレートを公開できます (新規公開，またはメンテナアカウントの乗っ取り)．推移的依存の奥に置かれる可能性があります．
- **v0.1 が想定する攻撃**: 本ツールを意識せずに書かれた悪性の build script (`proc-macro1` 型)．**ルールを知る攻撃者による回避には耐えません．** マクロの中や変数経由に操作を置くだけで回避できます (§2.1)．
- **信頼するもの**: `cargo`，crates.io のインデックス，利用者自身の環境変数と cargo の設定 (`RUSTFLAGS` など)．
- **前提**: 利用者は reported になった build script を実際に読むものとします．

### 2.1 既知の限界

- 拒否リストにない操作は報告しません．ファイルの読み取り，環境変数の読み取り，`OpenOptions` による書き込み，リンク作成，FFI，外部プログラムを指定する引数 (`cc::Build::compiler`，`cargo:rustc-link-arg` など) が該当します．
- マクロの中身は解析しません (`println!("{:?}", Command::new("sh").spawn())` は報告されない)．例外は `fs-write` の書き込み先の式に含まれるマクロで，文字列リテラルだけを拾います (§3.5)．
- 値の流れを追いません．変数経由の書き込み先 (`let p = "/tmp/x"; fs::write(p, ..)`) は報告しません．
- `type` エイリアス (`type C = std::process::Command;`) は追跡しません．
- build-dependency のライブラリ側に悪性ロジックを置かれると検出できません．
- 外部ファイル (C ソース，ビルドスクリプト) は解析しません．同梱された実行ファイルの検出は cargo-deny の `[bans.build]` に任せます．

## 3. 判定モデル

### 3.1 方針

1. build script のルートファイル 1 つだけを構文解析します．
2. 拒否リスト (§3.3) の操作が現れれば finding にします．
3. 解析しない入力 (§3.4) は `unanalyzable` の finding にします．難しい入力は解析を頑張らず，報告して人間に渡します．
4. finding が 0 件の build script を **recognized** とします．recognized は「既知の危険な操作が見つからなかった」ことだけを意味します．

監視対象が判定できない形で現れたときの扱いはルールごとに決めます．正当な build script でもよく現れる形 (変数を書き込み先に使うなど) は報告せず，正当な build script ではまれな形 (関数ポインタとしての使用など) は報告します．

### 3.2 名前の照合

syn は名前解決をしないため，次のように近似します．

- ファイルごとに，グロブでない `use` (別名 `as`，入れ子の `{}`，`self` を含む) と `extern crate ... as ...` から，名前 → 完全パスの表を作ります．モジュールのスコープは区別せず，同じ名前に複数の候補があればすべてを照合します．
- パスの先頭のセグメントを表で置き換えてから照合します (`use std::process::Command as C;` の後の `C::new` は `std::process::Command::new`)．先頭の `::` は除きます．
- 関数と型は **パスの末尾** で照合します (`Command::new` は `std::process::Command::new` にも `x::Command::new` にも一致)．ユーザー定義の同名の型も対象になりますが，過検出側に倒します．
- モジュールは **パスの先頭** で照合します (`std::net`)．
- メソッド呼び出しは名前だけで照合します．型推論は行いません．
- 監視対象のモジュールからのグロブインポートは照合せず，`unanalyzable` とします (§3.4)．

### 3.3 拒否リスト (finding)

重大度は README の表を正とします．重大度は表示順にのみ使い，終了コードには影響しません．

| ID               | 条件                                                                                                                                                                                                                                                       |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `net`            | `std::net`，`std::os::unix::net` の使用 (`use` を含む)．既知の HTTP クレート (`reqwest`，`ureq`，`curl`，`hyper`，`attohttpc`，`minreq` など) のパスと `extern crate`．build-dependency に既知の HTTP クレートを持つこと (メタデータで判定，位置なし) |
| `set-executable` | 名前が `set_mode`，`from_mode`，`set_permissions` の関数・メソッド・パス                                                                                                                                                                                  |
| `spawn`          | program が §3.5 を満たさない `Command::new`．`Command::new` の呼び出し以外の形での出現                                                                                                                                                                     |
| `fs-write`       | `env::temp_dir`，`env::home_dir` の使用．書き込み系の関数の書き込み先が危険なリテラル (§3.5) を含むこと．書き込み系の関数の呼び出し以外の形での出現                                                                                                        |
| `unanalyzable`   | §3.4 の入力                                                                                                                                                                                                                                                |

- **書き込み系の関数**: `fs::write`，`File::create`，`fs::copy` (書き込み先は第 2 引数)，`fs::rename` (両方の引数)，`fs::create_dir` / `create_dir_all`，`fs::remove_file` / `remove_dir` / `remove_dir_all`．
- `env::temp_dir` / `env::home_dir` は，書き込みに使うかどうかによらず報告します．build script の書き込み先は `OUT_DIR` であるべきで，正当な build script ではまれなうえ，変数を経由した書き込み (`let p = env::temp_dir().join("x"); fs::write(&p, ..)`) を値の追跡なしで捉えられるためです．
- build-dependency の HTTP クレートをメタデータで判定するのは，`Cargo.toml` の改名 (`r = { package = "reqwest" }`) でコード上の名前を変えられるためです．
- 監視対象の名前，HTTP クレート，書き込み系の関数，危険なリテラル，グロブを禁じるモジュールの一覧は `config.rs` の 1 か所にまとめます．

**任意 (余裕があれば v0.1 に入れる)**: 以下は小さいため，v0.1 に入れてもかまいません．入れた場合は README の表にも加えます．

| ID           | 条件                                                                                                                                                                        |
| ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `env-secret` | 名前が文字列リテラルの `env::var` / `var_os` / `env!` / `option_env!` で，機密パターン (`HOME`，`SSH_*`，`AWS_*`，`*_TOKEN`，`*SECRET*` など) に一致．`env::vars` / `vars_os` |
| `unsafe`     | `unsafe` ブロック，`extern` ブロック，`asm!` / `global_asm!`                                                                                                                |

### 3.4 解析しない入力 (`unanalyzable`)

| 入力                                                                                    | 理由                                                                     |
| --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| 構文エラー                                                                              | 解析できない．                                                           |
| サイズ上限 (既定 1 MiB) を超えるファイル                                                | 解析に時間がかかりうる．位置なし．                                       |
| 外部ファイルの `mod foo;` (`#[path]` 付きを含む)                                        | 読み込まない．インラインの `mod foo { .. }` は解析する．                 |
| `include!` / `include_str!` / `include_bytes!`                                          | 読み込まない．                                                           |
| `std`，`std::process`，`std::fs`，`std::env`，`std::os` 配下からのグロブインポート      | 未修飾の名前 (`write(..)`，`temp_dir()`) を照合できない．               |

ルートファイル以外を読み込まないため，パッケージルート外を指すパス，隣接するクレートの読み込み，シンボリックリンクを考える必要がありません．`std::net` からのグロブインポートは `std::net` の使用として `net` になります．

### 3.5 引数の判定 (同じ式の中のみ)

引数の式だけを見て判定します．変数はたどりません．

**program (`spawn`)**: 式が以下だけで組み立てられていれば許可します．

- 文字列リテラル `"rustc"`．
- 名前が文字列リテラル `"RUSTC"`，`"RUSTC_WRAPPER"`，`"RUSTC_WORKSPACE_WRAPPER"` の `env::var` / `env::var_os`．
- これらをつなぐ形: 参照・参照外し，`?`，括弧，メソッド呼び出し (レシーバと引数のすべてを同じ規則で検査し，クロージャは本体を検査する)，`OsString::from`，`PathBuf::from`，`String::from`，`Some`，`Ok` の呼び出し．

それ以外 (変数，他の関数呼び出し，フィールド，マクロ，他のリテラル) を含めば報告します．

- 許可: `Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))`
- 報告: `Command::new("git")`，`Command::new(rustc)` (変数)，`Command::new(cargo_env_var("RUSTC"))` (ヘルパー関数)

変数経由の rustc 起動 (`let rustc = env::var("RUSTC")?; Command::new(rustc)`) も報告されます．過検出ですが，読めば判断できる短いコードがほとんどです．

**書き込み先 (`fs-write`)**: 書き込み先の式の中に，以下の文字列リテラルがあれば報告します．式に含まれるマクロ (`format!` など) は，トークン列から文字列リテラルだけを拾います．

- `/`，`\`，`~`，ドライブ文字 (`C:` など) で始まる．
- `..` を含む．

例:

- 報告: `fs::write("/tmp/x", ..)`，`File::create(format!("/tmp/{}", name))`，`fs::copy(src, "../../x")`
- 報告しない: `fs::write(out_dir.join("x"), ..)`，`fs::write(p, ..)` (変数，§2.1)

## 4. 処理の流れ

```
Resolve -> Locate -> Load -> Parse -> Judge -> Report
```

- **Resolve**: `cargo metadata --format-version 1 --all-features` を `--filter-platform` なしで実行します．`--all-features` が有効にするのはワークスペースメンバーのフィーチャーのみであり，結果は `Cargo.lock` のパッケージ集合に一致します (小規模な例で確認済み，§9)．
- **Locate**: kind に `custom-build` を含む target の `src_path` を得ます．`proc-macro` は未解析として一覧に記録します．build-dependency のパッケージ名を記録します (`net` のメタデータ判定に使う)．
- **Load**: build script のルートファイルだけを読み込みます．サイズ超過は `unanalyzable`，読み込みの失敗 (I/O エラー) は status `error` とします．
- **Parse**: `syn` (`full`，`visit`，`parsing`) と `proc-macro2` (`span-locations`) を使います．失敗は `unanalyzable` とします．§3.4 の入力もここで検出します．
- **Judge**: `syn::visit` で走査し，§3.3 の finding を集めます．
- **Report**: human 出力のみです．

## 5. 出力の安全性

報告に含まれるクレート由来の文字列 (パス，パッケージ名，source，スニペット，`cargo metadata` の stderr) は攻撃者が制御できます．

- 制御文字 (ANSI エスケープ，CR，Unicode の双方向制御文字など) をエスケープします．
- スニペットは最大長で切り詰めます．

## 6. 依存方針

サプライチェーン対策ツール自身の依存が多いと説得力がありません．

- 本クレートは build.rs を持ちません．
- 直接の依存は `cargo_metadata`，`syn`，`proc-macro2`，`lexopt` に限り，dev-dependency は `insta` と `serde_json` (`cargo_metadata` の依存として既に含まれる) のみとします．`syn` と `proc-macro2` は `default-features = false` で必要な機能だけを有効にします．
- `anyhow` / `thiserror` は直接使わず，エラー型は自前の enum とします．どちらも build script を持つためです．
- lib + bin 構成としますが，lib の API は安定性を保証しません (`#[doc(hidden)]`)．

## 7. テスト

- **単体**: finding ごとに該当・非該当の両方．名前の照合と §3.5 の判定は個別に．
- **fixture** (`tests/fixtures/`，誤ってビルドされないよう `.rs.txt` とする)．ルートファイル 1 つだけを解析するため，各 fixture は 1 ファイルです．
  - `malicious/`: `proc-macro1` 型 (`net`，`fs-write`，`set-executable`，`spawn` がすべて出る)，§3.4 の入力．**動作するペイロードは含めず**，URL は `example.invalid` にします．
  - `benign/`: 数件．cargo の環境変数を読んで `cargo:` を出力する，`RUSTC` で版を調べる，`OUT_DIR` に書く，など．

## 8. v0.1 完了の定義

- [ ] README の CLI，終了コード，human 出力が動作します．
- [ ] §3.3 の finding (任意を除く) と §3.4 の `unanalyzable` が実装され，該当・非該当のテストがあります．
- [ ] `malicious/` の fixture で期待する finding がすべて出て，`benign/` の fixture が recognized になります．
- [ ] README の「既知の限界」が §2.1 と一致しています．

## 9. 未決事項

- `--all-features` の解決結果と `Cargo.lock` の一致を，複数メンバーのワークスペースやプラットフォーム固有の依存でも確認します (v0.2)．
- 名前 `cargo-buildscan` が crates.io で空いているか，公開時に再確認します．

## 付録: v0.2 以降の候補

- **許可リスト方式への移行**: build script から監視対象の操作を抽出し，定型の範囲 (rustc の起動，`OUT_DIR` への書き込み，許可リストの build-dependency など) で説明できないものをすべて報告します．関数内の由来の追跡，マクロ引数の解析，`exec-arg`，`build-dep`，`macro`，`env-dynamic` を含みます．設計の詳細は v0.1 縮小前の本書 (`git show 106126c:DESIGN.md`) にあります．
- **ベースライン**: build script のハッシュによるレビュー済みの記録と `approve` サブコマンド．許可リスト方式で残る誤検知を吸収するために導入します．
- **読み込みの拡張**: `mod` / `#[path]` / `include!` の解決，パッケージルート外の参照の判定．
- **ルールの追加**: `OpenOptions`，リンク作成，`env-secret` / `unsafe` (v0.1 で未実装の場合)．
- **出力と評価**: JSON 出力，CLI の E2E テスト，dogfooding，コーパス評価，cargo-deny の導入．
- 同一ファイル内のヘルパー関数の展開，`--since`，`--filter-platform`，`--include-workspace`，`--show-recognized`，メタデータ由来のシグナル，SARIF 出力，proc macro の解析，cargo-vet / cargo-deny との連携．
