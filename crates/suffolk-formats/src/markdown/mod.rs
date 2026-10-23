use crate::traits::{DocumentFilter, DocumentMerger, FilterOptions, FormatError, MergeOptions};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, Write};
use suffolk_codec::TranslatedUnit;
use suffolk_ir::{restore_placeholders, string_map_to_tags, DocumentFormat, DocumentIR, DocumentMeta, UnitIR};

pub struct MarkdownFilter;
pub struct MarkdownMerger;

impl DocumentFilter for MarkdownFilter {
    /// Markdown 文書 (`.md`) から翻訳対象ブロックを抽出します。
    ///
    /// ### 処理概要
    /// 1. YAML フロントマター（`---` で囲まれたメタデータ領域）を検出して保護（スキップ）。
    /// 2. フェンスされたコードブロック（``` や ~~~）を検出して保護（スキップ）。
    /// 3. 見出し記号（`# `〜`###### `）、リストマーカー（`- `, `* `, `+ `）、引用符（`> `）を
    ///    構文接頭辞として分離し、翻訳対象テキストのみを抽出。
    /// 4. 空行をスキップし、行番号およびブロック種別を `context` と `note` に記録。
    fn extract<R: Read + Seek>(
        &self,
        reader: R,
        filename: &str,
        options: &FilterOptions,
    ) -> Result<DocumentIR, FormatError> {
        let buf_reader = BufReader::new(reader);
        let meta = DocumentMeta {
            source_file: filename.to_string(),
            source_lang: options.source_lang.clone(),
            target_lang: options.target_lang.clone(),
            format: DocumentFormat::Markdown,
            encoding: "UTF-8".to_string(),
        };

        let mut doc = DocumentIR::new(meta);
        let mut seq = 1;
        let mut in_frontmatter = false;
        let mut in_code_block = false;
        let mut line_number = 0;

        for line_res in buf_reader.lines() {
            let line = line_res?;
            line_number += 1;
            let trimmed = line.trim();

            // 1. YAML frontmatter check
            if line_number == 1 && trimmed == "---" {
                in_frontmatter = true;
                continue;
            }
            if in_frontmatter {
                if trimmed == "---" {
                    in_frontmatter = false;
                }
                continue;
            }

            // 2. Fenced code block check
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_code_block = !in_code_block;
                continue;
            }
            if in_code_block {
                continue;
            }

            // 3. Blank line check
            if trimmed.is_empty() {
                continue;
            }

            // 4. Identify block type and extract text
            let (text, prefix, style) = if trimmed.starts_with('#') {
                let hashes = trimmed.chars().take_while(|&c| c == '#').count();
                if hashes <= 6 && trimmed.chars().nth(hashes) == Some(' ') {
                    let content = &trimmed[hashes + 1..];
                    (
                        content.to_string(),
                        trimmed[..hashes + 1].to_string(),
                        Some(format!("Heading {}", hashes)),
                    )
                } else {
                    (line.clone(), String::new(), None)
                }
            } else if trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("+ ")
            {
                let prefix_len = line.find(|c| c == '-' || c == '*' || c == '+').unwrap() + 2;
                (
                    line[prefix_len..].to_string(),
                    line[..prefix_len].to_string(),
                    Some("List Item".to_string()),
                )
            } else if trimmed.starts_with("> ") {
                let prefix_len = line.find('>').unwrap() + 2;
                (
                    line[prefix_len..].to_string(),
                    line[..prefix_len].to_string(),
                    Some("Blockquote".to_string()),
                )
            } else {
                (line.clone(), String::new(), None)
            };

            let mut unit = UnitIR::new(format!("md_{}", line_number), seq, text);
            unit.context.row = Some(line_number as u32);
            unit.context.style_name = style;
            if !prefix.is_empty() {
                unit.context.extra.insert("prefix".to_string(), prefix);
            }
            unit.populate_note_from_context(Some(&format!("Line {}", line_number)));
            doc.add_unit(unit);
            seq += 1;
        }

        Ok(doc)
    }
}

impl DocumentMerger for MarkdownMerger {
    /// 翻訳結果を元の Markdown 骨格にマージします。
    /// フロントマター、コードブロック、構文接頭辞（`# `, `- ` など）を元の状態に保ちながら翻訳テキストを出力します。
    fn merge<R: Read + Seek, W: Write + Seek>(
        &self,
        skeleton: R,
        translations: &HashMap<usize, TranslatedUnit>,
        mut writer: W,
        _options: &MergeOptions,
    ) -> Result<(), FormatError> {
        let buf_reader = BufReader::new(skeleton);
        let mut seq = 1;
        let mut in_frontmatter = false;
        let mut in_code_block = false;
        let mut line_number = 0;

        for line_res in buf_reader.lines() {
            let line = line_res?;
            line_number += 1;
            let trimmed = line.trim();

            if line_number == 1 && trimmed == "---" {
                in_frontmatter = true;
                writeln!(writer, "{}", line)?;
                continue;
            }
            if in_frontmatter {
                if trimmed == "---" {
                    in_frontmatter = false;
                }
                writeln!(writer, "{}", line)?;
                continue;
            }

            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_code_block = !in_code_block;
                writeln!(writer, "{}", line)?;
                continue;
            }
            if in_code_block || trimmed.is_empty() {
                writeln!(writer, "{}", line)?;
                continue;
            }

            // Translatable line
            if let Some(trans) = translations.get(&seq) {
                let tags = string_map_to_tags(&trans.placeholders);
                let restored_target = restore_placeholders(&trans.target, &tags);

                // Preserve original prefix
                let prefix = if trimmed.starts_with('#') {
                    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
                    if hashes <= 6 && trimmed.chars().nth(hashes) == Some(' ') {
                        let indent = &line[..line.find('#').unwrap_or(0)];
                        format!("{}{}", indent, &trimmed[..hashes + 1])
                    } else {
                        String::new()
                    }
                } else if trimmed.starts_with("- ")
                    || trimmed.starts_with("* ")
                    || trimmed.starts_with("+ ")
                {
                    let prefix_len = line.find(|c| c == '-' || c == '*' || c == '+').unwrap() + 2;
                    line[..prefix_len].to_string()
                } else if trimmed.starts_with("> ") {
                    let prefix_len = line.find('>').unwrap() + 2;
                    line[..prefix_len].to_string()
                } else {
                    let leading_spaces = line.chars().take_while(|c| c.is_whitespace()).collect::<String>();
                    leading_spaces
                };

                writeln!(writer, "{}{}", prefix, restored_target)?;
                seq += 1;
            } else {
                writeln!(writer, "{}", line)?;
                seq += 1;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_markdown_filter_and_merge() {
        let md = r#"---
title: Test Page
---
# Heading 1
Paragraph text here.

```rust
fn main() { println!("code"); }
```

- List item 1
- List item 2
"#;

        let filter = MarkdownFilter;
        let opts = FilterOptions {
            source_lang: "en".to_string(),
            target_lang: Some("ja".to_string()),
            ..Default::default()
        };

        let doc = filter.extract(Cursor::new(md), "test.md", &opts).unwrap();
        assert_eq!(doc.units.len(), 4);
        assert_eq!(doc.units[0].source, "Heading 1");
        assert_eq!(doc.units[1].source, "Paragraph text here.");
        assert_eq!(doc.units[2].source, "List item 1");
        assert_eq!(doc.units[3].source, "List item 2");

        let mut trans = HashMap::new();
        trans.insert(1, TranslatedUnit::new(1, "見出し1"));
        trans.insert(2, TranslatedUnit::new(2, "段落テキストです。"));
        trans.insert(3, TranslatedUnit::new(3, "項目1"));
        trans.insert(4, TranslatedUnit::new(4, "項目2"));

        let merger = MarkdownMerger;
        let mut out = Cursor::new(Vec::new());
        merger
            .merge(Cursor::new(md), &trans, &mut out, &MergeOptions::default())
            .unwrap();

        let out_str = String::from_utf8(out.into_inner()).unwrap();
        assert!(out_str.contains("# 見出し1"));
        assert!(out_str.contains("段落テキストです。"));
        assert!(out_str.contains("```rust")); // Code block preserved intact
        assert!(out_str.contains("- 項目1"));
        assert!(out_str.contains("- 項目2"));
    }
}
