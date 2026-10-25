use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use suffolk_codec::{
    export_failsafe_csv, export_failsafe_txt, parse_xliff_12, write_xliff_12, ShWvData,
    TranslatedUnit,
};
use suffolk_formats::{
    DocxFilter, DocxMerger, DocumentFilter, DocumentMerger, HtmlFilter, HtmlMerger, IdmlFilter,
    IdmlMerger, MarkdownFilter, MarkdownMerger, PptxFilter, PptxMerger, TextFilter, TextMerger,
    XlsxFilter, XlsxMerger,
};
use suffolk_ir::{segment_document_ir, DocumentFormat, DocumentIR};

use crate::options::{WasmFilterOptions, WasmMergeOptions};

/// Rust 構造体をプレーンな JavaScript オブジェクト（POJO）にシリアライズします。
pub fn to_js_value<T: Serialize>(val: &T) -> Result<JsValue, JsValue> {
    let json_str = serde_json::to_string(val)
        .map_err(|e| JsValue::from_str(&format!("JSON serialization error: {}", e)))?;
    js_sys::JSON::parse(&json_str)
        .map_err(|e| JsValue::from_str(&format!("JSON parse error: {:?}", e)))
}

/// 複数ファイル一括処理時のファイル入力データ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmFileInput {
    /// ファイル名（例: "report.docx"）
    pub name: String,
    /// ファイルのバイナリデータ
    pub data: Vec<u8>,
}

/// フェイルセーフ出力結果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmFailsafeOutput {
    /// 対照 CSV (UTF-8 BOM 付き)
    pub csv: String,
    /// プレーン TXT (訳文のみ、タグ除去済み)
    pub txt: String,
}

/// 抽出（Filter）の戻り値
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WasmFilterOutput {
    /// ShWvData JSON データ
    ShWv(ShWvData),
    /// XLIFF 1.2 XML 文字列
    Xliff(String),
}

/// 単一ファイルからのテキスト抽出（コアロジック）
pub fn extract_document(
    file_bytes: &[u8],
    file_name: &str,
    opts: &WasmFilterOptions,
) -> Result<DocumentIR, String> {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("File name '{}' has no extension", file_name))?;

    let format = DocumentFormat::from_extension(ext)
        .ok_or_else(|| format!("Unsupported file extension: .{}", ext))?;

    let filter_opts = opts.to_filter_options()?;
    let reader = Cursor::new(file_bytes);

    let raw_ir = match format {
        DocumentFormat::Docx => DocxFilter
            .extract(reader, file_name, &filter_opts)
            .map_err(|e| format!("Docx extraction error: {}", e))?,
        DocumentFormat::Xlsx => XlsxFilter
            .extract(reader, file_name, &filter_opts)
            .map_err(|e| format!("Xlsx extraction error: {}", e))?,
        DocumentFormat::Pptx => PptxFilter
            .extract(reader, file_name, &filter_opts)
            .map_err(|e| format!("Pptx extraction error: {}", e))?,
        DocumentFormat::PlainText => TextFilter
            .extract(reader, file_name, &filter_opts)
            .map_err(|e| format!("Text extraction error: {}", e))?,
        DocumentFormat::Markdown => MarkdownFilter
            .extract(reader, file_name, &filter_opts)
            .map_err(|e| format!("Markdown extraction error: {}", e))?,
        DocumentFormat::Html => HtmlFilter
            .extract(reader, file_name, &filter_opts)
            .map_err(|e| format!("Html extraction error: {}", e))?,
        DocumentFormat::Idml => IdmlFilter
            .extract(reader, file_name, &filter_opts)
            .map_err(|e| format!("Idml extraction error: {}", e))?,
        DocumentFormat::Svg => {
            return Err("Svg format is planned for future extension".to_string());
        }
    };

    let processed_ir = if opts.seg {
        segment_document_ir(&raw_ir)
    } else {
        raw_ir
    };

    Ok(processed_ir)
}

/// 複数ファイルの一括抽出（コアロジック）
pub fn extract_multiple_documents(
    files: &[WasmFileInput],
    opts: &WasmFilterOptions,
) -> Result<ShWvData, String> {
    if files.is_empty() {
        return Err("No input files provided".to_string());
    }

    let mut irs = Vec::with_capacity(files.len());
    for file in files {
        let ir = extract_document(&file.data, &file.name, opts)?;
        irs.push(ir);
    }

    let mut shwv_data = ShWvData::from_multiple_irs(&irs, opts.seg);
    shwv_data.meta.bilingual_path = "project.json".to_string();

    Ok(shwv_data)
}

/// 単一ドキュメントへの翻訳差し込み（コアロジック）
pub fn merge_document(
    skeleton_bytes: &[u8],
    file_name: &str,
    translations: &HashMap<usize, TranslatedUnit>,
    opts: &WasmMergeOptions,
) -> Result<Vec<u8>, String> {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("Skeleton file name '{}' has no extension", file_name))?;

    let format = DocumentFormat::from_extension(ext)
        .ok_or_else(|| format!("Unsupported skeleton file extension: .{}", ext))?;

    let merge_opts = opts.to_merge_options()?;
    let skeleton_reader = Cursor::new(skeleton_bytes);
    let mut out_writer = Cursor::new(Vec::new());

    match format {
        DocumentFormat::Docx => DocxMerger
            .merge(skeleton_reader, translations, &mut out_writer, &merge_opts)
            .map_err(|e| format!("Docx merge error: {}", e))?,
        DocumentFormat::Xlsx => XlsxMerger
            .merge(skeleton_reader, translations, &mut out_writer, &merge_opts)
            .map_err(|e| format!("Xlsx merge error: {}", e))?,
        DocumentFormat::Pptx => PptxMerger
            .merge(skeleton_reader, translations, &mut out_writer, &merge_opts)
            .map_err(|e| format!("Pptx merge error: {}", e))?,
        DocumentFormat::PlainText => TextMerger
            .merge(skeleton_reader, translations, &mut out_writer, &merge_opts)
            .map_err(|e| format!("Text merge error: {}", e))?,
        DocumentFormat::Markdown => MarkdownMerger
            .merge(skeleton_reader, translations, &mut out_writer, &merge_opts)
            .map_err(|e| format!("Markdown merge error: {}", e))?,
        DocumentFormat::Html => HtmlMerger
            .merge(skeleton_reader, translations, &mut out_writer, &merge_opts)
            .map_err(|e| format!("Html merge error: {}", e))?,
        DocumentFormat::Idml => IdmlMerger
            .merge(skeleton_reader, translations, &mut out_writer, &merge_opts)
            .map_err(|e| format!("Idml merge error: {}", e))?,
        DocumentFormat::Svg => {
            return Err("Svg format is planned for future extension".to_string());
        }
    };

    Ok(out_writer.into_inner())
}

/// JS から渡された翻訳データ（`JsValue`）を解析し、`HashMap<usize, TranslatedUnit>` に変換します。
pub fn parse_js_translation_units(js_val: &JsValue) -> Result<HashMap<usize, TranslatedUnit>, String> {
    // 1. 文字列型として渡された場合
    if let Some(text) = js_val.as_string() {
        let trimmed = text.trim();
        if trimmed.starts_with('<') {
            // XLIFF 1.2
            return parse_xliff_12(Cursor::new(trimmed.as_bytes()))
                .map_err(|e| format!("Failed to parse XLIFF: {}", e));
        } else {
            // JSON 文字列
            let parsed: serde_json::Value = serde_json::from_str(trimmed)
                .map_err(|e| format!("Failed to parse JSON string: {}", e))?;
            return parse_json_value_translation_units(&parsed);
        }
    }

    // 2. JavaScript オブジェクトの場合、JSON.stringify を介して一度標準 JSON 文字列に落とす
    //    これによって Map / POJO の違いや数値型の違いによる serde-wasm-bindgen の誤動作を完全に防止する
    if let Ok(json_str_js) = js_sys::JSON::stringify(js_val) {
        if let Some(json_str) = json_str_js.as_string() {
            let parsed: serde_json::Value = serde_json::from_str(&json_str)
                .map_err(|e| format!("Failed to parse stringified JSON: {}", e))?;
            return parse_json_value_translation_units(&parsed);
        }
    }

    Err("Invalid translation data: could not convert JS object to JSON".to_string())
}

/// serde_json::Value から翻訳ユニットマップを取得
fn parse_json_value_translation_units(raw_value: &serde_json::Value) -> Result<HashMap<usize, TranslatedUnit>, String> {
    // 1. ShWvData 形式のオブジェクトの場合（meta と body を持つ）
    if raw_value.get("body").is_some() && raw_value.get("meta").is_some() {
        let shwv: ShWvData = serde_json::from_value(raw_value.clone())
            .map_err(|e| format!("Failed to deserialize ShWvData: {}", e))?;
        return Ok(shwv.extract_recombined_translation_map(None));
    }

    // 2. 既に TranslatedUnit の連想配列（"1": { ... }）である場合
    if let Ok(map) = serde_json::from_value::<HashMap<usize, TranslatedUnit>>(raw_value.clone()) {
        return Ok(map);
    }

    // 3. ShWvUnit の配列（units: [...] または直接配列）である場合
    let units_slice = if let Some(arr) = raw_value.get("units").and_then(|u| u.as_array()) {
        Some(arr)
    } else {
        raw_value.as_array()
    };

    if let Some(units_val) = units_slice {
        if let Ok(shwv) = serde_json::from_value::<ShWvData>(serde_json::json!({
            "meta": {
                "ver": "1.0",
                "bilingual_path": "temp.json",
                "files": []
            },
            "body": {
                "units": units_val
            }
        })) {
            return Ok(shwv.extract_recombined_translation_map(None));
        }
    }

    Err("Unrecognized translation data format. Expected ShWvData object, XLIFF XML string, or translation map.".to_string())
}

/// フェイルセーフ対照 CSV と TXT を生成
pub fn create_failsafe(translations: &HashMap<usize, TranslatedUnit>) -> Result<WasmFailsafeOutput, String> {
    let csv = export_failsafe_csv(translations, true)
        .map_err(|e| format!("Failed to export failsafe CSV: {}", e))?;
    let txt = export_failsafe_txt(translations, false);
    Ok(WasmFailsafeOutput { csv, txt })
}

/// 単一ドキュメントを XLIFF 形式の文字列へ変換
pub fn ir_to_xliff(ir: &DocumentIR) -> Result<String, String> {
    write_xliff_12(ir).map_err(|e| format!("Failed to write XLIFF: {}", e))
}
