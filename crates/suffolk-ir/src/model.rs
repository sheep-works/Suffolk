use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// ドキュメント全体の中間表現（Suffolk Intermediate Representation）。
/// 各種原稿フォーマット（DOCX, XLSX, PPTX, HTML, MD, TXT）から抽出された
/// メタ情報および翻訳単位（セグメント）を統一的に保持します。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentIR {
    /// ドキュメント全体のメタ情報（元ファイル名、言語コード、文字コードなど）
    pub meta: DocumentMeta,
    /// 抽出された翻訳単位（セグメント）のリスト
    pub units: Vec<UnitIR>,
}

impl DocumentIR {
    /// 新規の DocumentIR を生成します。
    pub fn new(meta: DocumentMeta) -> Self {
        Self {
            meta,
            units: Vec::new(),
        }
    }

    /// 翻訳単位を追加します。
    pub fn add_unit(&mut self, unit: UnitIR) {
        self.units.push(unit);
    }
}

/// ソースドキュメントのメタ情報。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentMeta {
    /// 原稿ファイル名（例: "Device_Config.xlsx"）
    pub source_file: String,
    /// 原文の言語コード（例: "en", "ja"）
    pub source_lang: String,
    /// 訳文の言語コード（例: "ja", "zh"）
    pub target_lang: Option<String>,
    /// ドキュメントのフォーマット種別
    pub format: DocumentFormat,
    /// 原稿の文字エンコーディング（例: "UTF-8", "Shift_JIS"）
    pub encoding: String,
}

/// 対応するドキュメントフォーマット一覧。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DocumentFormat {
    Docx,
    Xlsx,
    Pptx,
    PlainText,
    Markdown,
    Html,
    Idml,
    Svg,  // 将来拡張用
}

impl DocumentFormat {
    /// ファイル拡張子文字列からフォーマット種別を判定します。
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "docx" => Some(DocumentFormat::Docx),
            "xlsx" => Some(DocumentFormat::Xlsx),
            "pptx" => Some(DocumentFormat::Pptx),
            "txt" | "text" => Some(DocumentFormat::PlainText),
            "md" | "markdown" => Some(DocumentFormat::Markdown),
            "html" | "htm" => Some(DocumentFormat::Html),
            "idml" => Some(DocumentFormat::Idml),
            "svg" => Some(DocumentFormat::Svg),
            _ => None,
        }
    }
}

/// 単一の翻訳単位（セグメントまたは段落）。
/// `type.ts` の `ShWvUnit` と 1:1 にマッピングできるよう設計されています。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnitIR {
    /// 内部ユニーク識別子（例: "u1", "Sheet1!A2", "ppt/slides/slide1.xml_p1"）
    pub id: String,
    /// 1から始まる連番インデックス（ShWvUnit.idx に直結）
    pub seq: usize,
    /// インライン装飾タグが `{0}`, `{1}` などのトークンで正規化された原文テキスト
    pub source: String,
    /// 訳文テキスト（多言語 Excel の target 列など、原稿内に事前訳が存在する場合に格納）
    pub target: Option<String>,
    /// 事前翻訳 / 機械翻訳テキスト（抽出時は target と同一または空）
    pub pre: Option<String>,
    /// 翻訳者向けノート。シート名、スライド番号、セル番地、多言語コンテキスト列の内容など
    /// 翻訳前ドキュメントの背景情報を集約して格納します。
    pub note: Option<String>,
    /// 翻訳作業者が翻訳中に残す作業用メモ（抽出時は空を維持）
    pub memo: Option<String>,
    /// プレースホルダーインデックス（0, 1, 2...）と元のインラインタグの対応表
    pub placeholders: HashMap<usize, InlineTag>,
    /// 構造化された詳細コンテキスト情報
    pub context: UnitContext,
    /// 同一ブロック/段落内の2文目以降であるかを示すフラグ（type.ts の isSub にマッピング）
    #[serde(default)]
    pub is_sub: bool,
    /// 元の所属段落・ブロックを識別するグループID（マージ時の再結合キー）
    #[serde(default)]
    pub group_id: usize,
    /// 文末の後続空白・区切り文字（マージ時に原文の空白・改行を復元するため保持）
    #[serde(default)]
    pub trailing_ws: String,
}

impl UnitIR {
    /// 新規の UnitIR を生成します。
    pub fn new(id: impl Into<String>, seq: usize, source: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            seq,
            source: source.into(),
            target: None,
            pre: None,
            note: None,
            memo: None,
            placeholders: HashMap::new(),
            context: UnitContext::default(),
            is_sub: false,
            group_id: seq,
            trailing_ws: String::new(),
        }
    }

    /// `UnitContext` の内容および追加のコンテキスト文字列から、
    /// 人間可読な `note` 文字列（例: `[Sheet: Sheet1 | Cell: A10] Added: Ver 2.0`）を自動構築します。
    pub fn populate_note_from_context(&mut self, extra_note: Option<&str>) {
        let mut parts = Vec::new();

        if let Some(ref sheet) = self.context.sheet_name {
            parts.push(format!("Sheet: {}", sheet));
        }
        if let Some(ref cell) = self.context.cell_ref {
            parts.push(format!("Cell: {}", cell));
        }
        if let Some(ref header) = self.context.column_header {
            parts.push(format!("Header: {}", header));
        }
        if let Some(slide) = self.context.slide_number {
            parts.push(format!("Slide: {}", slide));
        }
        if let Some(ref shape) = self.context.shape_name {
            parts.push(format!("Shape: {}", shape));
        }
        if self.context.is_speaker_notes {
            parts.push("Speaker Notes".to_string());
        }
        if let Some(ref style) = self.context.style_name {
            parts.push(format!("Style: {}", style));
        }

        let prefix = if !parts.is_empty() {
            format!("[{}]", parts.join(" | "))
        } else {
            String::new()
        };

        self.note = match (prefix.is_empty(), extra_note) {
            (false, Some(n)) if !n.trim().is_empty() => Some(format!("{} {}", prefix, n.trim())),
            (false, _) => Some(prefix),
            (true, Some(n)) if !n.trim().is_empty() => Some(n.trim().to_string()),
            _ => None,
        };
    }
}

/// インライン装飾コードの定義。
/// `{0}` などで置き換えられた元の XML/HTML タグの生文字列やタグ種別を保持します。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InlineTag {
    /// 置換前の生タグ文字列（例: `<span class="bigger">`, `<w:rPr>...</w:rPr>`）
    pub raw: String,
    /// 開始タグ、終了タグ、単独タグの区分
    pub tag_type: TagType,
    /// タグ名（例: "span", "rPr", "b"）
    pub name: Option<String>,
}

/// インラインタグの種別。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TagType {
    /// 開始タグ（例: `<b>`, `<span ...>`）
    Opening,
    /// 終了タグ（例: `</b>`, `</span>`）
    Closing,
    /// 単独タグ（例: `<br/>`, `<img .../>`）
    Standalone,
}

/// ドキュメント固有の背景・構造化コンテキスト情報。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UnitContext {
    // --- Excel 固有情報 ---
    /// ワークシート名（例: "Sheet1", "果物リスト"）
    pub sheet_name: Option<String>,
    /// セル番地（例: "A2", "B10"）
    pub cell_ref: Option<String>,
    /// 1から始まる行番号
    pub row: Option<u32>,
    /// 列記号（例: "A", "AA"）
    pub col: Option<String>,
    /// 該当列のヘッダー名（ヘッダー行指定時、例: "Source", "Target"）
    pub column_header: Option<String>,

    // --- PowerPoint 固有情報 ---
    /// 1から始まるスライド番号
    pub slide_number: Option<u32>,
    /// オートシェイプやテキストボックスの名前（例: "Title 1", "TextBox 5"）
    pub shape_name: Option<String>,
    /// 発表者ノート領域（Speaker Notes）かどうかのフラグ
    pub is_speaker_notes: bool,

    // --- Word / HTML / Markdown 固有情報 ---
    /// 段落スタイル名（Word: "Heading 1", "1", "Normal" など）
    pub style_name: Option<String>,
    /// HTML 等の DOM パス（例: "html > body > main > p"）
    pub dom_path: Option<String>,

    // --- 汎用属性 ---
    /// フォーマット固有の追加属性マップ
    pub extra: HashMap<String, String>,
}
