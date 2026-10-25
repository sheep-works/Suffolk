use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use suffolk_ir::{tags_to_string_map, DocumentIR};
use thiserror::Error;

/// ShWvData の処理に関するエラー定義
#[derive(Debug, Error)]
pub enum ShWvError {
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid ShWv format: {0}")]
    InvalidFormat(String),
}

/// SheepCombWeb データ仕様定義（ヘッダー識別子）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShWvDefine {
    /// データ型識別子（デフォルト: "SHWV_DATA"）
    pub name: String,
    /// フォーマットバージョン（デフォルト: "1.5"）
    pub version: String,
}

impl Default for ShWvDefine {
    fn default() -> Self {
        Self {
            name: "SHWV_DATA".to_string(),
            version: "1.5".to_string(),
        }
    }
}

/// 単一ファイルまたは結合ファイル内の範囲情報
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShWvFileInfo {
    /// 元のファイル名（例: "sample.docx"）
    pub name: String,
    /// 開始ユニットインデックス（1始まり）
    pub start: usize,
    /// 終了ユニットインデックス
    pub end: usize,
}

/// ワークフロー工程およびセグメンテーション情報
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShWvWorkflow {
    /// ワークフローのインデックス
    pub index: usize,
    /// 役割（例: "Extract", "Translate", "Review"）
    pub role: String,
    /// ツールまたは担当者名（例: "Suffolk"）
    pub name: String,
    /// セグメンテーション方式（例: "line", "sentence"）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segmentation: Option<String>,
}

impl Default for ShWvWorkflow {
    fn default() -> Self {
        Self {
            index: 0,
            role: "Extract".to_string(),
            name: "Suffolk".to_string(),
            segmentation: Some("line".to_string()),
        }
    }
}

/// ShWvData のメタデータ定義
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShWvMeta {
    /// バイリンガルファイルの保存先相対パス
    #[serde(rename = "bilingualPath")]
    pub bilingual_path: String,
    /// 対象ファイル情報一覧
    pub files: Vec<ShWvFileInfo>,
    /// 原文の言語コード（例: "en", "ja"）
    #[serde(rename = "sourceLang")]
    pub source_lang: String,
    /// 訳文の言語コード（例: "ja", "zh-CN"）
    #[serde(rename = "targetLang")]
    pub target_lang: String,
    /// プロジェクト名（任意）
    #[serde(rename = "projectName", skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    /// ワークフロー情報
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow: Option<ShWvWorkflow>,
}

/// 翻訳メモリ (TM) や用語集 (TB) の参照情報
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ShWvRef {
    /// 翻訳メモリのマッチ候補
    pub tms: Vec<serde_json::Value>,
    /// 用語集の一致情報
    pub tb: Vec<serde_json::Value>,
    /// 引用・クォート一致の範囲
    pub quoted: Vec<(usize, usize)>,
    /// 100%一致のインデックスリスト
    pub quoted100: Vec<usize>,
}

/// 翻訳の最小単位（セグメント / ユニット）
///
/// TypeScript の `ShWvUnit` インターフェース（`type.ts`）に完全準拠します。
///
/// 【メタ情報とメモの分離について】
/// - `note`: 原稿由来の情報（シート名、セル番地、スライド番号、スピーカーノート、コンテキスト列の内容など）を格納します。
/// - `memo`: 翻訳作業中に翻訳者が気づいたことや疑問点を残す領域であり、抽出時は常に None（null）を維持します。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShWvUnit {
    /// ユニット通番（1始まり、SheepCombWeb 上のユニークキー）
    pub idx: usize,
    /// 原文テキスト（インラインタグ最小化時は `{0}`, `{1}` などのプレースホルダーを含む）
    pub src: String,
    /// 事前翻訳テキスト（機械翻訳結果や事前充填された訳文）
    pub pre: String,
    /// 確定訳文テキスト
    pub tgt: String,
    /// 原稿由来のコンテキスト情報（シート名、スライド番号、セル番地等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// 翻訳者が作業中に残す個人用メモ（抽出初期はクリーンな None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
    /// サブセグメント（改行等で細分化されたセグメント）フラグ
    #[serde(rename = "isSub", skip_serializing_if = "Option::is_none")]
    pub is_sub: Option<bool>,
    /// セグメントの翻訳ステータス（0: 未翻訳, 1: 下書き, 2: 完了 等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,
    /// 参照データ（TM/TB）- JSONシリアライズ時は `"ref"` キーで出力
    pub ref_data: ShWvRef,
    /// インライン書式タグ等のプレースホルダー辞書（例: `{ "0": "<b>", "1": "</b>" }`）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholders: Option<HashMap<usize, String>>,
}

/// Rust の予約語 `ref` を JSON フィールド名 `"ref"` としてシリアライズするためのラッパー
impl Serialize for ShWvUnitWrapper<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct Helper<'a> {
            idx: usize,
            src: &'a str,
            pre: &'a str,
            tgt: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            note: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            memo: Option<&'a str>,
            #[serde(rename = "isSub", skip_serializing_if = "Option::is_none")]
            is_sub: Option<bool>,
            #[serde(skip_serializing_if = "Option::is_none")]
            status: Option<i32>,
            #[serde(rename = "ref")]
            ref_data: &'a ShWvRef,
            #[serde(skip_serializing_if = "Option::is_none")]
            placeholders: Option<&'a HashMap<usize, String>>,
        }

        let helper = Helper {
            idx: self.0.idx,
            src: &self.0.src,
            pre: &self.0.pre,
            tgt: &self.0.tgt,
            note: self.0.note.as_deref(),
            memo: self.0.memo.as_deref(),
            is_sub: self.0.is_sub,
            status: self.0.status,
            ref_data: &self.0.ref_data,
            placeholders: self.0.placeholders.as_ref(),
        };

        helper.serialize(serializer)
    }
}

/// JSON からデシリアライズする際の中間構造体（`"ref"` キーを `ref_data` にマッピング）
#[derive(Debug, Clone, Deserialize)]
pub struct ShWvUnitRaw {
    pub idx: usize,
    pub src: String,
    #[serde(default)]
    pub pre: String,
    #[serde(default)]
    pub tgt: String,
    pub note: Option<String>,
    pub memo: Option<String>,
    #[serde(rename = "isSub")]
    pub is_sub: Option<bool>,
    pub status: Option<i32>,
    #[serde(rename = "ref", default)]
    pub ref_data: ShWvRef,
    pub placeholders: Option<HashMap<usize, String>>,
}

impl From<ShWvUnitRaw> for ShWvUnit {
    fn from(r: ShWvUnitRaw) -> Self {
        Self {
            idx: r.idx,
            src: r.src,
            pre: r.pre,
            tgt: r.tgt,
            note: r.note,
            memo: r.memo,
            is_sub: r.is_sub,
            status: r.status,
            ref_data: r.ref_data,
            placeholders: r.placeholders,
        }
    }
}

pub struct ShWvUnitWrapper<'a>(pub &'a ShWvUnit);

/// ShWvData の本文コンテナ（セグメントリストおよび用語リスト）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShWvBody {
    #[serde(
        serialize_with = "serialize_units",
        deserialize_with = "deserialize_units"
    )]
    pub units: Vec<ShWvUnit>,
    pub terms: Vec<ShWvTerm>,
}

fn serialize_units<S>(units: &[ShWvUnit], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(units.len()))?;
    for unit in units {
        seq.serialize_element(&ShWvUnitWrapper(unit))?;
    }
    seq.end()
}

fn deserialize_units<'de, D>(deserializer: D) -> Result<Vec<ShWvUnit>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw_units: Vec<ShWvUnitRaw> = Vec::deserialize(deserializer)?;
    Ok(raw_units.into_iter().map(ShWvUnit::from).collect())
}

/// プロジェクト内の用語定義
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShWvTerm {
    pub src: String,
    pub tgt: String,
}

/// SheepCombWeb のルート JSON 構造体
///
/// [type.ts](file:///d:/Code/SheepFamily/Suffolk/type.ts) の `ShWvData` に完全準拠します。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShWvData {
    pub define: ShWvDefine,
    pub meta: ShWvMeta,
    pub body: ShWvBody,
}

impl ShWvData {
    /// 抽出された単一の中間表現 `DocumentIR` から `ShWvData` JSON 構造体を構築します。
    pub fn from_document_ir(ir: &DocumentIR) -> Self {
        Self::from_document_ir_with_seg(ir, false)
    }

    /// セグメンテーションフラグを指定して `ShWvData` を構築します。
    pub fn from_document_ir_with_seg(ir: &DocumentIR, is_seg: bool) -> Self {
        Self::from_multiple_irs(&[ir.clone()], is_seg)
    }

    /// 複数の `DocumentIR` を統合し、単一の `ShWvData` JSON 構造体を構築します。
    /// 各ファイルごとの範囲情報は `meta.files` に記録されます。
    pub fn from_multiple_irs(irs: &[DocumentIR], is_seg: bool) -> Self {
        let mut all_units = Vec::new();
        let mut files_info = Vec::new();
        let mut global_idx = 1;

        let src_lang = irs
            .first()
            .map(|ir| ir.meta.source_lang.clone())
            .unwrap_or_else(|| "en".to_string());
        let trg_lang = irs
            .first()
            .and_then(|ir| ir.meta.target_lang.clone())
            .unwrap_or_else(|| "ja".to_string());

        for ir in irs {
            let start = global_idx;
            for u in &ir.units {
                let placeholders_map = if !u.placeholders.is_empty() {
                    Some(tags_to_string_map(&u.placeholders))
                } else {
                    None
                };

                let shwv_unit = ShWvUnit {
                    idx: global_idx,
                    src: u.source.clone(),
                    pre: u.pre.clone().unwrap_or_default(),
                    tgt: u.target.clone().unwrap_or_default(),
                    note: u.note.clone(),
                    memo: u.memo.clone(),
                    is_sub: if u.is_sub { Some(true) } else { None },
                    status: Some(0),
                    ref_data: ShWvRef::default(),
                    placeholders: placeholders_map,
                };
                all_units.push(shwv_unit);
                global_idx += 1;
            }

            let end = if global_idx > start {
                global_idx - 1
            } else {
                start
            };

            files_info.push(ShWvFileInfo {
                name: ir.meta.source_file.clone(),
                start: if all_units.is_empty() { 0 } else { start },
                end,
            });
        }

        let bilingual_path = if irs.len() == 1 {
            format!("{}.shwv.json", irs[0].meta.source_file)
        } else {
            "bundle.shwv.json".to_string()
        };

        let workflow = ShWvWorkflow {
            index: 0,
            role: "Extract".to_string(),
            name: "Suffolk".to_string(),
            segmentation: Some(if is_seg {
                "seg".to_string()
            } else {
                "line".to_string()
            }),
        };

        let meta = ShWvMeta {
            bilingual_path,
            files: files_info,
            source_lang: src_lang,
            target_lang: trg_lang,
            project_name: None,
            workflow: Some(workflow),
        };

        Self {
            define: ShWvDefine::default(),
            meta,
            body: ShWvBody {
                units: all_units,
                terms: Vec::new(),
            },
        }
    }

    /// 各ユニットの確定訳文を取り出し、文結合（isSub処理）を行った上で
    /// 元文書へのマージ用マップ（`block_seq` -> `TranslatedUnit`）を作成します。
    ///
    /// `range` を指定した場合、そのインデックス範囲 `(start, end)` のユニットのみを対象とし、
    /// 出力マップのキーは対象ファイル内の段落連番（1始まり）にリセットされます。
    pub fn extract_recombined_translation_map(
        &self,
        range: Option<(usize, usize)>,
    ) -> HashMap<usize, TranslatedUnit> {
        let is_cjk = self.meta.target_lang.starts_with("ja") || self.meta.target_lang.starts_with("zh");

        let filtered_units: Vec<&ShWvUnit> = self
            .body
            .units
            .iter()
            .filter(|u| {
                if let Some((start, end)) = range {
                    u.idx >= start && u.idx <= end
                } else {
                    true
                }
            })
            .collect();

        let mut block_map = HashMap::new();
        let mut block_seq = 0;
        let mut current_target = String::new();
        let mut current_placeholders = HashMap::new();
        let mut current_note = None;
        let mut current_source = None;

        for unit in filtered_units {
            let is_sub = unit.is_sub.unwrap_or(false);
            if !is_sub {
                // 前のブロックを確定して登録
                if block_seq > 0 {
                    block_map.insert(
                        block_seq,
                        TranslatedUnit {
                            idx: block_seq,
                            source: current_source.take(),
                            target: current_target.clone(),
                            placeholders: current_placeholders.clone(),
                            note: current_note.take(),
                            is_sub: false,
                        },
                    );
                }

                block_seq += 1;
                current_target = unit.tgt.clone();
                current_placeholders = unit.placeholders.clone().unwrap_or_default();
                current_note = unit.note.clone();
                current_source = Some(unit.src.clone());
            } else {
                // サブセグメントを直前のブロックへ結合
                if !is_cjk && !current_target.is_empty() && !current_target.ends_with(' ') {
                    current_target.push(' ');
                }
                current_target.push_str(&unit.tgt);
                if let Some(ref ph) = unit.placeholders {
                    current_placeholders.extend(ph.clone());
                }
            }
        }

        // 最終ブロックの確定登録
        if block_seq > 0 {
            block_map.insert(
                block_seq,
                TranslatedUnit {
                    idx: block_seq,
                    source: current_source,
                    target: current_target,
                    placeholders: current_placeholders,
                    note: current_note,
                    is_sub: false,
                },
            );
        }

        block_map
    }

    /// 各ユニットの確定訳文を取り出し、元文書へのマージ用マップ（`idx` -> `TranslatedUnit`）を作成します。
    /// （後方互換性のための既存メソッド。セグメント結合済みのマップを返します）
    pub fn extract_translation_map(&self) -> HashMap<usize, TranslatedUnit> {
        self.extract_recombined_translation_map(None)
    }
}

/// 各フォーマットの Merger（復元処理）が参照する翻訳済みユニット情報
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranslatedUnit {
    /// ユニット通番（または段落通番）
    pub idx: usize,
    /// 原文テキスト（利用可能な場合）
    pub source: Option<String>,
    /// 翻訳後テキスト
    pub target: String,
    /// インライン書式プレースホルダー辞書
    pub placeholders: HashMap<usize, String>,
    /// コンテキスト情報（Excelセル番地や行番号など）
    pub note: Option<String>,
    /// サブセグメント（同一段落内での継続文）フラグ
    pub is_sub: bool,
}

impl TranslatedUnit {
    pub fn new(idx: usize, target: impl Into<String>) -> Self {
        Self {
            idx,
            source: None,
            target: target.into(),
            placeholders: HashMap::new(),
            note: None,
            is_sub: false,
        }
    }
}

/// 汎用的なセグメント再結合ヘルパー関数。
/// リスト状の `TranslatedUnit`（XLIFFパース後など）を受け取り、
/// `is_sub == true` の文を直前の親文に結合して段落連番（1始まり）をキーとするマップを生成します。
pub fn recombine_translated_units(
    units: &[TranslatedUnit],
    target_lang: &str,
) -> HashMap<usize, TranslatedUnit> {
    let is_cjk = target_lang.starts_with("ja") || target_lang.starts_with("zh");
    let mut block_map = HashMap::new();
    let mut block_seq = 0;
    let mut current_target = String::new();
    let mut current_placeholders = HashMap::new();
    let mut current_note = None;
    let mut current_source = None;

    for unit in units {
        if !unit.is_sub {
            if block_seq > 0 {
                block_map.insert(
                    block_seq,
                    TranslatedUnit {
                        idx: block_seq,
                        source: current_source.take(),
                        target: current_target.clone(),
                        placeholders: current_placeholders.clone(),
                        note: current_note.take(),
                        is_sub: false,
                    },
                );
            }
            block_seq += 1;
            current_target = unit.target.clone();
            current_placeholders = unit.placeholders.clone();
            current_note = unit.note.clone();
            current_source = unit.source.clone();
        } else {
            if !is_cjk && !current_target.is_empty() && !current_target.ends_with(' ') {
                current_target.push(' ');
            }
            current_target.push_str(&unit.target);
            current_placeholders.extend(unit.placeholders.clone());
        }
    }

    if block_seq > 0 {
        block_map.insert(
            block_seq,
            TranslatedUnit {
                idx: block_seq,
                source: current_source,
                target: current_target,
                placeholders: current_placeholders,
                note: current_note,
                is_sub: false,
            },
        );
    }

    block_map
}

#[cfg(test)]
mod tests {
    use super::*;
    use suffolk_ir::{DocumentFormat, DocumentMeta, UnitIR};

    #[test]
    fn test_multiple_irs_and_recombine() {
        let meta1 = DocumentMeta {
            source_file: "doc1.docx".to_string(),
            source_lang: "en".to_string(),
            target_lang: Some("ja".to_string()),
            format: DocumentFormat::Docx,
            encoding: "UTF-8".to_string(),
        };
        let mut ir1 = DocumentIR::new(meta1);
        let mut u1 = UnitIR::new("p1", 1, "Sentence 1.");
        u1.target = Some("文1。".to_string());
        let mut u2 = UnitIR::new("p1_s2", 2, "Sentence 2.");
        u2.is_sub = true;
        u2.target = Some("文2。".to_string());
        ir1.add_unit(u1);
        ir1.add_unit(u2);

        let meta2 = DocumentMeta {
            source_file: "doc2.xlsx".to_string(),
            source_lang: "en".to_string(),
            target_lang: Some("ja".to_string()),
            format: DocumentFormat::Xlsx,
            encoding: "UTF-8".to_string(),
        };
        let mut ir2 = DocumentIR::new(meta2);
        let mut u3 = UnitIR::new("c1", 1, "Doc2 Cell1.");
        u3.target = Some("ドキュメント2 セル1。".to_string());
        ir2.add_unit(u3);

        let shwv = ShWvData::from_multiple_irs(&[ir1, ir2], true);
        assert_eq!(shwv.meta.files.len(), 2);
        assert_eq!(shwv.meta.files[0].name, "doc1.docx");
        assert_eq!(shwv.meta.files[0].start, 1);
        assert_eq!(shwv.meta.files[0].end, 2);
        assert_eq!(shwv.meta.files[1].name, "doc2.xlsx");
        assert_eq!(shwv.meta.files[1].start, 3);
        assert_eq!(shwv.meta.files[1].end, 3);
        assert_eq!(shwv.meta.workflow.as_ref().unwrap().segmentation.as_deref(), Some("seg"));

        // File 1 recombined map (should combine u1 and u2 into block 1)
        let map1 = shwv.extract_recombined_translation_map(Some((1, 2)));
        assert_eq!(map1.len(), 1);
        assert_eq!(map1.get(&1).unwrap().target, "文1。文2。");

        // File 2 recombined map (should be block 1 in File 2's context)
        let map2 = shwv.extract_recombined_translation_map(Some((3, 3)));
        assert_eq!(map2.len(), 1);
        assert_eq!(map2.get(&1).unwrap().target, "ドキュメント2 セル1。");
    }
}
