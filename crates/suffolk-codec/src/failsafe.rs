use crate::shwv::TranslatedUnit;
use std::collections::HashMap;

/// テキスト内の `{@0}`, `{@1}` または `{0}` などのプレースホルダー記号を除去し、
/// 人間がそのまま DTP や Word に貼り付けられるプレーンな表示テキストを返します。
pub fn strip_placeholders(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' {
            let mut is_at = false;
            if let Some(&'@') = chars.peek() {
                chars.next();
                is_at = true;
            }

            let mut num_str = String::new();
            let mut is_ph = false;

            while let Some(&nc) = chars.peek() {
                if nc.is_ascii_digit() {
                    num_str.push(nc);
                    chars.next();
                } else if nc == '}' && !num_str.is_empty() {
                    chars.next();
                    is_ph = true;
                    break;
                } else {
                    break;
                }
            }

            if !is_ph {
                result.push('{');
                if is_at {
                    result.push('@');
                }
                result.push_str(&num_str);
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// 翻訳データをフェイルセーフ用の 2 言語対照 CSV 文字列（`idx,source,target,note`）として出力します。
///
/// # 引数
/// * `translations` - 翻訳済みユニットマップ
/// * `with_bom` - true の場合、Excel（特に日本語 Windows）で文字化けせず即座に開けるよう UTF-8 BOM（\u{FEFF}）を付与します。
pub fn export_failsafe_csv(
    translations: &HashMap<usize, TranslatedUnit>,
    with_bom: bool,
) -> Result<String, csv::Error> {
    let mut sorted_units: Vec<&TranslatedUnit> = translations.values().collect();
    sorted_units.sort_by_key(|u| u.idx);

    let mut wtr = csv::WriterBuilder::new().from_writer(Vec::new());

    // ヘッダー行
    wtr.write_record(&["idx", "source", "target", "note"])?;

    for u in sorted_units {
        let idx_str = u.idx.to_string();
        let src = u.source.as_deref().unwrap_or("");
        let tgt = &u.target;
        let note = u.note.as_deref().unwrap_or("");
        wtr.write_record(&[&idx_str, src, tgt, note])?;
    }

    let bytes = wtr.into_inner().map_err(|e| e.into_error())?;
    let mut s = String::from_utf8_lossy(&bytes).to_string();

    if with_bom {
        s.insert(0, '\u{FEFF}');
    }

    Ok(s)
}

/// 翻訳データの訳文（`tgt`）のみを行単位で並べたフェイルセーフ用 TXT 文字列を出力します。
///
/// # 特徴
/// - DTP オペレータや翻訳者が、InDesign や Word の該当段落へ順番にコピペできるよう、行順序を厳密に維持します。
/// - `raw_tags` が false（デフォルト）の場合、`{@0}` などのプレースホルダー記号を除去して純粋なテキストを出力します。
/// - 訳文が空の場合は、未翻訳段落の抜け漏れを防ぐため原文（利用可能な場合）にフォールバックします。
pub fn export_failsafe_txt(
    translations: &HashMap<usize, TranslatedUnit>,
    raw_tags: bool,
) -> String {
    let mut sorted_units: Vec<&TranslatedUnit> = translations.values().collect();
    sorted_units.sort_by_key(|u| u.idx);

    let mut lines = Vec::with_capacity(sorted_units.len());

    for u in sorted_units {
        let raw_text = if !u.target.trim().is_empty() {
            u.target.as_str()
        } else if let Some(ref s) = u.source {
            s.as_str()
        } else {
            ""
        };

        if raw_tags {
            lines.push(raw_text.to_string());
        } else {
            lines.push(strip_placeholders(raw_text));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_failsafe_csv_and_txt() {
        let mut map = HashMap::new();
        let mut u1 = TranslatedUnit::new(1, "こんにちは{@0}世界{@1}");
        u1.source = Some("Hello {@0}world{@1}".to_string());
        u1.note = Some("[Sheet1!A1]".to_string());
        map.insert(1, u1);

        let mut u2 = TranslatedUnit::new(2, "第2段落のテストです。");
        u2.source = Some("Second paragraph test.".to_string());
        u2.note = Some("[Story_u188]".to_string());
        map.insert(2, u2);

        // CSV test
        let csv_str = export_failsafe_csv(&map, true).unwrap();
        assert!(csv_str.starts_with('\u{FEFF}')); // BOM present
        assert!(csv_str.contains("idx,source,target,note"));
        assert!(csv_str.contains("こんにちは{@0}世界{@1}"));
        assert!(csv_str.contains("[Story_u188]"));

        // TXT test (strip tags)
        let txt_clean = export_failsafe_txt(&map, false);
        let lines: Vec<&str> = txt_clean.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "こんにちは世界"); // {@0} and {@1} removed!
        assert_eq!(lines[1], "第2段落のテストです。");

        // TXT test (raw tags)
        let txt_raw = export_failsafe_txt(&map, true);
        assert!(txt_raw.contains("こんにちは{@0}世界{@1}"));
    }
}
