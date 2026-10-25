use serde::{Deserialize, Deserializer, Serialize};
use suffolk_formats::{FilterOptions, MergeOptions, WorksheetRule};

fn default_src_lang() -> String {
    "en".to_string()
}

fn default_trg_lang() -> String {
    "ja".to_string()
}

/// 抽出（Filter / Extract）実行時のオプション設定。
/// JS/TS からプレーンな JSON オブジェクトとして受け取ります。
/// camelCase と snake_case の両方に対応しています。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmFilterOptions {
    /// 文単位セグメンテーションを行うか (-seg / --seg)
    #[serde(default, alias = "segment")]
    pub seg: bool,

    /// 出力形式: "shwv" (デフォルト) または "xliff" / "xlf"
    #[serde(default)]
    pub to: Option<String>,

    /// 原文の言語コード (例: "en", "ja")
    #[serde(default = "default_src_lang", alias = "source_lang", alias = "src_lang")]
    pub src_lang: String,

    /// 訳文の言語コード (例: "ja", "en", "zh-CN")
    #[serde(default = "default_trg_lang", alias = "target_lang", alias = "trg_lang")]
    pub trg_lang: String,

    /// タグ最小化を無効化し、生の装飾タグを保持するか
    #[serde(default, alias = "raw_tags")]
    pub raw_tags: bool,

    /// 文字エンコーディング指定 (例: "UTF-8", "Shift_JIS")
    #[serde(default)]
    pub encoding: Option<String>,

    // --- Excel (XLSX) 多言語列フィルタ設定 ---
    /// 原文列指定 (例: "A" または ["A"] または "A,D")
    #[serde(default, deserialize_with = "deserialize_opt_string_list", alias = "xlsx_source", alias = "xlsx_source_cols")]
    pub xlsx_source: Option<Vec<String>>,

    /// 訳文列指定 (例: "B" または ["B"] または "B,E")
    #[serde(default, deserialize_with = "deserialize_opt_string_list", alias = "xlsx_target", alias = "xlsx_target_cols")]
    pub xlsx_target: Option<Vec<String>>,

    /// コンテキスト情報列指定 (例: "C" または ["C"])
    #[serde(default, deserialize_with = "deserialize_opt_string_list", alias = "xlsx_context", alias = "xlsx_context_cols")]
    pub xlsx_context: Option<Vec<String>>,

    /// スキップする行番号 (例: 1 または "1" または [1, 2] または "1,2")
    #[serde(default, deserialize_with = "deserialize_opt_num_list", alias = "xlsx_skip_rows")]
    pub xlsx_skip_rows: Option<Vec<u32>>,

    /// シート名正規表現パターン (例: "Sheet1", ".*")
    #[serde(default, alias = "xlsx_sheet", alias = "xlsx_sheet_pattern")]
    pub xlsx_sheet: Option<String>,

    /// CSV 設定文字列 (rules.csv のファイル内容そのものを渡す場合に使用)
    #[serde(default, alias = "xlsx_config_csv", alias = "xlsx_config")]
    pub xlsx_config_csv: Option<String>,
}

impl Default for WasmFilterOptions {
    fn default() -> Self {
        Self {
            seg: false,
            to: None,
            src_lang: default_src_lang(),
            trg_lang: default_trg_lang(),
            raw_tags: false,
            encoding: None,
            xlsx_source: None,
            xlsx_target: None,
            xlsx_context: None,
            xlsx_skip_rows: None,
            xlsx_sheet: None,
            xlsx_config_csv: None,
        }
    }
}

impl WasmFilterOptions {
    /// 内部の `FilterOptions` へ変換します。
    pub fn to_filter_options(&self) -> Result<FilterOptions, String> {
        let mut opts = FilterOptions {
            source_lang: self.src_lang.clone(),
            target_lang: Some(self.trg_lang.clone()),
            encoding: self.encoding.clone(),
            minimize_tags: !self.raw_tags,
            ..Default::default()
        };

        // CSV 設定文字列が指定されている場合はパース
        if let Some(csv_str) = &self.xlsx_config_csv {
            let rules = WorksheetRule::parse_from_str(csv_str)
                .map_err(|e| format!("Invalid xlsx_config_csv: {}", e))?;
            if let Some(rule) = rules.first() {
                opts.xlsx_sheet_pattern = Some(rule.sheet_pattern.clone());
                opts.xlsx_source_cols = rule.source_cols.clone();
                opts.xlsx_target_cols = rule.target_cols.clone();
                opts.xlsx_context_cols = rule.context_cols.clone();
                opts.xlsx_skip_rows = rule.skip_rows.clone();
            }
        }

        // 個別指定による上書き
        if let Some(s) = &self.xlsx_source {
            opts.xlsx_source_cols = s.clone();
        }
        if let Some(t) = &self.xlsx_target {
            opts.xlsx_target_cols = t.clone();
        }
        if let Some(c) = &self.xlsx_context {
            opts.xlsx_context_cols = c.clone();
        }
        if let Some(r) = &self.xlsx_skip_rows {
            opts.xlsx_skip_rows = r.clone();
        }
        if let Some(p) = &self.xlsx_sheet {
            opts.xlsx_sheet_pattern = Some(p.clone());
        }

        Ok(opts)
    }
}

/// 復元（Merge）実行時のオプション設定。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WasmMergeOptions {
    /// 文字エンコーディング指定
    #[serde(default)]
    pub encoding: Option<String>,

    /// 原文列指定 (Excel用)
    #[serde(default, deserialize_with = "deserialize_opt_string_list", alias = "xlsx_source", alias = "xlsx_source_cols")]
    pub xlsx_source: Option<Vec<String>>,

    /// 訳文列指定 (Excel用)
    #[serde(default, deserialize_with = "deserialize_opt_string_list", alias = "xlsx_target", alias = "xlsx_target_cols")]
    pub xlsx_target: Option<Vec<String>>,

    /// CSV 設定文字列 (rules.csv のファイル内容そのものを渡す場合に使用)
    #[serde(default, alias = "xlsx_config_csv", alias = "xlsx_config")]
    pub xlsx_config_csv: Option<String>,
}

impl WasmMergeOptions {
    /// 内部の `MergeOptions` へ変換します。
    pub fn to_merge_options(&self) -> Result<MergeOptions, String> {
        let mut opts = MergeOptions {
            encoding: self.encoding.clone(),
            ..Default::default()
        };

        if let Some(csv_str) = &self.xlsx_config_csv {
            let rules = WorksheetRule::parse_from_str(csv_str)
                .map_err(|e| format!("Invalid xlsx_config_csv: {}", e))?;
            if let Some(rule) = rules.first() {
                opts.xlsx_source_cols = rule.source_cols.clone();
                opts.xlsx_target_cols = rule.target_cols.clone();
            }
        }

        if let Some(s) = &self.xlsx_source {
            opts.xlsx_source_cols = s.clone();
        }
        if let Some(t) = &self.xlsx_target {
            opts.xlsx_target_cols = t.clone();
        }

        Ok(opts)
    }
}

// --- ヘルパー: 文字列または配列を Vec<String> に柔軟デシリアライズ ---
fn deserialize_opt_string_list<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        Str(String),
        Vec(Vec<String>),
    }

    let opt = Option::<StringOrVec>::deserialize(deserializer)?;
    Ok(opt.map(|val| match val {
        StringOrVec::Str(s) => s
            .split(|c| c == ',' || c == ';' || c == ' ')
            .map(|part| part.trim().to_ascii_uppercase())
            .filter(|part| !part.is_empty())
            .collect(),
        StringOrVec::Vec(v) => v
            .into_iter()
            .map(|part| part.trim().to_ascii_uppercase())
            .filter(|part| !part.is_empty())
            .collect(),
    }))
}

// --- ヘルパー: 数値、文字列、配列を Vec<u32> に柔軟デシリアライズ ---
fn deserialize_opt_num_list<'de, D>(deserializer: D) -> Result<Option<Vec<u32>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrVec {
        SingleNum(u32),
        SingleStr(String),
        VecNum(Vec<u32>),
        VecStr(Vec<String>),
    }

    let opt = Option::<NumOrVec>::deserialize(deserializer)?;
    Ok(opt.map(|val| match val {
        NumOrVec::SingleNum(n) => vec![n],
        NumOrVec::SingleStr(s) => s
            .split(|c| c == ',' || c == ';' || c == ' ')
            .filter_map(|part| part.trim().parse::<u32>().ok())
            .collect(),
        NumOrVec::VecNum(v) => v,
        NumOrVec::VecStr(v) => v
            .into_iter()
            .filter_map(|part| part.trim().parse::<u32>().ok())
            .collect(),
    }))
}
