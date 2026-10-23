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

pub struct DocxFilter;
pub struct DocxMerger;

impl DocumentFilter for DocxFilter {
    /// Word 文書 (`.docx`) の `word/document.xml` から段落テキストを抽出します。
    ///
    /// ### 処理概要
    /// 1. PKZip アーカイブから `word/document.xml` を読み込み。
    /// 2. `<w:p>`（段落）ごとにスタイルおよび `<w:r>`（ラン）の列を収集。
    /// 3. 不要なノイズ属性を無視した上で同一スタイルの隣接ランを結合 (`coalesce_runs`)。
    /// 4. スタイル変化がある箇所に `{0}`, `{1}` のインラインプレースホルダーを付与。
    /// 5. 見出しや段落のスタイル名を `context.style_name`、行・段落番号を `note` に記録。
    fn extract<R: Read + Seek>(
        &self,
        mut reader: R,
        filename: &str,
        options: &FilterOptions,
    ) -> Result<DocumentIR, FormatError> {
        let mut archive = ZipArchive::new(&mut reader)?;
        let doc_xml = read_zip_entry_to_string(&mut archive, "word/document.xml").ok_or_else(
            || FormatError::Parse("word/document.xml not found in docx archive".to_string()),
        )?;

        let meta = DocumentMeta {
            source_file: filename.to_string(),
            source_lang: options.source_lang.clone(),
            target_lang: options.target_lang.clone(),
            format: DocumentFormat::Docx,
            encoding: "UTF-8".to_string(),
        };

        let mut doc = DocumentIR::new(meta);
        let mut seq = 1;

        let mut xml_reader = Reader::from_str(&doc_xml);
        xml_reader.config_mut().trim_text(false);
        let mut buf = Vec::new();

        let mut in_p = false;
        let mut in_r = false;
        let mut in_t = false;
        let mut in_rpr = false;
        let mut current_style: Option<String> = None;
        let mut current_runs: Vec<ProcessedRun> = Vec::new();
        let mut current_run_text = String::new();
        let mut current_rpr_xml = String::new();

        loop {
            match xml_reader.read_event_into(&mut buf)? {
                Event::Start(ref e) => {
                    let name = e.name();
                    if name.as_ref() == b"w:p" {
                        in_p = true;
                        current_style = None;
                        current_runs.clear();
                    } else if in_p && name.as_ref() == b"w:pStyle" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"w:val" {
                                current_style = Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    } else if in_p && name.as_ref() == b"w:r" {
                        in_r = true;
                        current_run_text.clear();
                        current_rpr_xml.clear();
                    } else if in_r && name.as_ref() == b"w:rPr" {
                        in_rpr = true;
                        current_rpr_xml.push_str("<w:rPr>");
                    } else if in_r && name.as_ref() == b"w:t" {
                        in_t = true;
                    } else if in_rpr {
                        // Accumulate rPr tags
                        let name = e.name();
                        let tag_str = String::from_utf8_lossy(name.as_ref());
                        current_rpr_xml.push_str(&format!("<{}>", tag_str));
                    }
                }
                Event::Empty(ref e) => {
                    if in_p && e.name().as_ref() == b"w:pStyle" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"w:val" {
                                current_style = Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    } else if in_rpr {
                        let mut tag_str = String::from_utf8_lossy(e.name().as_ref()).to_string();
                        let attrs: Vec<String> = e
                            .attributes()
                            .flatten()
                            .map(|a| {
                                format!(
                                    "{}=\"{}\"",
                                    String::from_utf8_lossy(a.key.as_ref()),
                                    String::from_utf8_lossy(&a.value)
                                )
                            })
                            .collect();
                        if !attrs.is_empty() {
                            tag_str.push(' ');
                            tag_str.push_str(&attrs.join(" "));
                        }
                        current_rpr_xml.push_str(&format!("<{}/>", tag_str));
                    }
                }
                Event::Text(ref e) => {
                    if in_t {
                        current_run_text.push_str(&e.unescape()?);
                    }
                }
                Event::End(ref e) => {
                    let name = e.name();
                    if name.as_ref() == b"w:t" {
                        in_t = false;
                    } else if name.as_ref() == b"w:rPr" {
                        in_rpr = false;
                        current_rpr_xml.push_str("</w:rPr>");
                    } else if name.as_ref() == b"w:r" {
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
                    } else if name.as_ref() == b"w:p" {
                        in_p = false;
                        let coalesced = if options.minimize_tags {
                            coalesce_runs(current_runs.clone())
                        } else {
                            current_runs.clone()
                        };

                        let (normalized, placeholders) =
                            runs_to_normalized_text(&coalesced, options.minimize_tags);

                        if !normalized.trim().is_empty() {
                            let mut unit = UnitIR::new(format!("p_{}", seq), seq, normalized);
                            unit.placeholders = placeholders;
                            unit.context.style_name = current_style.clone();
                            unit.populate_note_from_context(None);
                            doc.add_unit(unit);
                            seq += 1;
                        }
                    }
                }
                Event::Eof => break,
                _ => {}
            }
            buf.clear();
        }

        Ok(doc)
    }
}

impl DocumentMerger for DocxMerger {
    /// 翻訳結果を元の Word 文書（スケルトン）にマージし、新しい `.docx` を出力します。
    /// 各段落のプロパティ（`<w:pPr>`）はそのまま保持し、テキスト領域のみを翻訳文に安全に置き換えます。
    fn merge<R: Read + Seek, W: Write + Seek>(
        &self,
        mut skeleton: R,
        translations: &HashMap<usize, TranslatedUnit>,
        writer: W,
        _options: &MergeOptions,
    ) -> Result<(), FormatError> {
        let mut archive = ZipArchive::new(&mut skeleton)?;
        let doc_xml = read_zip_entry_to_string(&mut archive, "word/document.xml").ok_or_else(
            || FormatError::Parse("word/document.xml not found in docx archive".to_string()),
        )?;

        let mut replaced_entries: HashMap<String, Vec<u8>> = HashMap::new();
        let rewritten_doc = rewrite_document_xml(&doc_xml, translations)?;
        replaced_entries.insert("word/document.xml".to_string(), rewritten_doc);

        skeleton.seek(std::io::SeekFrom::Start(0))?;
        rewrite_zip(skeleton, writer, &replaced_entries)?;

        Ok(())
    }
}

/// Rewrites `word/document.xml` by replacing translatable `<w:p>` contents with translated text.
fn rewrite_document_xml(
    xml_content: &str,
    translations: &HashMap<usize, TranslatedUnit>,
) -> Result<Vec<u8>, FormatError> {
    let mut reader = Reader::from_str(xml_content);
    reader.config_mut().trim_text(false);
    let mut out = Vec::with_capacity(xml_content.len());
    let mut writer = Writer::new(&mut out);

    let mut buf = Vec::new();
    let mut seq = 1;
    let mut in_p = false;
    let mut p_has_text = false;
    let mut p_events: Vec<Event<'static>> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(ref e) if e.name().as_ref() == b"w:p" => {
                in_p = true;
                p_has_text = false;
                p_events.clear();
                p_events.push(Event::Start(e.clone().into_owned()));
            }
            Event::End(ref e) if in_p && e.name().as_ref() == b"w:p" => {
                in_p = false;
                p_events.push(Event::End(e.clone().into_owned()));

                if p_has_text {
                    if let Some(trans) = translations.get(&seq) {
                        write_merged_paragraph(&mut writer, &p_events, trans)?;
                    } else {
                        for ev in p_events.drain(..) {
                            writer.write_event(ev)?;
                        }
                    }
                    seq += 1;
                } else {
                    for ev in p_events.drain(..) {
                        writer.write_event(ev)?;
                    }
                }
            }
            Event::Start(ref e) if in_p && e.name().as_ref() == b"w:t" => {
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

/// Writes a merged paragraph: keeps `<w:pPr>` and non-text elements, and replaces text runs with translation.
fn write_merged_paragraph<W: Write>(
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
                if e.name().as_ref() == b"w:pPr" {
                    in_p_pr = true;
                    writer.write_event(ev.clone())?;
                } else if in_p_pr {
                    writer.write_event(ev.clone())?;
                } else if e.name().as_ref() == b"w:r" {
                    // Suppress original runs
                } else {
                    writer.write_event(ev.clone())?;
                }
            }
            Event::End(ref e) => {
                if e.name().as_ref() == b"w:pPr" {
                    in_p_pr = false;
                    writer.write_event(ev.clone())?;
                } else if in_p_pr {
                    writer.write_event(ev.clone())?;
                } else if e.name().as_ref() == b"w:p" {
                    if !wrote_translation {
                        // Write translation run before </w:p>
                        writer.write_event(Event::Start(BytesStart::new("w:r")))?;
                        let mut t_start = BytesStart::new("w:t");
                        t_start.push_attribute(("xml:space", "preserve"));
                        writer.write_event(Event::Start(t_start))?;
                        writer.write_event(Event::Text(BytesText::new(&restored_text)))?;
                        writer.write_event(Event::End(BytesEnd::new("w:t")))?;
                        writer.write_event(Event::End(BytesEnd::new("w:r")))?;
                        wrote_translation = true;
                    }
                    writer.write_event(ev.clone())?;
                } else if e.name().as_ref() == b"w:r" {
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
    fn test_extract_device_guide_docx() {
        let test_path = Path::new("../../test_data/デバイスガイド.docx");
        if !test_path.exists() {
            eprintln!("Skipping test: test_data/デバイスガイド.docx not found");
            return;
        }

        let file = File::open(test_path).expect("Failed to open デバイスガイド.docx");
        let filter = DocxFilter;
        let opts = FilterOptions {
            source_lang: "ja".to_string(),
            target_lang: Some("en".to_string()),
            minimize_tags: true,
            ..Default::default()
        };

        let doc = filter.extract(file, "デバイスガイド.docx", &opts).unwrap();
        assert!(!doc.units.is_empty(), "Units should not be empty");

        // Verify that tag soup in Paragraph 3 was minimized into clean text with color placeholder
        let p3 = &doc.units[2];
        println!("Paragraph 3 source: {}", p3.source);
        assert!(p3.source.contains("保守作业を開始する前に、デバイスA1の电源が入っていることを"));

        // Test merge
        let mut trans = HashMap::new();
        trans.insert(1, TranslatedUnit::new(1, "System Maintenance Guide"));

        let merger = DocxMerger;
        let skeleton_file = File::open(test_path).unwrap();
        let mut merged_out = Cursor::new(Vec::new());

        merger
            .merge(skeleton_file, &trans, &mut merged_out, &MergeOptions::default())
            .unwrap();

        let mut merged_archive = ZipArchive::new(&mut merged_out).unwrap();
        let doc_xml = read_zip_entry_to_string(&mut merged_archive, "word/document.xml").unwrap();
        assert!(doc_xml.contains("System Maintenance Guide"));
    }
}
