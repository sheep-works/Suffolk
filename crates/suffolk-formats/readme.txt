================================================================================
 suffolk-formats (Suffolk Native Document Formats Parser & Merger)
================================================================================

【このクレートの役割】
Word, Excel, PowerPoint, HTML, Markdown, プレーンテキストなどの
各種原稿ファイルから翻訳対象テキストを抽出（Filter / Extract）し、
翻訳完了後に元のレイアウトを保ったまま復元・組み戻し（Merge）を行うクレートです。

【なぜ独立しているのか】
ファイル形式ごとの処理（ZIPアーカイブ展開、XMLノードの走査・置換、HTMLパース、
文字エンコーディング判別など）はコード量が大きく、フォーマット固有のノウハウが必要です。
これらを共通トレイト（インターフェース）の元に独立モジュールとしてまとめることで、
新しいフォーマット（例: SVG, InDesign 等）を追加する際もプラグイン感覚で拡張できます。

【配置されているロジック・ファイル構成】
- src/
  ├─ lib.rs
  │    各フォーマットモジュールの公開。
  │
  ├─ traits.rs
  │    フォーマット処理の基盤となる共通インターフェース（トレイト）の定義。
  │    ・`DocumentFilter`: ファイルから DocumentIR を抽出する共通規約
  │    ・`DocumentMerger`: 翻訳結果を元ファイルに組み戻す共通規約
  │    ・`FilterOptions` / `MergeOptions`: 言語設定、タグ最小化、Excel設定などのパラメータ
  │
  ├─ openxml/ (Microsoft Office フォーマット)
  │    DOCX, XLSX, PPTX の ZIP 構造の展開・XML解析・再圧縮処理。
  │    ・docx.rs: Word文書の段落、表、ラン（Run）の抽出とタグ最小化
  │    ・xlsx.rs: Excelのシート走査、共有文字列（SharedStrings）、多言語列フィルター設定
  │    ・pptx.rs: PowerPointのスライド番号、図形内テキストの抽出
  │
  ├─ html/
  │    HTML文書のタグ構造を壊さずにテキストを抽出し、翻訳文を差し替えるロジック。
  │
  ├─ markdown/
  │    Markdown (.md) の構文木（AST）を解析し、見出し、リスト、段落などを処理。
  │
  ├─ text/
  │    プレーンテキスト (.txt) の処理。UTF-8 だけでなく Shift_JIS や EUC-JP 等の
  │    エンコーディング自動判別・変換（encoding_rs）も含みます。
  │
  └─ idml/
       Adobe InDesign (.idml) パッケージのストーリー XML 展開およびテキスト抽出・復元。

【依存関係】
・参照元（依存しているクレート）: `suffolk-ir`, `suffolk-codec`
・外部ライブラリ: zip, quick-xml, lol_html, pulldown-cmark, encoding_rs, csv 等
================================================================================
