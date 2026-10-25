use std::collections::HashSet;
use regex::Regex;
use wasm_bindgen::prelude::*;

use crate::core::{
    create_failsafe, extract_document, extract_multiple_documents, ir_to_xliff, merge_document,
    parse_js_translation_units, to_js_value, WasmFileInput,
};
use crate::options::{WasmFilterOptions, WasmMergeOptions};
use suffolk_ir::split_sentences;

/// [VS Code 拡張 エントリポイント]
/// 単一ドキュメント（Buffer / Uint8Array）から翻訳セグメントを抽出します。
/// `vscode.workspace.fs.readFile(uri)` で取得した Uint8Array をそのまま渡せます。
///
/// ### 引数
/// - `file_bytes`: 原稿ファイルのバイナリ (`Uint8Array` / `Buffer`)
/// - `file_name`: ファイル名（例: `"document.docx"`, `"config.xlsx"`）
/// - `options`: オプション設定 JSON オブジェクト
///
/// ### 戻り値
/// - デフォルト: `ShWvData` 形式の JavaScript オブジェクト
/// - `options.to == "xliff"` 指定時: `{ format: "xliff", content: string, fileName: string }`
#[wasm_bindgen(js_name = suffolk_vscode_extract)]
pub fn vscode_extract(
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

/// [VS Code 拡張 エントリポイント]
/// ワークスペース内の複数ファイルの一括抽出を行い、統合された `project.json` (ShWvData) を返します。
///
/// ### 引数
/// - `files`: `[{ name: string, data: Uint8Array }]` の配列
/// - `options`: オプション設定 JSON オブジェクト
#[wasm_bindgen(js_name = suffolk_vscode_extract_batch)]
pub fn vscode_extract_batch(
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

/// [VS Code 拡張 エントリポイント]
/// スケルトンファイルに翻訳結果を流し込み、完成バイナリを出力します。
/// `vscode.workspace.fs.writeFile(uri, resultBytes)` に直接書き込めます。
///
/// ### 引数
/// - `skeleton_bytes`: 元原稿ファイルのバイナリ (`Uint8Array` / `Buffer`)
/// - `file_name`: ファイル名（例: `"document.docx"`）
/// - `translation_data`: 翻訳データ（`ShWvData` オブジェクトまたは XLIFF 文字列）
/// - `options`: マージオプション設定 JSON オブジェクト
///
/// ### 戻り値
/// 完成バイナリ (`Uint8Array`)
#[wasm_bindgen(js_name = suffolk_vscode_merge)]
pub fn vscode_merge(
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

/// [VS Code 拡張 エントリポイント]
/// フェイルセーフ出力（対照 CSV および プレーン TXT）を生成します。
///
/// ### 戻り値
/// `{ csv: string, txt: string }`
#[wasm_bindgen(js_name = suffolk_vscode_export_failsafe)]
pub fn vscode_export_failsafe(
    translation_data: JsValue,
) -> Result<JsValue, JsValue> {
    let translations = parse_js_translation_units(&translation_data)
        .map_err(|e| JsValue::from_str(&e))?;

    let failsafe = create_failsafe(&translations)
        .map_err(|e| JsValue::from_str(&e))?;

    to_js_value(&failsafe)
}

/// [VS Code 拡張 便利機能]
/// 選択テキストや段落テキストを Suffolk の文分割ルールでセグメント分割します。
/// エディタ上でリアルタイムに文境界をプレビュー・ハイライト表示する際に便利です。
///
/// ### 引数
/// - `text`: 分割対象テキスト
///
/// ### 戻り値
/// `string[]` (分割された文の配列)
#[wasm_bindgen(js_name = suffolk_vscode_segment_text)]
pub fn vscode_segment_text(text: &str) -> Result<JsValue, JsValue> {
    let spans = split_sentences(text);
    let sentences: Vec<String> = spans.into_iter().map(|s| s.text).collect();
    to_js_value(&sentences)
}

/// [VS Code 拡張 便利機能]
/// 原文と訳文のインライン装飾タグ（`{@0}`, `{@1}`, `{0}` 等）の整合性を検証します。
/// ポストエディット作業中のタグ欠損や余計なタグ挿入を警告する際に活用できます。
///
/// ### 戻り値
/// `{ valid: boolean, missing: string[], extra: string[] }`
#[wasm_bindgen(js_name = suffolk_vscode_validate_tags)]
pub fn vscode_validate_tags(source: &str, target: &str) -> Result<JsValue, JsValue> {
    let re = Regex::new(r"\{@?(\d+)\}").expect("Invalid regex");

    let mut src_tags = HashSet::new();
    for cap in re.captures_iter(source) {
        if let Some(m) = cap.get(1) {
            src_tags.insert(m.as_str().to_string());
        }
    }

    let mut tgt_tags = HashSet::new();
    for cap in re.captures_iter(target) {
        if let Some(m) = cap.get(1) {
            tgt_tags.insert(m.as_str().to_string());
        }
    }

    let missing: Vec<String> = src_tags
        .difference(&tgt_tags)
        .map(|s| format!("{{@{}}}", s))
        .collect();

    let extra: Vec<String> = tgt_tags
        .difference(&src_tags)
        .map(|s| format!("{{@{}}}", s))
        .collect();

    let valid = missing.is_empty() && extra.is_empty();

    let res = serde_json::json!({
        "valid": valid,
        "missing": missing,
        "extra": extra,
    });

    to_js_value(&res)
}

/// [VS Code 拡張 便利機能]
/// Suffolk が対応している全フォーマットと拡張子の一覧を返します。
/// VS Code のファイルピッカー（`showOpenDialog`）のフィルター定義等に利用できます。
#[wasm_bindgen(js_name = suffolk_vscode_get_supported_formats)]
pub fn vscode_get_supported_formats() -> JsValue {
    let formats = serde_json::json!([
        { "name": "Microsoft Word", "extensions": ["docx"] },
        { "name": "Microsoft Excel", "extensions": ["xlsx"] },
        { "name": "Microsoft PowerPoint", "extensions": ["pptx"] },
        { "name": "HTML Document", "extensions": ["html", "htm"] },
        { "name": "Markdown Document", "extensions": ["md", "markdown"] },
        { "name": "Plain Text", "extensions": ["txt", "text"] },
        { "name": "Adobe InDesign Markup", "extensions": ["idml"] }
    ]);
    to_js_value(&formats).unwrap()
}
