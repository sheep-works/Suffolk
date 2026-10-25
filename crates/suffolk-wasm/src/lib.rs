pub mod browser;
pub mod core;
pub mod options;
pub mod vscode;

use wasm_bindgen::prelude::*;

pub use browser::*;
pub use core::*;
pub use options::*;
pub use vscode::*;

/// Suffolk WASM コアエンジンのバージョン
#[wasm_bindgen]
pub fn suffolk_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ----------------------------------------------------------------------------
// 共通ショートハンド関数（Browser / VS Code 共通の汎用名）
// ----------------------------------------------------------------------------

/// [汎用ショートハンド] 単一原稿ファイルからの抽出
#[wasm_bindgen]
pub fn extract(
    file_bytes: &[u8],
    file_name: &str,
    options: JsValue,
) -> Result<JsValue, JsValue> {
    browser_extract(file_bytes, file_name, options)
}

/// [汎用ショートハンド] 複数原稿ファイルの一括抽出
#[wasm_bindgen]
pub fn extract_batch(
    files: JsValue,
    options: JsValue,
) -> Result<JsValue, JsValue> {
    browser_extract_batch(files, options)
}

/// [汎用ショートハンド] 翻訳データからの完成文書マージ
#[wasm_bindgen]
pub fn merge(
    skeleton_bytes: &[u8],
    file_name: &str,
    translation_data: JsValue,
    options: JsValue,
) -> Result<js_sys::Uint8Array, JsValue> {
    browser_merge(skeleton_bytes, file_name, translation_data, options)
}

/// [汎用ショートハンド] フェイルセーフ CSV / TXT の生成
#[wasm_bindgen]
pub fn export_failsafe(
    translation_data: JsValue,
) -> Result<JsValue, JsValue> {
    browser_export_failsafe(translation_data)
}
