use crate::openxml::minimizer::{coalesce_runs, runs_to_normalized_text, ProcessedRun};
use crate::openxml::zip_handler::{read_zip_entry_to_string, rewrite_zip};
use crate::traits::{DocumentFilter, DocumentMerger, FilterOptions, FormatError, MergeOptions};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use suffolk_codec::TranslatedUnit;
use suffolk_ir::{DocumentFormat, DocumentIR, DocumentMeta, UnitIR};
use zip::ZipArchive;

pub struct IdmlFilter;
pub struct IdmlMerger;

/// Adobe InDesign (`.idml`) 文書から翻訳対象テキストを抽出します。
///
/// ### IDML の構造
/// IDML は DOCX や PPTX と同様の PKZip アーカイブです。
/// テキスト実体は `Stories/Story_*.xml` に格納されており、
/// `<ParagraphStyleRange>`（段落）→ `<CharacterStyleRange>`（文字スタイル）→ `<Content>`（テキスト）
/// という階層構造を持ちます。
///
/// ### 処理概要
/// 1. `designmap.xml` からストーリーの登録順序（`idPkg:Story src="..."`）を取得。
/// 2. 各ストーリー XML からテキストとスタイル（太字、斜体、下線、打消線、文字色等）を走査。
/// 3. 段落単位および `<Br />`（強制改行）単位でセグメント化。
/// 4. タグ最小化ロジック（`coalesce_runs`）を適用し、インライン書式境界に `{0}`, `{1}` プレースホルダーを付与。
/// 5. ストーリー識別子（例: `Story_u188`）を `note` に記録。
impl DocumentFilter for IdmlFilter {
    fn extract<R: Read + Seek>(
        &self,
        mut reader: R,
        filename: &str,
        options: &FilterOptions,
    ) -> Result<DocumentIR, FormatError> {
        let mut archive = ZipArchive::new(&mut reader)?;
        let story_paths = parse_story_paths(&mut archive);

        let meta = DocumentMeta {
            source_file: filename.to_string(),
            source_lang: options.source_lang.clone(),
            target_lang: options.target_lang.clone(),
            format: DocumentFormat::Idml,
            encoding: "UTF-8".to_string(),
        };

        let mut doc = DocumentIR::new(meta);
        let mut seq = 1;

        for story_path in story_paths {
            let Some(xml_content) = read_zip_entry_to_string(&mut archive, &story_path) else {
                continue;
            };

            let story_name = story_path
                .rsplit('/')
                .next()
                .and_then(|s| s.strip_suffix(".xml"))
                .unwrap_or(&story_path);

            extract_story_paragraphs(
                &xml_content,
                story_name,
                options.minimize_tags,
                &mut doc,
                &mut seq,
            )?;
        }

        Ok(doc)
    }
}

impl DocumentMerger for IdmlMerger {
    /// 翻訳結果を元の IDML ファイル（スケルトン）にマージします。
    /// レイアウト情報（Spreads）、スタイル定義、画像リンク等は 100% 保持し、
    /// 各 `Stories/Story_*.xml` 内の `<Content>` 要素のみを翻訳文へと安全に差し替えます。
    fn merge<R: Read + Seek, W: Write + Seek>(
        &self,
        mut skeleton: R,
        translations: &HashMap<usize, TranslatedUnit>,
        writer: W,
        _options: &MergeOptions,
    ) -> Result<(), FormatError> {
        let mut archive = ZipArchive::new(&mut skeleton)?;
        let story_paths = parse_story_paths(&mut archive);
        let mut replaced_entries: HashMap<String, Vec<u8>> = HashMap::new();
        let mut seq = 1;

        for story_path in story_paths {
            let Some(xml_content) = read_zip_entry_to_string(&mut archive, &story_path) else {
                continue;
            };

            let rewritten = rewrite_story_xml(&xml_content, translations, &mut seq)?;
            replaced_entries.insert(story_path, rewritten);
        }

        skeleton.seek(std::io::SeekFrom::Start(0))?;
        rewrite_zip(skeleton, writer, &replaced_entries)?;

        Ok(())
    }
}

/// `designmap.xml` を参照し、IDML 内で宣言されている Story ファイルパス一覧を取得します。
/// `<Document StoryList="...">` 属性に定義されている論理的な読み順（見出し→本文）に従ってソートします。
fn parse_story_paths<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Vec<String> {
    let mut story_paths = Vec::new();

    if let Some(designmap) = read_zip_entry_to_string(archive, "designmap.xml") {
        let mut reader = Reader::from_str(&designmap);
        let mut buf = Vec::new();
        let mut story_list_order: Vec<String> = Vec::new();
        let mut declared_stories: Vec<String> = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let name = e.name();
                    if name.as_ref() == b"Document" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"StoryList" {
                                let val = String::from_utf8_lossy(&attr.value);
                                for id in val.split_whitespace() {
                                    story_list_order.push(format!("Stories/Story_{}.xml", id));
                                }
                            }
                        }
                    } else if name.as_ref() == b"idPkg:Story" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"src" {
                                let src = String::from_utf8_lossy(&attr.value).to_string();
                                declared_stories.push(src);
                            }
                        }
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }

        // StoryList の優先順序に従って配置
        for path in story_list_order {
            if declared_stories.contains(&path) && !story_paths.contains(&path) {
                story_paths.push(path);
            }
        }

        // その他のストーリーを追加
        for path in declared_stories {
            if !story_paths.contains(&path) {
                story_paths.push(path);
            }
        }
    }

    if story_paths.is_empty() {
        for i in 0..archive.len() {
            if let Ok(file) = archive.by_index(i) {
                let name = file.name().to_string();
                if name.starts_with("Stories/Story_") && name.ends_with(".xml") {
                    story_paths.push(name);
                }
            }
        }
        story_paths.sort();
    }

    story_paths
}

/// `<CharacterStyleRange>` から有意なスタイル署名（太字、斜体、文字色等）を抽出します。
fn extract_char_style_sig(start: &BytesStart) -> (String, Option<String>) {
    let mut sigs = Vec::new();
    let mut raw_attrs = Vec::new();

    for attr in start.attributes().flatten() {
        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
        let val = String::from_utf8_lossy(&attr.value).to_string();

        raw_attrs.push(format!("{}=\"{}\"", key, val));

        if key == "FontStyle" || key == "AppliedCharacterStyle" {
            let lower = val.to_lowercase();
            if lower.contains("bold") {
                sigs.push("bold".to_string());
            }
            if lower.contains("italic") {
                sigs.push("italic".to_string());
            }
        } else if key == "Underline" && val == "true" {
            sigs.push("underline".to_string());
        } else if key == "StrikeThru" && val == "true" {
            sigs.push("strike".to_string());
        } else if key == "FillColor"
            && !val.contains("Black")
            && !val.contains("None")
            && !val.contains("Paper")
        {
            sigs.push(format!("color:{}", val));
        }
    }

    sigs.sort();
    let raw = if raw_attrs.is_empty() {
        None
    } else {
        Some(format!("<CharacterStyleRange {}/>", raw_attrs.join(" ")))
    };
    (sigs.join(";"), raw)
}

/// 単一の Story XML から段落およびテキストランを解析して `DocumentIR` に追加します。
fn extract_story_paragraphs(
    xml_content: &str,
    story_name: &str,
    minimize_tags: bool,
    doc: &mut DocumentIR,
    seq: &mut usize,
) -> Result<(), FormatError> {
    let mut reader = Reader::from_str(xml_content);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut current_runs: Vec<ProcessedRun> = Vec::new();
    let mut current_style_sig = String::new();
    let mut current_raw_rpr: Option<String> = None;
    let mut in_content = false;
    let mut current_content = String::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(ref e) => {
                let name = e.name();
                if name.as_ref() == b"CharacterStyleRange" {
                    let (sig, raw) = extract_char_style_sig(e);
                    current_style_sig = sig;
                    current_raw_rpr = raw;
                } else if name.as_ref() == b"Content" {
                    in_content = true;
                    current_content.clear();
                }
            }
            Event::Text(ref e) => {
                if in_content {
                    if let Ok(unescaped) = e.unescape() {
                        current_content.push_str(&unescaped);
                    }
                }
            }
            Event::End(ref e) => {
                let name = e.name();
                if name.as_ref() == b"Content" {
                    in_content = false;
                    if !current_content.is_empty() {
                        current_runs.push(ProcessedRun {
                            text: current_content.clone(),
                            style_sig: current_style_sig.clone(),
                            raw_rpr: current_raw_rpr.clone(),
                        });
                    }
                } else if name.as_ref() == b"ParagraphStyleRange" {
                    finalize_paragraph(&mut current_runs, story_name, minimize_tags, doc, seq);
                }
            }
            Event::Empty(ref e) => {
                let name = e.name();
                if name.as_ref() == b"Br" {
                    // InDesign の改行・段落区切り
                    finalize_paragraph(&mut current_runs, story_name, minimize_tags, doc, seq);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    finalize_paragraph(&mut current_runs, story_name, minimize_tags, doc, seq);
    Ok(())
}

fn finalize_paragraph(
    runs: &mut Vec<ProcessedRun>,
    story_name: &str,
    minimize_tags: bool,
    doc: &mut DocumentIR,
    seq: &mut usize,
) {
    let full_text: String = runs.iter().map(|r| r.text.as_str()).collect();
    if full_text.trim().is_empty() {
        runs.clear();
        return;
    }

    let coalesced = if minimize_tags {
        coalesce_runs(std::mem::take(runs))
    } else {
        std::mem::take(runs)
    };

    let (norm_text, placeholders) = runs_to_normalized_text(&coalesced, minimize_tags);
    if norm_text.trim().is_empty() {
        return;
    }

    let mut unit = UnitIR::new(format!("{}_{}", story_name, *seq), *seq, norm_text);
    unit.placeholders = placeholders;
    unit.context.style_name = Some(story_name.to_string());
    unit.populate_note_from_context(Some(&format!("Story: {}", story_name)));
    doc.add_unit(unit);
    *seq += 1;
}

/// 単一の Story XML を走査し、翻訳対象段落の `<Content>` を確定訳文へ差し替えます。
fn rewrite_story_xml(
    xml_content: &str,
    translations: &HashMap<usize, TranslatedUnit>,
    seq: &mut usize,
) -> Result<Vec<u8>, FormatError> {
    let mut reader = Reader::from_str(xml_content);
    reader.config_mut().trim_text(false);
    let mut out = Vec::with_capacity(xml_content.len());
    let mut writer = Writer::new(&mut out);

    let mut buf = Vec::new();
    let mut has_written_content = false;
    let mut in_content = false;

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(ref e) if e.name().as_ref() == b"Content" => {
                in_content = true;
                if !has_written_content {
                    // 段落の先頭 Content 要素
                    if let Some(trans) = translations.get(seq) {
                        let text = strip_placeholders(&trans.target);
                        let final_text = if text.is_empty() {
                            // 訳文が空の場合は原文をプレースホルダー除去して出力
                            strip_placeholders(&trans.target)
                        } else {
                            text
                        };

                        let mut new_content = BytesStart::new("Content");
                        new_content.push_attribute(("xml:space", "preserve"));
                        writer.write_event(Event::Start(new_content))?;
                        writer.write_event(Event::Text(BytesText::new(&final_text)))?;
                        writer.write_event(Event::End(BytesEnd::new("Content")))?;
                        has_written_content = true;
                    } else {
                        // 翻訳データが見つからない場合は原文をそのまま維持
                        writer.write_event(Event::Start(e.clone()))?;
                    }
                } else {
                    // 同一段落内の後続 Content 要素: 空要素を出力
                    let mut empty_content = BytesStart::new("Content");
                    empty_content.push_attribute(("xml:space", "preserve"));
                    writer.write_event(Event::Start(empty_content))?;
                    writer.write_event(Event::End(BytesEnd::new("Content")))?;
                }
            }
            Event::Text(ref e) => {
                if in_content {
                    if !has_written_content {
                        writer.write_event(Event::Text(e.clone()))?;
                    }
                } else {
                    writer.write_event(Event::Text(e.clone()))?;
                }
            }
            Event::End(ref e) => {
                let name = e.name();
                if name.as_ref() == b"Content" {
                    in_content = false;
                    if !has_written_content {
                        writer.write_event(Event::End(e.clone()))?;
                    }
                } else if name.as_ref() == b"ParagraphStyleRange" {
                    if has_written_content {
                        *seq += 1;
                        has_written_content = false;
                    }
                    writer.write_event(Event::End(e.clone()))?;
                } else {
                    writer.write_event(Event::End(e.clone()))?;
                }
            }
            Event::Empty(ref e) => {
                let name = e.name();
                if name.as_ref() == b"Br" {
                    if has_written_content {
                        *seq += 1;
                        has_written_content = false;
                    }
                }
                writer.write_event(Event::Empty(e.clone()))?;
            }
            Event::Eof => break,
            other => {
                writer.write_event(other)?;
            }
        }
        buf.clear();
    }

    if has_written_content {
        *seq += 1;
    }

    Ok(out)
}

/// テキスト内の `{@0}`, `{@1}` などのプレースホルダー記号を除去し、純粋な表示テキストを返します。
/// InDesign の `<Content>` 要素は装飾タグを受け付けず、内部文字列がそのまま画面に描画されるため、
/// プレースホルダー記号がテキストとして漏出するのを防止します。
fn strip_placeholders(text: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_extract_and_merge_idml() {
        let sample_idml_path = "test_data/2articles-1photo.idml";
        if !std::path::Path::new(sample_idml_path).exists() {
            return;
        }

        let file = std::fs::File::open(sample_idml_path).unwrap();
        let options = FilterOptions {
            source_lang: "en".to_string(),
            target_lang: Some("ja".to_string()),
            minimize_tags: true,
            ..Default::default()
        };

        let filter = IdmlFilter;
        let doc_ir = filter.extract(file, "2articles-1photo.idml", &options).unwrap();
        assert_eq!(doc_ir.units.len(), 5);
        assert!(doc_ir.units.iter().any(|u| u.source.contains("THE HEADLINE HERE")));

        // Test merge
        let mut translations = HashMap::new();
        for u in &doc_ir.units {
            translations.insert(
                u.seq,
                TranslatedUnit::new(u.seq, format!("【翻訳】{}", u.source)),
            );
        }

        let skeleton_file = std::fs::File::open(sample_idml_path).unwrap();
        let mut out_buffer = Cursor::new(Vec::new());
        let merger = IdmlMerger;
        merger
            .merge(skeleton_file, &translations, &mut out_buffer, &MergeOptions::default())
            .unwrap();

        // Re-extract merged archive
        out_buffer.set_position(0);
        let re_extracted = filter.extract(out_buffer, "merged.idml", &options).unwrap();
        assert_eq!(re_extracted.units.len(), 5);
        assert!(re_extracted.units.iter().any(|u| u.source.contains("【翻訳】THE HEADLINE HERE")));
    }
}
