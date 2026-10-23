================================================================================
 suffolk-cli (Suffolk Command Line Interface)
================================================================================

【このクレートの役割】
Suffolk をコマンドライン（端末）から実行するための「実行可能バイナリ（CLI本体）」です。
ユーザーの入力を受け取り、ライブラリクレート（formats, codec, ir）を呼び出して
実際のファイル変換パイプラインを制御・実行します。

【なぜ独立しているのか】
Rust では、コアロジックをライブラリクレートとして作成し、CLI 部分を薄いラッパーとして
独立させることが標準的なベストプラクティスです。
こうすることで、将来 WebAssembly (WASM) 化してブラウザ上で動かしたり、
デスクトップ GUI アプリや Web API サーバーを開発する際にも、
CLI を変更せずに同じコアロジックをそのまま再利用できます。

【配置されているロジック・ファイル構成】
- src/
  └─ main.rs
       CLI アプリケーションのエントリーポイントです。以下の責務を持ちます：
       1. コマンドライン引数の定義と解析（clap クレートを使用）
          - `filter` サブコマンド: 原稿からテキストを抽出して ShWvData (JSON) や XLIFF を出力
          - `merge` サブコマンド: 翻訳結果（JSONやXLIFF）を元ファイルに組み戻して完成文書を出力
       2. オプション解析
          - 文分割（--seg / -g）
          - 出力形式（--to shwv / xliff）
          - 言語指定（-s en -t ja）
          - タグ最小化の制御（--raw-tags）
          - Excel 多言語列設定ファイル（--xlsx-config）
          - フェイルセーフ出力（--failsafe txt / csv）
       3. 入力ファイル判別とパイプライン実行
          - 拡張子に応じた適切なフィルター・マージャー（suffolk-formats）の選択
          - コーデック（suffolk-codec）を用いたシリアライズ・ファイル保存
          - ユーザーへの進捗やエラーの分かりやすいコンソール出力

【ビルドと実行方法】
・開発時ビルド: cargo build
・リリース用ビルド: cargo build --release  （target/release/suffolk.exe が生成されます）
・実行例:
    cargo run -- filter document.docx -s en -t ja --seg
    cargo run -- merge document.docx -t translated.json -o document_ja.docx

【依存関係】
・参照元（依存しているクレート）: `suffolk-ir`, `suffolk-codec`, `suffolk-formats`
・外部ライブラリ: clap（CLI引数解析）, anyhow（エラー処理）, serde_json
================================================================================
