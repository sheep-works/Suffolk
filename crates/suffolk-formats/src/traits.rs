use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use suffolk_codec::TranslatedUnit;
use suffolk_ir::DocumentIR;
use thiserror::Error;

/// 各ファイルフォーマットの処理時に発生するエラー
#[derive(Debug, Error)]
pub enum FormatError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Encoding error: {0}")]
    Encoding(String),
    #[error("Format error: {0}")]
    Parse(String),
    #[error("Zip archive error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),
}

/// 抽出（Filter）実行時のパラメータ設定
#[derive(Debug, Clone, Default)]
pub struct FilterOptions {
    /// 原文の言語コード（例: "en", "ja"）
    pub source_lang: String,
    /// 訳文の言語コード（例: "ja", "zh-CN"）
    pub target_lang: Option<String>,
    /// 文字エンコーディング指定（未指定時は自動判定）
    pub encoding: Option<String>,
    /// インライン書式タグの最小化を行うか（デフォルト: true）
    pub minimize_tags: bool,
    // --- Excel (XLSX) 多言語列フィルタ設定 ---
    /// 原文列（例: `["A"]` や `["A", "D"]`）
    pub xlsx_source_cols: Vec<String>,
    /// 訳文列（例: `["B"]` や `["B", "E"]`）
    pub xlsx_target_cols: Vec<String>,
    /// コンテキスト情報列（例: `["C"]`）
    pub xlsx_context_cols: Vec<String>,
    /// ヘッダー行などの除外行番号（1始まり、例: `[1]`）
    pub xlsx_skip_rows: Vec<u32>,
    /// 対象とするシート名の正規表現パターン（例: `".*"`）
    pub xlsx_sheet_pattern: Option<String>,
}

/// 復元（Merge）実行時のパラメータ設定
#[derive(Debug, Clone, Default)]
pub struct MergeOptions {
    /// 文字エンコーディング指定
    pub encoding: Option<String>,
    /// 原文列指定（Excel用）
    pub xlsx_source_cols: Vec<String>,
    /// 訳文書き込み対象列（Excel用）
    pub xlsx_target_cols: Vec<String>,
}

/// 各文書フォーマットからテキストおよびメタ情報を抽出する共通インターフェース
pub trait DocumentFilter {
    /// 原稿ストリームからテキストを抽出し、中間表現 `DocumentIR` を生成します。
    fn extract<R: Read + Seek>(
        &self,
        reader: R,
        filename: &str,
        options: &FilterOptions,
    ) -> Result<DocumentIR, FormatError>;
}

/// 原稿スケルトンファイルと翻訳済みデータから翻訳後文書を再構築する共通インターフェース
pub trait DocumentMerger {
    /// スケルトンファイルに翻訳結果を流し込み、完成文書を出力ストリームに書き出します。
    fn merge<R: Read + Seek, W: Write + Seek>(
        &self,
        skeleton: R,
        translations: &HashMap<usize, TranslatedUnit>,
        writer: W,
        options: &MergeOptions,
    ) -> Result<(), FormatError>;
}
