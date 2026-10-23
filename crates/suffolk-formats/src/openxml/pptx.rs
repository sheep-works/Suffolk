use crate::openxml::minimizer::{
    coalesce_runs, extract_style_signature, runs_to_normalized_text, ProcessedRun,
};
use crate::openxml::zip_handler::{read_zip_entry_to_string, rewrite_zip};
use crate::traits::{DocumentFilter, DocumentMerger, FilterOptions, FormatError, MergeOptions};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use suffolk_codec::TranslatedUnit;
use suffolk_ir::{
    restore_placeholders, string_map_to_tags, DocumentFormat, DocumentIR, DocumentMeta, UnitIR,
};
use zip::ZipArchive;

pub struct PptxFilter;
pub struct PptxMerger;

/// Zip内のファイルパス（例: "ppt/slides/slide3.xml" や "ppt/notesSlides/notesSlide3.xml"）から
/// スライド番号（3）を数値として抽出します。
/// ディレクトリ名 "slides" の文字列に誤マッチしないよう、ファイル名部分を抽出してから解析します。
pub fn parse_slide_number(entry_name: &str) -> Option<u32> {
    let file_name = entry_name.rsplit('/').next().unwrap_or(entry_name).to_lowercase();
    if let Some(pos) = file_name.find("slide") {
        let after = &file_name[pos + "slide".len()..];
        let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        return num_str.parse::<u32>().ok();
    }
    None
}

impl DocumentFilter for PptxFilter {
    /// PowerPoint 文書 (`.pptx`) から全スライドおよびスピーカーノートのテキストを抽出します。
    ///
    /// ### 処理概要
    /// 1. アーカイブ内の `ppt/slides/slide*.xml` および `ppt/notesSlides/notesSlide*.xml` を収集。
    /// 2. スライド番号順にソートして一貫したセグメント順序を保証。
    /// 3. DrawingML (`<a:p>`, `<a:r>`) のテキストおよびスタイルを抽出・結合。
    /// 4. ユニットの `note` に `[Slide 1]` や `[Slide 1 Speaker Notes]` を明記し、翻訳者が
    ///    スライド本文かスピーカーノート（発表者用メモ）かを判別できるようにします。
    fn extract<R: Read + Seek>(
        &self,
        mut reader: R,
        filename: &str,
        options: &FilterOptions,
    ) -> Result<DocumentIR, FormatError> {
        let mut archive = ZipArchive::new(&mut reader)?;
        let mut slide_entries: Vec<String> = Vec::new();
        let mut notes_entries: Vec<String> = Vec::new();

        for i in 0..archive.len() {
            if let Ok(file) = archive.by_index(i) {
                let name = file.name().to_string();
                if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                    slide_entries.push(name);
                } else if name.starts_with("ppt/notesSlides/notesSlide") && name.ends_with(".xml") {
                    notes_entries.push(name);
                }
            }
        }

        // Sort slides numerically
        slide_entries.sort_by_key(|name| parse_slide_number(name).unwrap_or(0));
        notes_entries.sort_by_key(|name| parse_slide_number(name).unwrap_or(0));

        let meta = DocumentMeta {
            source_file: filename.to_string(),
            source_lang: options.source_lang.clone(),
            target_lang: options.target_lang.clone(),
            format: DocumentFormat::Pptx,
            encoding: "UTF-8".to_string(),
        };

        let mut doc = DocumentIR::new(meta);
        let mut seq = 1;

        // Extract slides
        for entry_name in slide_entries {
            let slide_num = parse_slide_number(&entry_name).unwrap_or(1);
            let Some(xml) = read_zip_entry_to_string(&mut archive, &entry_name) else {
                continue;
            };

            extract_paragraphs_from_drawing_xml(
                &xml,
                slide_num,
                false,
                &entry_name,
                options.minimize_tags,
                &mut doc,
                &mut seq,
            )?;
        }

        // Extract speaker notes
        for entry_name in notes_entries {
            let slide_num = parse_slide_number(&entry_name).unwrap_or(1);
            let Some(xml) = read_zip_entry_to_string(&mut archive, &entry_name) else {
                continue;
            };

            extract_paragraphs_from_drawing_xml(
                &xml,
                slide_num,
                true,
                &entry_name,
                options.minimize_tags,
                &mut doc,
                &mut seq,
            )?;
        }

        Ok(doc)
    }
}

/// Helper that extracts `<a:p>` paragraphs from PowerPoint slide or notes XML.
fn extract_paragraphs_from_drawing_xml(
    xml_content: &str,
    slide_num: u32,
    is_notes: bool,
    entry_name: &str,
    minimize_tags: bool,
    doc: &mut DocumentIR,
    seq: &mut usize,
) -> Result<(), FormatError> {
    let mut reader = Reader::from_str(xml_content);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut current_shape_name: Option<String> = None;
    let mut in_p = false;
    let mut in_r = false;
    let mut in_t = false;
    let mut current_runs: Vec<ProcessedRun> = Vec::new();
    let mut current_run_text = String::new();
    let mut current_rpr_xml = String::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(ref e) => {
                let name = e.name();
                if name.as_ref() == b"p:cNvPr" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"name" {
                            current_shape_name =
                                Some(String::from_utf8_lossy(&attr.value).to_string());
                        }
                    }
                } else if name.as_ref() == b"a:p" {
                    in_p = true;
                    current_runs.clear();
                } else if in_p && name.as_ref() == b"a:r" {
                    in_r = true;
                    current_run_text.clear();
                    current_rpr_xml.clear();
                } else if in_r && name.as_ref() == b"a:rPr" {
                    current_rpr_xml.push_str("<a:rPr>");
                } else if in_r && name.as_ref() == b"a:t" {
                    in_t = true;
                }
            }
            Event::Empty(ref e) => {
                if e.name().as_ref() == b"p:cNvPr" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"name" {
                            current_shape_name =
                                Some(String::from_utf8_lossy(&attr.value).to_string());
                        }
                    }
                }
            }
            Event::Text(ref e) => {
                if in_t {
                    current_run_text.push_str(&e.unescape()?);
                }
            }
            Event::End(ref e) => {
                let name = e.name();
                if name.as_ref() == b"a:t" {
                    in_t = false;
                } else if name.as_ref() == b"a:r" {
                    in_r = false;
                    if !current_run_text.is_empty() {
                        let sig = extract_style_signature(&current_rpr_xml);
                        current_runs.push(ProcessedRun {
                            text: current_run_text.clone(),
                            style_sig: sig,
                            raw_rpr: if current_rpr_xml.is_empty() {
                                None
                            } else {
                                Some(current_rpr_xml.clone())
                            },
                        });
                    }
                } else if name.as_ref() == b"a:p" {
                    in_p = false;
                    let coalesced = if minimize_tags {
                        coalesce_runs(current_runs.clone())
                    } else {
                        current_runs.clone()
                    };

                    let (normalized, placeholders) =
                        runs_to_normalized_text(&coalesced, minimize_tags);

                    if !normalized.trim().is_empty() {
                        let unit_id = format!("{}_p{}", entry_name, *seq);
                        let mut unit = UnitIR::new(unit_id, *seq, normalized);
                        unit.placeholders = placeholders;
                        unit.context.slide_number = Some(slide_num);
                        unit.context.shape_name = current_shape_name.clone();
                        unit.context.is_speaker_notes = is_notes;
                        unit.populate_note_from_context(None);

                        doc.add_unit(unit);
                        *seq += 1;
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(())
}

impl DocumentMerger for PptxMerger {
    /// 翻訳結果を元の PowerPoint プレゼンテーション（スケルトン）にマージします。
    /// 各スライドおよびスピーカーノート XML のテキスト部分のみを置換し、図形レイアウトやアニメーション設定を保護します。
    fn merge<R: Read + Seek, W: Write + Seek>(
        &self,
        mut skeleton: R,
        translations: &HashMap<usize, TranslatedUnit>,
        writer: W,
        _options: &MergeOptions,
    ) -> Result<(), FormatError> {
        let mut archive = ZipArchive::new(&mut skeleton)?;
        let mut replaced_entries: HashMap<String, Vec<u8>> = HashMap::new();

        let mut slide_entries: Vec<String> = Vec::new();
        for i in 0..archive.len() {
            if let Ok(file) = archive.by_index(i) {
                let name = file.name().to_string();
                if (name.starts_with("ppt/slides/slide") || name.starts_with("ppt/notesSlides/notesSlide"))
                    && name.ends_with(".xml")
                {
                    slide_entries.push(name);
                }
            }
        }

        slide_entries.sort_by_key(|name| {
            let is_notes = name.contains("notesSlide");
            let num = parse_slide_number(name).unwrap_or(0);
            (is_notes, num)
        });

        let mut current_seq = 1;
        for entry_name in slide_entries {
            let Some(xml) = read_zip_entry_to_string(&mut archive, &entry_name) else {
                continue;
            };

            let rewritten = rewrite_slide_xml(&xml, translations, &mut current_seq)?;
            replaced_entries.insert(entry_name, rewritten);
        }

        skeleton.seek(std::io::SeekFrom::Start(0))?;
        rewrite_zip(skeleton, writer, &replaced_entries)?;

        Ok(())
    }
}

/// Rewrites a slide XML by replacing translatable `<a:p>` runs with translations.
fn rewrite_slide_xml(
    xml_content: &str,
    translations: &HashMap<usize, TranslatedUnit>,
    seq: &mut usize,
) -> Result<Vec<u8>, FormatError> {
    let mut reader = Reader::from_str(xml_content);
    reader.config_mut().trim_text(false);
    let mut out = Vec::with_capacity(xml_content.len());
    let mut writer = Writer::new(&mut out);

    let mut buf = Vec::new();
    let mut in_p = false;
    let mut p_has_text = false;
    let mut p_events: Vec<Event<'static>> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(ref e) if e.name().as_ref() == b"a:p" => {
                in_p = true;
                p_has_text = false;
                p_events.clear();
                p_events.push(Event::Start(e.clone().into_owned()));
            }
            Event::End(ref e) if in_p && e.name().as_ref() == b"a:p" => {
                in_p = false;
                p_events.push(Event::End(e.clone().into_owned()));

                if p_has_text {
                    if let Some(trans) = translations.get(seq) {
                        write_merged_slide_paragraph(&mut writer, &p_events, trans)?;
                    } else {
                        for ev in p_events.drain(..) {
                            writer.write_event(ev)?;
                        }
                    }
                    *seq += 1;
                } else {
                    for ev in p_events.drain(..) {
                        writer.write_event(ev)?;
                    }
                }
            }
            Event::Start(ref e) if in_p && e.name().as_ref() == b"a:t" => {
                p_has_text = true;
                p_events.push(Event::Start(e.clone().into_owned()));
            }
            Event::Eof => break,
            other => {
                if in_p {
                    p_events.push(other.into_owned());
                } else {
                    writer.write_event(other)?;
                }
            }
        }
        buf.clear();
    }

    Ok(out)
}

/// Writes merged PowerPoint paragraph `<a:p>`: keeps `<a:pPr>`, replaces text runs.
fn write_merged_slide_paragraph<W: Write>(
    writer: &mut Writer<W>,
    events: &[Event<'static>],
    trans: &TranslatedUnit,
) -> Result<(), FormatError> {
    let tags = string_map_to_tags(&trans.placeholders);
    let restored_text = restore_placeholders(&trans.target, &tags);

    let mut in_p_pr = false;
    let mut wrote_translation = false;

    for ev in events {
        match ev {
            Event::Start(ref e) => {
                if e.name().as_ref() == b"a:pPr" {
                    in_p_pr = true;
                    writer.write_event(ev.clone())?;
                } else if in_p_pr {
                    writer.write_event(ev.clone())?;
                } else if e.name().as_ref() == b"a:r" {
                    // Suppress original runs
                } else {
                    writer.write_event(ev.clone())?;
                }
            }
            Event::End(ref e) => {
                if e.name().as_ref() == b"a:pPr" {
                    in_p_pr = false;
                    writer.write_event(ev.clone())?;
                } else if in_p_pr {
                    writer.write_event(ev.clone())?;
                } else if e.name().as_ref() == b"a:p" {
                    if !wrote_translation {
                        writer.write_event(Event::Start(BytesStart::new("a:r")))?;
                        writer.write_event(Event::Start(BytesStart::new("a:t")))?;
                        writer.write_event(Event::Text(BytesText::new(&restored_text)))?;
                        writer.write_event(Event::End(BytesEnd::new("a:t")))?;
                        writer.write_event(Event::End(BytesEnd::new("a:r")))?;
                        wrote_translation = true;
                    }
                    writer.write_event(ev.clone())?;
                } else if e.name().as_ref() == b"a:r" {
                    // Suppress original runs
                } else {
                    writer.write_event(ev.clone())?;
                }
            }
            Event::Text(_) => {
                if in_p_pr {
                    writer.write_event(ev.clone())?;
                }
            }
            other => {
                if in_p_pr || !matches!(other, Event::Start(_) | Event::End(_)) {
                    writer.write_event(other.clone())?;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Cursor;
    use std::path::Path;

    #[test]
    fn test_extract_troubleshooting_pptx() {
        let test_path = Path::new("../../test_data/PCお困りごと共有＆解決勉強会.pptx");
        if !test_path.exists() {
            eprintln!("Skipping test: test_data/PCお困りごと共有＆解決勉強会.pptx not found");
            return;
        }

        let file = File::open(test_path).expect("Failed to open pptx");
        let filter = PptxFilter;
        let opts = FilterOptions {
            source_lang: "ja".to_string(),
            target_lang: Some("en".to_string()),
            minimize_tags: true,
            ..Default::default()
        };

        let doc = filter.extract(file, "test.pptx", &opts).unwrap();
        assert!(!doc.units.is_empty(), "Units should not be empty");

        let has_slide1 = doc.units.iter().any(|u| u.note.as_deref().unwrap_or("").contains("Slide: 1"));
        let has_slide2 = doc.units.iter().any(|u| u.note.as_deref().unwrap_or("").contains("Slide: 2"));
        let has_slide3 = doc.units.iter().any(|u| u.note.as_deref().unwrap_or("").contains("Slide: 3"));
        let has_slide4 = doc.units.iter().any(|u| u.note.as_deref().unwrap_or("").contains("Slide: 4"));
        assert!(has_slide1, "Should have units from Slide 1");
        assert!(has_slide2, "Should have units from Slide 2");
        assert!(has_slide3, "Should have units from Slide 3");
        assert!(has_slide4, "Should have units from Slide 4");

        // Test merge
        let mut trans = HashMap::new();
        trans.insert(1, TranslatedUnit::new(1, "PC Troubleshooting Workshop"));

        let merger = PptxMerger;
        let skeleton_file = File::open(test_path).unwrap();
        let mut merged_out = Cursor::new(Vec::new());

        merger
            .merge(skeleton_file, &trans, &mut merged_out, &MergeOptions::default())
            .unwrap();

        let mut merged_archive = ZipArchive::new(&mut merged_out).unwrap();
        let slide1_xml = read_zip_entry_to_string(&mut merged_archive, "ppt/slides/slide1.xml").unwrap();
        assert!(slide1_xml.contains("PC Troubleshooting Workshop"));
    }
}
