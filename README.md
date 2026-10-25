# Suffolk 🐑

> **Pure Rust Translation Filter & Merger for SheepFamily**  
> Okapi Framework の Filter (`tikal -x`) および Merge (`tikal -m`) 機能を Rust でゼロから軽量・高速・スタンドアロンに再実装した翻訳ツールチェーンです。

Web 翻訳支援ツール（CAT）**SheepCombWeb** のデータスキーマ（[`type.ts`](./type.ts) 内の `ShWvData`）を第一級出力形式（SSOT）としてネイティブサポートし、標準規格である **XLIFF 1.2** にも対応しています。

---

## 🌟 主な特徴と強み

1. **Pure Rust 設計（依存ライブラリゼロ）**
   - C/C++ ネイティブライブラリや Java 実行環境（JRE）への依存が一切ありません。
   - 単一の自己完結型バイナリ（`suffolk.exe`）として動作するだけでなく、**WebAssembly (WASM)** に完全対応。ブラウザ（SheepCombWeb SPA）や VS Code 拡張機能（SheepWeave）のインメモリ環境で即座に直接実行可能です。

2. **タグ最小化（Tag Minimization）の標準装備**
   - Word や PowerPoint で編集履歴やスペルチェックが原因で発生する「同一書式なのに寸断されたテキスト（ラン）」を自動結合します。
   - 視覚的な装飾（太字・斜体・文字色・蛍光ペンなど）の境界のみに `{@0}`, `{@1}` プレースホルダーを最小限配置し、翻訳者に不要なタグノイズを見せません（既存 CAT ツール仕様に準拠、後方互換で `{0}` も復元可）。

3. **高度な Excel (XLSX) 多言語抽出と安全なマージ**
   - **翻訳者にやさしい CSV 設定**: `sheet,source,target,context,skip_rows` 形式の CSV ファイルまたは CLI フラグで柔軟に原文列・訳文列・補足情報列を指定可能。
   - **ルビ・ふりがな (`<rPh>`) の自動除外**: 日本語 Excel ファイルでふりがな情報が重複抽出される問題（例: `起動キドウ`）を防止。
   - **安全な `inlineStr` 書き込み方式**: 共通文字列テーブル（`sharedStrings.xml`）を再インデックスするリスクを避け、シート単位で安全かつ高速にセルを再構築。列のアルファベット順序を厳密に保証するため、Excel や LibreOffice で警告なしに即座に開けます。

4. **翻訳作業を考慮したメタ情報とメモの分離**
   - **`note`**: シート名、セル番地、スライド番号、スピーカーノート注記、コンテキスト列の内容など、翻訳前の原稿情報を集約。
   - **`memo`**: 翻訳作業者が作業中に気づいたメモや疑問点を残す専用領域として、抽出時は常にクリーンな空状態（`null`）を保持。

5. **文字コード自動判定**
   - 日本語 Windows 環境で混在する **Shift-JIS (Windows-31J / CP932)** と **UTF-8** を自動判別し、文字化けなく処理します。

6. **PowerPoint スピーカーノート対応**
   - スライド本文と発表者用スピーカーノートを明確に区別して抽出・復元します。

7. **HTML / Markdown 構文保護**
   - HTML の `<script>` や `<style>`、Markdown の YAML フロントマターやフェンスコードブロック（```` ``` ````）を厳密に保護し、翻訳対象から除外します。

8. **マージ失敗時のセーフティネット（CSV & TXT フェイルセーフ救出）**
   - 万が一の元ファイル（スケルトン）破損やレイアウト破綻時でも、翻訳作業の成果物を **対照 CSV** および **訳文のみプレーン TXT** として即座に救出できます。
   - **対照 CSV (`idx,source,target,note`)**: 日本語 Windows の Excel で即座に開ける UTF-8 BOM 付き。どの原文がどのシート・スライド・段落に対応しているかを明記。
   - **プレーン TXT (訳文のみ)**: 人間が InDesign や Word へ手作業でコピペできるよう、行順序を維持しつつ `{@0}` などの装飾タグ記号を自動除去。
   - **自動緊急リカバリ**: マージ処理中にエラーが発生した場合でも、強制終了前にフェイルセーフファイルが自動生成されます。

---

## 📑 目次

- [📁 対応フォーマット](#-対応フォーマット)
- [🛠️ ビルドとテスト（CLI）](#️-ビルドとテスト)
- [🚀 CLI の使い方](#-cli-の使い方)
- [🧪 テストデータによる検証ガイド](#-テストデータによる検証ガイド)
- [🔍 Okapi Tikal との比較検証](#-okapi-tikal-との比較検証)
- [🌐 WebAssembly (WASM) のビルドと使い方](#-webassembly-wasm-のビルドと使い方)
  - [1. WASM のビルド](#1-wasm-のビルド)
  - [2. 提供されるエントリポイント一覧](#2-提供されるエントリポイント一覧)
  - [3. システム引数（JSON オブジェクト）の仕様](#3-システム引数json-オブジェクトの仕様)
  - [4. ブラウザアプリ（Vue 3 / Nuxt 3 / SPA）での組み込み例](#4-ブラウザアプリvue-3--nuxt-3--spaでの組み込み例)
  - [5. VS Code 拡張機能（SheepWeave 等）での組み込み例](#5-vs-code-拡張機能sheepweave-等での組み込み例)
- [🏗️ クレート構成](#️-クレート構成)
- [🛣️ 今後のロードマップ](#️-今後のロードマップ)

---

## 📁 対応フォーマット

| フォーマット | 拡張子 | 抽出対象 | 書式保持 / 特記事項 |
| :--- | :--- | :--- | :--- |
| **Microsoft Word** | `.docx` | 本文段落、見出し | タグ最小化、太字/斜体/文字色/下線等の保持 |
| **Microsoft Excel** | `.xlsx` | 多言語列指定（A列原文、B列訳文等）または全セル | CSV設定対応、ふりがな(`rPh`)除外、`inlineStr`安全マージ |
| **Microsoft PowerPoint** | `.pptx` | スライド本文、スピーカーノート | スライド番号ソート、スピーカーノート注記 |
| **Adobe InDesign** | `.idml` | ストーリー本文 (`Stories/Story_*.xml`) | 論理読み順 (`StoryList`) ソート、タグ最小化、`<Br/>` 区切り保持 |
| **HTML** | `.html`, `.htm` | 本文ブロック要素 (`p`, `h1`〜`h6`, `li`, `td`, `button` 等) | `<script>`/`<style>`保護、インラインタグプレースホルダー化 |
| **Markdown** | `.md` | 見出し、段落、リスト項目、引用 | フロントマター・コードブロック保護、構文記号保持 |
| **Plain Text** | `.txt` | 行単位テキスト | UTF-8 / Shift-JIS (CP932) 自動判定 |
| *SVG 画像* | *.svg* | *(次期ロードマップ)* | アーキテクチャ準備済み |

---

## 🛠️ ビルドとテスト

### 前提環境
- [Rust ツールチェーン](https://rustup.rs/) (edition 2021 以降)

### リリースビルド
```bash
cargo build --release
```
ビルド完了後、`target/release/suffolk.exe`（Linux/macOS では `suffolk`）が生成されます。

### ユニットテスト実行
```bash
cargo test --all
```
すべてのテストが約 0.03 秒で高速に成功します。

---

## 🚀 CLI の使い方

Suffolk CLI は、抽出を行う `filter`（または `extract`, `-x`）と、復元を行う `merge`（または `-m`）の2つのサブコマンドを提供します。

### 1. 抽出 (Filter / Extract)

原稿ファイルから翻訳セグメントを抽出し、`ShWvData` JSON（デフォルト）または XLIFF 1.2 ファイルを生成します。
単一ファイルだけでなく、**複数ファイルの一括抽出（カンマ区切りまたはスペース区切り）** および **`-seg` による文単位セグメンテーション** に対応しています。
ShWvData 形式の出力は CAT ツール（SheepCombWeb）の標準仕様に合わせて **`project.json`** 固定（既に存在する場合は `project(1).json` と自動ナンバリングして上書き防止）で生成されます。

```bash
# 基本形 (デフォルトで project.json を出力、同名既存時は project(1).json)
suffolk filter <原稿ファイル>

# 文単位セグメンテーション (-seg / --seg) を有効化
suffolk filter -seg <原稿ファイル>

# 複数ファイルの一括抽出（カンマ区切り、またはスペース区切り）
suffolk filter file1.docx,file2.xlsx
suffolk filter file1.docx file2.xlsx file3.idml

# 複数ファイルを文単位でセグメント分割（project.json に統合）
suffolk filter -seg file1.docx,file2.xlsx

# XLIFF 1.2 形式で出力（複数ファイル指定時は各ファイルごとに 1:1 で .xlf を出力）
suffolk filter file1.docx file2.xlsx --to xliff

# 出力先パスを明示的に指定
suffolk filter <原稿ファイル> -o custom_name.json -s en -t ja

# タグ最小化を無効化（生のタグを忠実に保持）
suffolk filter <原稿ファイル> --raw-tags
```

#### Excel (XLSX) 多言語列フィルタの指定方法

##### 方法 A: CSV 設定ファイルを使用（推奨）
設定ファイル（例: `rules.csv`）を作成します:
```csv
sheet,source,target,context,skip_rows
Device_Config,A,B,C,1
```
- `sheet`: シート名（正規表現指定可。省略時は `.*`）
- `source`: 原文列（例: `A`）
- `target`: 訳文列（例: `B`。既に訳文がある場合は事前翻訳として取り込み）
- `context`: 補足説明列（例: `C`。`note` に追記）
- `skip_rows`: スキップする行番号（例: `1` で見出し行を除外）

実行コマンド:
```bash
suffolk filter test_data/Device_Config.xlsx --xlsx-config rules.csv
```

##### 方法 B: CLI フラグで直接指定
```bash
suffolk filter test_data/Device_Config.xlsx \
  --xlsx-sheet "Device_Config" \
  --xlsx-source A \
  --xlsx-target B \
  --xlsx-context C \
  --xlsx-skip-rows 1
```

---

### 2. 復元 (Merge / Rebuild)

確定した翻訳ファイル（`.shwv.json` または `.xlf`）から、翻訳文を元の文書骨格（スケルトン）に差し込み、完成文書（`.out.<拡張子>`）を出力します。
単一ファイルだけでなく、**複数ファイルが結合された ShWvData バンドルの一括逆展開マージ** や **複数 XLIFF の一括マージ** にも対応しています。
また、文単位セグメンテーション（`-seg`）で分割されていた文は、**段落・ブロック構造へ自動的に再結合**されてから安全に復元されます。

```bash
# 基本形 (引数省略時はカレントディレクトリの project.json を自動検出して復元)
suffolk merge

# ファイル名を指定して復元（project.json や project(1).json、任意の .shwv.json）
suffolk merge project.json
suffolk merge "project(1).json"

# 複数 XLIFF ファイルの一括マージ
suffolk merge file1.docx.xlf file2.xlsx.xlf

# スケルトンファイルや出力先を明示的に指定
suffolk merge project.json -s original.docx -o translated.docx

# XLIFF 1.2 ファイルからのマージ
suffolk merge <翻訳ファイル.xlf>

# Excel 多言語マージ（A列の原文に対応する確定訳文をB列に注入）
suffolk merge test_data/Device_Config.xlsx.shwv.json \
  --xlsx-source A \
  --xlsx-target B

# フェイルセーフ（CSV & TXT）も同時に出力
suffolk merge <翻訳ファイル.shwv.json> --failsafe
```

---

### 3. フェイルセーフ出力 (Export / Failsafe)

元ファイル（スケルトン）の紛失・破損やレイアウト破綻により、自動マージ（`.docx`, `.xlsx`, `.idml` 等の再構築）が困難になった場合でも、翻訳データを即座に救出するためのセーフティネット機能です。

#### 救出形式の詳細
| 形式 | 拡張子 | 特徴と用途 |
| :--- | :--- | :--- |
| **対照 CSV** | `.csv` | `idx,source,target,note` の 4 列構成。Excel で文字化けしない UTF-8 BOM 付き。どの訳文がどのセル/段落に対応しているかを確認・検収可能。 |
| **プレーン TXT** | `.txt` | **訳文 (`tgt`) のみを行単位で出力**。DTP オペレータや翻訳者が InDesign や Word の該当箇所へ手作業でコピペできるよう、`{@0}` などの装飾タグ記号を自動除去（段落ズレ防止のため未翻訳行は原文を保持）。 |

#### コマンド例

```bash
# 基本形: CSV と TXT（タグ除去済み）の両方を同時に生成 (<入力名>.failsafe.csv / .txt)
suffolk export <翻訳ファイル.shwv.json>

# CSV のみ出力（Excel 検収・外部ツール連携用）
suffolk export <翻訳ファイル.shwv.json> --to csv -o output.csv

# TXT のみ出力（InDesign/Word 手動貼り付け用、訳文のみ・タグ除去済み）
suffolk export <翻訳ファイル.shwv.json> --to txt -o output.txt

# TXT 出力時に {@0} などのタグ記号をあえて保持したい場合
suffolk export <翻訳ファイル.shwv.json> --to txt --raw-tags -o output_with_tags.txt

# merge コマンドの出力拡張子を .csv や .txt にすることでも直接エクスポート可能
suffolk merge <翻訳ファイル.shwv.json> -o emergency.csv
suffolk merge <翻訳ファイル.shwv.json> -o emergency.txt

# 通常のマージを実行しつつ、予防措置としてフェイルセーフも同時に出力
suffolk merge <翻訳ファイル.shwv.json> --failsafe
```

> **🛡️ 自動緊急リカバリ機能**:  
> `suffolk merge` 実行中に、万が一「元ファイル（スケルトン）が見つからない」「ファイルが破損している」「XML構文エラー」などの致命的なエラーが発生した場合でも、エラー終了する前に **緊急フェイルセーフファイル（`<原稿名>.failsafe.csv` および `<原稿名>.failsafe.txt`）が自動生成** されます。いかなる場合でも翻訳者の作業成果が失われることはありません。

---

## 🧪 テストデータによる検証ガイド

リポジトリ内の [`test_data/`](./test_data/) ディレクトリに用意されている各種検証ファイルを使って動作を確認できます。

### 1. Excel 多言語検証 (`Device_Config.xlsx`)
```bash
# A列=原文, B列=訳文, C列=補足説明, 1行目=ヘッダー除外で抽出
suffolk filter test_data/Device_Config.xlsx \
  --xlsx-source A \
  --xlsx-target B \
  --xlsx-context C \
  --xlsx-skip-rows 1

# 結果の確認: 61件のユニットが抽出され、note にシート名・セル番地・補足文が格納されます
# マージのテスト
suffolk merge test_data/Device_Config.xlsx.shwv.json --xlsx-source A --xlsx-target B
# -> test_data/Device_Config.out.xlsx が生成されます
```

### 2. 日本語ふりがな除外検証 (`Device_Flowchart.xlsx`)
```bash
suffolk filter test_data/Device_Flowchart.xlsx
# -> <rPh> のふりがなが重複せず、きれいなテキストのみが抽出されます
```

### 3. Word タグ最小化検証 (`デバイスガイド.docx`)
```bash
suffolk filter test_data/デバイスガイド.docx
# -> 17件の段落が抽出され、Word特有の寸断タグが解消されます
suffolk merge test_data/デバイスガイド.docx.shwv.json
# -> test_data/デバイスガイド.out.docx が生成されます
```

### 4. PowerPoint スピーカーノート検証 (`PCお困りごと共有＆解決勉強会.pptx`)
```bash
suffolk filter test_data/PCお困りごと共有＆解決勉強会.pptx
# -> スライド本文（[Slide N]）とスピーカーノート（[Slide N Speaker Notes]）が
#    正確に note に識別されて抽出されます
suffolk merge test_data/PCお困りごと共有＆解決勉強会.pptx.shwv.json
# -> test_data/PCお困りごと共有＆解決勉強会.out.pptx が生成されます
```

### 5. HTML コード・スタイル保護検証 (`ui.html`)
```bash
suffolk filter test_data/ui.html
# -> <script> や <style> が完全に保護され、UI文字列のみが抽出されます
```

### 6. Adobe InDesign 検証 (`2articles-1photo.idml`)
```bash
suffolk filter test_data/2articles-1photo.idml
# -> designmap.xml の StoryList に従い、見出し→第1記事→第2記事の論理順（5ユニット）で抽出されます
suffolk merge test_data/2articles-1photo.idml.shwv.json
# -> test_data/2articles-1photo.out.idml が生成されます
```

### 7. 文字コード自動判定検証 (`test_ansi.txt`)
```bash
suffolk filter test_data/test_ansi.txt
# -> Shift-JIS が自動検出され、文字化けなく UTF-8 の JSON に変換されます
```

### 8. フェイルセーフ救出機能の検証 (`2articles-1photo.idml`)
```bash
# 翻訳データから CSV と TXT（訳文のみ・タグ除去済み）を即座に生成
suffolk export test_data/2articles-1photo.idml.shwv.json
# -> test_data/2articles-1photo.idml.failsafe.csv (Excelで即座に閲覧可能)
# -> test_data/2articles-1photo.idml.failsafe.txt (InDesign手作業貼り付け用)

# 存在しないスケルトンを指定してマージを実行し、自動緊急リカバリを確認
suffolk merge test_data/2articles-1photo.idml.shwv.json -s missing_skeleton.idml
# -> [FAILSAFE] による緊急ファイル自動生成ログが表示されます
```

---

## 🔍 Okapi Tikal との比較検証

ルートディレクトリ直下の `okapi-apps_win32-x86_64_1.48.0/` に Okapi Framework のバイナリが配置されています。同じファイルに対して Tikal を実行し、抽出内容を比較検証できます。

```bash
# Okapi Tikal による抽出 (.xlf が生成されます)
./okapi-apps_win32-x86_64_1.48.0/tikal.bat -x test_data/デバイスガイド.docx -sl en -tl ja

# Suffolk による XLIFF 抽出
suffolk filter test_data/デバイスガイド.docx --to xliff
```
- **タグの簡潔さ**: Suffolk ではデフォルトでタグ最小化が効くため、Okapi の標準抽出に比べてインラインタグが格段にクリーンになります。
- **処理速度**: Java VM の起動オーバーヘッドがないため、ミリ秒単位で瞬時に完了します。

---

## 🌐 WebAssembly (WASM) のビルドと使い方

Suffolk は、ブラウザアプリケーション（Web / SPA / **SheepCombWeb**）および **VS Code 拡張機能**（**SheepWeave** 等）に組み込んで、ファイルシステムや Java 実行環境なしにインメモリで直接実行できる WASM パッケージ（`suffolk-wasm`）を提供しています。

すべての WASM 関数は、**各種システム引数を柔軟な JSON オブジェクト（`camelCase` / `snake_case` 両対応）として受け取る** ことができます。

### 1. WASM のビルド

前提として `wasm-pack` がインストールされている必要があります（`cargo install wasm-pack`）。

```bash
# プロジェクトルートから一括ビルド（Web用とNode.js用の両方を生成）
pnpm build:wasm
# または PowerShell スクリプト
./build-wasm.ps1

# 個別にビルドする場合
# ブラウザアプリ用 (--target web -> pkg/web)
wasm-pack build crates/suffolk-wasm --target web --out-dir ../../pkg/web

# VS Code 拡張機能 / Node.js 用 (--target nodejs -> pkg/node)
wasm-pack build crates/suffolk-wasm --target nodejs --out-dir ../../pkg/node
```

ビルド完了後、`pkg/web`（ブラウザ用 ESM）および `pkg/node`（Node.js / VS Code 拡張用 CommonJS）に型定義（`.d.ts`）付きでパッケージが生成されます。

### 2. 提供されるエントリポイント一覧

| エントリポイント名 | 対象環境 | 主な役割・用途 |
| :--- | :--- | :--- |
| `suffolk_browser_extract` | ブラウザ / Web | 単一原稿バイナリ (`Uint8Array`) から `ShWvData` (JSON) または XLIFF を抽出 |
| `suffolk_browser_extract_batch` | ブラウザ / Web | 複数ファイル（ドラッグ＆ドロップ）を一括抽出し、統合 `project.json` を生成 |
| `suffolk_browser_merge` | ブラウザ / Web | スケルトン (`Uint8Array`) + 翻訳データから完成バイナリ (`Uint8Array`) を生成 |
| `suffolk_browser_export_failsafe` | ブラウザ / Web | 救済用フェイルセーフ対照 CSV およびプレーン TXT を生成 |
| `suffolk_vscode_extract` | VS Code 拡張 | `vscode.workspace.fs.readFile` の `Uint8Array` を直接渡して抽出 |
| `suffolk_vscode_extract_batch` | VS Code 拡張 | ワークスペース内複数ファイルの一括抽出 |
| `suffolk_vscode_merge` | VS Code 拡張 | 完成バイナリを出力（`vscode.workspace.fs.writeFile` で直接保存可） |
| `suffolk_vscode_export_failsafe` | VS Code 拡張 | フェイルセーフ CSV / TXT の出力 |
| `suffolk_vscode_segment_text` | VS Code 拡張 | 選択中テキストを Suffolk の文分割ルールでリアルタイム分割 (`string[]`) |
| `suffolk_vscode_validate_tags` | VS Code 拡張 | 原文・訳文間のインライン書式タグ（`{@0}`等）の整合性をリアルタイム検証 |
| `suffolk_vscode_get_supported_formats` | VS Code 拡張 | 対応フォーマット・拡張子一覧を取得（ダイアログ等のフィルター用） |
| `extract`, `merge`, `export_failsafe` | 共通 | 汎用ショートハンド関数 |

### 3. システム引数（JSON オブジェクト）の仕様

```typescript
// 抽出（Filter / Extract）オプション
interface SuffolkFilterOptions {
  seg?: boolean;                 // 文単位セグメンテーション（デフォルト: false）
  srcLang?: string;              // 原文言語コード（デフォルト: "en"）
  trgLang?: string;              // 訳文言語コード（デフォルト: "ja"）
  rawTags?: boolean;             // タグ最小化を無効化（デフォルト: false）
  to?: "shwv" | "xliff";         // 出力形式（デフォルト: "shwv"）
  encoding?: string;             // 文字コード指定（省略時は自動判別）
  
  // Excel (XLSX) 多言語列フィルタ設定
  xlsxSource?: string | string[]; // 原文列（例: "A" または ["A"]）
  xlsxTarget?: string | string[]; // 訳文列（例: "B" または ["B"]）
  xlsxContext?: string | string[];// コンテキスト列（例: "C" または ["C"]）
  xlsxSkipRows?: number | string | number[]; // 除外行（例: 1 または [1]）
  xlsxSheet?: string;            // 対象シート名正規表現（例: ".*"）
  xlsxConfigCsv?: string;        // rules.csv の文字列内容をそのまま渡すことも可能
}

// 復元（Merge）オプション
interface SuffolkMergeOptions {
  encoding?: string;
  xlsxSource?: string | string[];
  xlsxTarget?: string | string[];
  xlsxConfigCsv?: string;
}
```

### 4. ブラウザアプリ（Vue 3 / Nuxt 3 / SPA）での組み込み例

```javascript
import init, {
  suffolk_browser_extract,
  suffolk_browser_merge,
  suffolk_browser_export_failsafe
} from './pkg/web/suffolk_wasm.js';

// 1. WASM の初期化
await init();

// 2. ユーザーがドラッグ＆ドロップしたファイルの読み込みと抽出
async function onFileDropped(file) {
  const arrayBuffer = await file.arrayBuffer();
  const fileBytes = new Uint8Array(arrayBuffer);

  const options = {
    seg: true,
    srcLang: 'en',
    trgLang: 'ja'
  };

  // ShWvData (JSON オブジェクト) が即座に返ります
  const projectData = suffolk_browser_extract(fileBytes, file.name, options);
  console.log('Extracted project:', projectData);
  return projectData;
}

// 3. 翻訳確定後のマージとダウンロード
function downloadMergedDocument(originalFileBytes, fileName, translationProject) {
  const mergedBytes = suffolk_browser_merge(
    originalFileBytes,
    fileName,
    translationProject,
    {}
  );

  // Blob を作成してブラウザから直接ダウンロード
  const blob = new Blob([mergedBytes], { type: 'application/octet-stream' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `translated_${fileName}`;
  a.click();
}
```

### 5. VS Code 拡張機能（SheepWeave 等）での組み込み例

```typescript
import * as vscode from 'vscode';
import * as wasm from './pkg/node/suffolk_wasm.js';

export function activate(context: vscode.ExtensionContext) {
  // コマンド: 原稿ファイルから翻訳プロジェクトを抽出
  const filterCmd = vscode.commands.registerCommand('sheepweave.extractDocument', async (uri: vscode.Uri) => {
    const fileBytes = await vscode.workspace.fs.readFile(uri);
    const fileName = vscode.workspace.asRelativePath(uri);

    const options = {
      seg: true,
      srcLang: 'ja',
      trgLang: 'en'
    };

    // WASM で抽出実行
    const projectData = wasm.suffolk_vscode_extract(fileBytes, fileName, options);

    // project.json としてワークスペースに保存
    const outUri = vscode.Uri.joinPath(vscode.workspace.workspaceFolders![0].uri, 'project.json');
    const jsonBytes = Buffer.from(JSON.stringify(projectData, null, 2), 'utf-8');
    await vscode.workspace.fs.writeFile(outUri, jsonBytes);

    vscode.window.showInformationMessage(`Suffolk: Extracted ${projectData.body.units.length} units!`);
  });

  // 便利機能: エディタで選択中のテキストを文分割
  const segmentCmd = vscode.commands.registerCommand('sheepweave.segmentSelection', () => {
    const editor = vscode.window.activeTextEditor;
    if (!editor) return;

    const selectionText = editor.document.getText(editor.selection);
    const sentences: string[] = wasm.suffolk_vscode_segment_text(selectionText);
    console.log('Segmented sentences:', sentences);
  });

  // 便利機能: 原文と訳文のタグ整合性チェック
  const validateTags = (source: string, target: string) => {
    const check = wasm.suffolk_vscode_validate_tags(source, target);
    if (!check.valid) {
      vscode.window.showWarningMessage(`Tag mismatch! Missing: ${check.missing.join(', ')}`);
    }
  };

  context.subscriptions.push(filterCmd, segmentCmd);
}
```

---

## 🏗️ クレート構成

本リポジトリは Cargo ワークスペースとして設計されており、関心事が綺麗に分離されています:

```
Suffolk/
├── crates/
│   ├── suffolk-ir/          # 中間データモデル (DocumentIR, UnitIR, Tag)
│   │   ├── src/model.rs     # セグメントおよびコンテキスト定義（詳細コメント付き）
│   │   └── src/placeholders.rs # {@0} プレースホルダーのパースおよび復元（{0}も後方互換サポート）
│   │
│   ├── suffolk-codec/       # データ交換形式の入出力
│   │   ├── src/shwv.rs      # SheepCombWeb (type.ts) の ShWvData シリアライザ
│   │   └── src/xliff.rs     # XLIFF 1.2 リーダー & ライター
│   │
│   ├── suffolk-formats/     # 各ファイル形式のフィルタ & マージ実装
│   │   ├── src/traits.rs    # DocumentFilter / DocumentMerger トレイト
│   │   ├── src/openxml/     # DOCX, XLSX, PPTX 実装
│   │   ├── src/idml/        # Adobe InDesign (IDML) フィルタ & マージ実装
│   │   ├── src/html/        # lol_html による高速ストリーミング HTML 処理
│   │   ├── src/markdown/    # フロントマター & コードブロック保護 Markdown 処理
│   │   └── src/text/        # encoding_rs による文字コード自動判定 PlainText 処理
│   │
│   ├── suffolk-wasm/        # Web & VS Code 向け WASM バインディング (pkg/web, pkg/node)
│   │   ├── src/browser.rs   # ブラウザ向けエントリポイント
│   │   ├── src/vscode.rs    # VS Code 拡張向けエントリポイント & 便利ツール
│   │   ├── src/options.rs   # 柔軟な JSON システム引数パーサー
│   │   └── src/core.rs      # メモリ上での抽出 & マージエンジン
│   │
│   └── suffolk-cli/         # CLI エントリポイント (suffolk.exe)
│
├── pkg/
│   ├── web/                 # ブラウザ向け WASM パッケージ (--target web)
│   └── node/                # VS Code 拡張向け WASM パッケージ (--target nodejs)
├── test_data/               # 各種検証用サンプルファイル
├── type.ts                  # SheepCombWeb 共通データスキーマ定義 (SSOT)
├── test_wasm.js             # WASM 結合検証テストスクリプト
└── README.md                # 本ドキュメント
```

---

## 🛣️ 今後のロードマップ

1. **SVG 画像フィルタ**: ベクターグラフィックス内テキストの抽出・マージ
2. **WebAssembly (WASM) パッケージング**: ✅ **実装完了**（ブラウザ用 `pkg/web` および VS Code 拡張用 `pkg/node` を提供）

---

## 📄 ライセンス

Copyright (c) 2026 SheepFamily. All rights reserved.
