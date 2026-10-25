use std::fs::File;
use std::path::Path;
use thiserror::Error;

/// CSV設定ファイルの読み込みに関するエラー定義
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("CSV parse error: {0}")]
    Csv(#[from] csv::Error),
    #[error("Invalid configuration format: {0}")]
    Invalid(String),
}

/// Excel ワークシートごとの多言語抽出・マージ設定ルール
///
/// 翻訳者が使い慣れた表計算ソフトやテキストエディタで簡単に編集できるよう、
/// シンプルな CSV 形式（`sheet,source,target,context,skip_rows`）を採用しています。
#[derive(Debug, Clone, Default)]
pub struct WorksheetRule {
    /// 対象シート名の正規表現パターン（例: "Device_.*", "Sheet1" 等、省略時は ".*"）
    pub sheet_pattern: String,
    /// 原文が格納されている列（例: `["A"]` や `["A", "D"]`）
    pub source_cols: Vec<String>,
    /// 訳文を配置・参照する列（例: `["B"]` や `["B", "E"]`）
    pub target_cols: Vec<String>,
    /// 翻訳者に補足提示するコンテキスト情報列（例: `["C"]`）
    pub context_cols: Vec<String>,
    /// 除外するヘッダー行などの行番号リスト（1始まり、例: `[1]` や `[1, 2]`）
    pub skip_rows: Vec<u32>,
}

impl WorksheetRule {
    /// CSV ファイルからワークシート設定ルールを読み込みます。
    pub fn load_from_csv<P: AsRef<Path>>(path: P) -> Result<Vec<Self>, ConfigError> {
        let file = File::open(path)?;
        Self::parse_from_reader(file)
    }

    /// CSV 文字列から直接ワークシート設定ルールを読み込みます（WASM / メモリ実行用）。
    pub fn parse_from_str(csv_str: &str) -> Result<Vec<Self>, ConfigError> {
        Self::parse_from_reader(csv_str.as_bytes())
    }

    /// 任意のリーダーからワークシート設定ルールを読み込みます。
    pub fn parse_from_reader<R: std::io::Read>(reader: R) -> Result<Vec<Self>, ConfigError> {
        let mut rdr = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .flexible(true)
            .from_reader(reader);

        let mut rules = Vec::new();

        for result in rdr.records() {
            let record = result?;
            if record.is_empty() {
                continue;
            }

            let sheet_pattern = record.get(0).unwrap_or(".*").trim().to_string();
            let source_str = record.get(1).unwrap_or("").trim();
            let target_str = record.get(2).unwrap_or("").trim();
            let context_str = record.get(3).unwrap_or("").trim();
            let skip_str = record.get(4).unwrap_or("").trim();

            let source_cols = parse_col_list(source_str);
            let target_cols = parse_col_list(target_str);
            let context_cols = parse_col_list(context_str);
            let skip_rows = parse_num_list(skip_str);

            rules.push(WorksheetRule {
                sheet_pattern: if sheet_pattern.is_empty() {
                    ".*".to_string()
                } else {
                    sheet_pattern
                },
                source_cols,
                target_cols,
                context_cols,
                skip_rows,
            });
        }

        Ok(rules)
    }
}

fn parse_col_list(s: &str) -> Vec<String> {
    s.split(|c| c == ',' || c == ';' || c == ' ')
        .map(|col| col.trim().to_ascii_uppercase())
        .filter(|col| !col.is_empty())
        .collect()
}

fn parse_num_list(s: &str) -> Vec<u32> {
    s.split(|c| c == ',' || c == ';' || c == ' ')
        .map(|num| num.trim().parse::<u32>().ok())
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_csv() {
        let csv_data = "sheet,source,target,context,skip_rows\nSheet1,A,B,C,1\n.*,D,E,,1 2\n";
        let mut rdr = csv::ReaderBuilder::new().from_reader(csv_data.as_bytes());
        let mut rules = Vec::new();
        for r in rdr.records() {
            let rec = r.unwrap();
            rules.push(WorksheetRule {
                sheet_pattern: rec[0].to_string(),
                source_cols: parse_col_list(&rec[1]),
                target_cols: parse_col_list(&rec[2]),
                context_cols: parse_col_list(&rec[3]),
                skip_rows: parse_num_list(&rec[4]),
            });
        }
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].sheet_pattern, "Sheet1");
        assert_eq!(rules[0].source_cols, vec!["A"]);
        assert_eq!(rules[0].target_cols, vec!["B"]);
        assert_eq!(rules[0].context_cols, vec!["C"]);
        assert_eq!(rules[0].skip_rows, vec![1]);

        assert_eq!(rules[1].sheet_pattern, ".*");
        assert_eq!(rules[1].source_cols, vec!["D"]);
        assert_eq!(rules[1].skip_rows, vec![1, 2]);
    }
}
