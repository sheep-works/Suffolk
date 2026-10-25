================================================================================
 suffolk-wasm - WebAssembly バインディング（Browser & VS Code Extension）
================================================================================

【概要】
Suffolk の抽出（Filter/Extract）、復元（Merge）、文セグメンテーション（Segmenter）、
タグ整合性検証、フェイルセーフ出力機能を、ブラウザアプリケーション（Web / SPA / SheepCombWeb）
および VS Code 拡張機能（Node.js / SheepWeave）から直接利用できるようにした WASM クレートです。

すべての引数はプレーンな JSON オブジェクト（JsValue）として受け取ることができ、
camelCase および snake_case の双方を透過的に解釈します。

【ファイル構成】
- src/options.rs : JSON オプション設定構造体 (WasmFilterOptions, WasmMergeOptions)
- src/core.rs    : メモリ上での抽出・マージ・フェイルセーフ処理を行うコアロジック
- src/browser.rs : ブラウザ向けエントリポイント (suffolk_browser_*)
- src/vscode.rs  : VS Code 拡張機能向けエントリポイント (suffolk_vscode_*)
- src/lib.rs     : 再エクスポートおよび汎用ショートハンド関数 (extract, merge, ...)

【ビルド方法】
- ブラウザ向け (ESM + Fetch):
    wasm-pack build crates/suffolk-wasm --target web --out-dir ../../pkg/web

- VS Code 拡張機能向け (CommonJS + Node.js):
    wasm-pack build crates/suffolk-wasm --target nodejs --out-dir ../../pkg/node

- プロジェクトルートから一括実行:
    pnpm build:wasm または powershell ./build-wasm.ps1
================================================================================
