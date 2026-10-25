use wasm_bindgen::prelude::*;

use crate::core::{
    create_failsafe, extract_document, extract_multiple_documents, ir_to_xliff, merge_document,
    parse_js_translation_units, to_js_value, WasmFileInput,
};
use crate::options::{WasmFilterOptions, WasmMergeOptions};

/// [Browser エントリポイント]
/// 単一原稿ファイルのバイナリ（Uint8Array）からテキストを抽出します。
///
/// ### 引数
/// - `file_bytes`: 原稿ファイルのバイナリ (`Uint8Array`)
/// - `file_name`: ファイル名（拡張子付き、例: `"guide.docx"`, `"config.xlsx"`）
/// - `options`: オプション設定 JSON オブジェクト (省略時はデフォルト)
///
/// ### 戻り値
/// - デフォルト: `ShWvData` 形式の JavaScript オブジェクト
/// - `options.to == "xliff"` 指定時: `{ format: "xliff", content: string, fileName: string }`
#[wasm_bindgen(js_name = suffolk_browser_extract)]
pub fn browser_extract(
    file_bytes: &[u8],
    file_name: &str,
    options: JsValue,
) -> Result<JsValue, JsValue> {
    let opts: WasmFilterOptions = if options.is_undefined() || options.is_null() {
        WasmFilterOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options)
            .map_err(|e| JsValue::from_str(&format!("Invalid options JSON: {}", e)))?
    };

    let ir = extract_document(file_bytes, file_name, &opts)
        .map_err(|e| JsValue::from_str(&e))?;

    let is_xliff = opts
        .to
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("xliff") || s.eq_ignore_ascii_case("xlf"))
        .unwrap_or(false);

    if is_xliff {
        let xliff_xml = ir_to_xliff(&ir).map_err(|e| JsValue::from_str(&e))?;
        let res = serde_json::json!({
            "format": "xliff",
            "fileName": format!("{}.xlf", file_name),
            "content": xliff_xml,
        });
        to_js_value(&res)
    } else {
        let shwv_data = suffolk_codec::ShWvData::from_multiple_irs(&[ir], opts.seg);
        to_js_value(&shwv_data)
    }
}

/// [Browser エントリポイント]
/// 複数原稿ファイルの一括抽出を行い、統合された `project.json` (ShWvData) を返します。
/// ブラウザ上のドラッグ＆ドロップで複数ファイルが渡された場合に最適です。
///
/// ### 引数
/// - `files`: `[{ name: "file1.docx", data: Uint8Array }, ...]` 形式の配列
/// - `options`: オプション設定 JSON オブジェクト
#[wasm_bindgen(js_name = suffolk_browser_extract_batch)]
pub fn browser_extract_batch(
    files: JsValue,
    options: JsValue,
) -> Result<JsValue, JsValue> {
    let file_inputs: Vec<WasmFileInput> = serde_wasm_bindgen::from_value(files)
        .map_err(|e| JsValue::from_str(&format!("Invalid files input array: {}", e)))?;

    let opts: WasmFilterOptions = if options.is_undefined() || options.is_null() {
        WasmFilterOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options)
            .map_err(|e| JsValue::from_str(&format!("Invalid options JSON: {}", e)))?
    };

    let shwv_data = extract_multiple_documents(&file_inputs, &opts)
        .map_err(|e| JsValue::from_str(&e))?;

    to_js_value(&shwv_data)
}

/// [Browser エントリポイント]
/// 原稿スケルトンファイルに確定した翻訳データを流し込み、完成バイナリを出力します。
///
/// ### 引数
/// - `skeleton_bytes`: 元原稿ファイルのバイナリ (`Uint8Array`)
/// - `file_name`: ファイル名（例: `"guide.docx"`）
/// - `translation_data`: 翻訳データ（`ShWvData` オブジェクトまたは XLIFF 文字列）
/// - `options`: マージオプション設定 JSON オブジェクト
///
/// ### 戻り値
/// - 完成文書のバイナリデータ (`Uint8Array`)。そのまま `Blob` 化してダウンロード可能です。
#[wasm_bindgen(js_name = suffolk_browser_merge)]
pub fn browser_merge(
    skeleton_bytes: &[u8],
    file_name: &str,
    translation_data: JsValue,
    options: JsValue,
) -> Result<js_sys::Uint8Array, JsValue> {
    let opts: WasmMergeOptions = if options.is_undefined() || options.is_null() {
        WasmMergeOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options)
            .map_err(|e| JsValue::from_str(&format!("Invalid merge options JSON: {}", e)))?
    };

    let translations = parse_js_translation_units(&translation_data)
        .map_err(|e| JsValue::from_str(&e))?;

    let out_bytes = merge_document(skeleton_bytes, file_name, &translations, &opts)
        .map_err(|e| JsValue::from_str(&e))?;

    let uint8_arr = js_sys::Uint8Array::new_with_length(out_bytes.len() as u32);
    uint8_arr.copy_from(&out_bytes);
    Ok(uint8_arr)
}

/// [Browser エントリポイント]
/// マージが困難な場合のフェイルセーフ出力（対照 CSV および プレーン TXT）を生成します。
///
/// ### 戻り値
/// `{ csv: string, txt: string }`
#[wasm_bindgen(js_name = suffolk_browser_export_failsafe)]
pub fn browser_export_failsafe(
    translation_data: JsValue,
) -> Result<JsValue, JsValue> {
    let translations = parse_js_translation_units(&translation_data)
        .map_err(|e| JsValue::from_str(&e))?;

    let failsafe = create_failsafe(&translations)
        .map_err(|e| JsValue::from_str(&e))?;

    to_js_value(&failsafe)
}
