use crate::traits::{DocumentFilter, DocumentMerger, FilterOptions, FormatError, MergeOptions};
use encoding_rs::Encoding;
use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use suffolk_codec::TranslatedUnit;
use suffolk_ir::{DocumentFormat, DocumentIR, DocumentMeta, UnitIR};

pub struct TextFilter;
pub struct TextMerger;

impl DocumentFilter for TextFilter {
    /// プレーンテキストファイル (`.txt`) から行単位で翻訳テキストを抽出します。
    ///
    /// ### 文字コード自動判定（UTF-8 & Shift-JIS / Windows-31J）
    /// `encoding_rs` を用いて UTF-8 バイト列か検証し、無効な場合は日本のレガシー環境で多用される
    /// Shift-JIS (Windows-31J / CP932) として自動デコードします。
    /// 空行はスキップし、行番号を `note` に記録します。
    fn extract<R: Read + Seek>(
        &self,
        mut reader: R,
        filename: &str,
        options: &FilterOptions,
    ) -> Result<DocumentIR, FormatError> {
        let mut raw_bytes = Vec::new();
        reader.read_to_end(&mut raw_bytes)?;

        // Auto-detect encoding or use option
        let (encoding_label, decoded_text) = if let Some(ref enc_name) = options.encoding {
            let enc = Encoding::for_label(enc_name.as_bytes()).ok_or_else(|| {
                FormatError::Encoding(format!("Unsupported encoding: {}", enc_name))
            })?;
            let (cow, _, malformed) = enc.decode(&raw_bytes);
            if malformed {
                return Err(FormatError::Encoding("Malformed bytes in text".to_string()));
            }
            (enc.name().to_string(), cow.into_owned())
        } else {
            // Check UTF-8 BOM or UTF-8 validity
            if let Ok(utf8_str) = std::str::from_utf8(&raw_bytes) {
                ("UTF-8".to_string(), utf8_str.to_string())
            } else {
                // Fallback to Shift-JIS (Windows-31J / ANSI on Japanese Windows)
                let (cow, _, _) = encoding_rs::SHIFT_JIS.decode(&raw_bytes);
                ("Shift_JIS".to_string(), cow.into_owned())
            }
        };

        let meta = DocumentMeta {
            source_file: filename.to_string(),
            source_lang: options.source_lang.clone(),
            target_lang: options.target_lang.clone(),
            format: DocumentFormat::PlainText,
            encoding: encoding_label,
        };

        let mut doc = DocumentIR::new(meta);
        let mut seq = 1;

        for (line_idx, line) in decoded_text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let mut unit = UnitIR::new(format!("line_{}", line_idx + 1), seq, line);
            unit.context.row = Some((line_idx + 1) as u32);
            unit.populate_note_from_context(Some(&format!("Line {}", line_idx + 1)));
            doc.add_unit(unit);
            seq += 1;
        }

        Ok(doc)
    }
}

impl DocumentMerger for TextMerger {
    /// 翻訳結果を元のプレーンテキスト骨格にマージします。
    /// 空行や改行構成を維持し、元の文字エンコーディング（UTF-8 または Shift-JIS）に合わせてエンコード出力します。
    fn merge<R: Read + Seek, W: Write + Seek>(
        &self,
        mut skeleton: R,
        translations: &HashMap<usize, TranslatedUnit>,
        mut writer: W,
        options: &MergeOptions,
    ) -> Result<(), FormatError> {
        let mut raw_bytes = Vec::new();
        skeleton.read_to_end(&mut raw_bytes)?;

        // Detect encoding
        let (encoding, decoded_text) = if let Some(ref enc_name) = options.encoding {
            let enc = Encoding::for_label(enc_name.as_bytes()).ok_or_else(|| {
                FormatError::Encoding(format!("Unsupported encoding: {}", enc_name))
            })?;
            let (cow, _, _) = enc.decode(&raw_bytes);
            (enc, cow.into_owned())
        } else if let Ok(utf8_str) = std::str::from_utf8(&raw_bytes) {
            (encoding_rs::UTF_8, utf8_str.to_string())
        } else {
            let (cow, _, _) = encoding_rs::SHIFT_JIS.decode(&raw_bytes);
            (encoding_rs::SHIFT_JIS, cow.into_owned())
        };

        let mut seq = 1;
        let mut output_lines = Vec::new();

        for line in decoded_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                output_lines.push(line.to_string());
            } else {
                if let Some(trans) = translations.get(&seq) {
                    output_lines.push(trans.target.clone());
                } else {
                    output_lines.push(line.to_string());
                }
                seq += 1;
            }
        }

        let full_text = output_lines.join("\r\n") + "\r\n";
        let (encoded_bytes, _, _) = encoding.encode(&full_text);
        writer.write_all(&encoded_bytes)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_plain_text_roundtrip() {
        let sample = "Line 1\nLine 2\n\nLine 3";
        let filter = TextFilter;
        let opts = FilterOptions {
            source_lang: "en".to_string(),
            target_lang: Some("ja".to_string()),
            ..Default::default()
        };

        let doc = filter
            .extract(Cursor::new(sample), "sample.txt", &opts)
            .unwrap();
        assert_eq!(doc.units.len(), 3);
        assert_eq!(doc.units[0].source, "Line 1");
        assert_eq!(doc.units[1].source, "Line 2");
        assert_eq!(doc.units[2].source, "Line 3");

        let mut trans = HashMap::new();
        trans.insert(1, TranslatedUnit::new(1, "1行目"));
        trans.insert(2, TranslatedUnit::new(2, "2行目"));
        trans.insert(3, TranslatedUnit::new(3, "3行目"));

        let merger = TextMerger;
        let mut out = Cursor::new(Vec::new());
        merger
            .merge(
                Cursor::new(sample),
                &trans,
                &mut out,
                &MergeOptions::default(),
            )
            .unwrap();

        let out_str = String::from_utf8(out.into_inner()).unwrap();
        assert!(out_str.contains("1行目"));
        assert!(out_str.contains("2行目"));
        assert!(out_str.contains("3行目"));
    }
}
