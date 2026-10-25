use crate::openxml::minimizer::{
    coalesce_runs, extract_style_signature, runs_to_normalized_text, ProcessedRun,
};
use crate::openxml::zip_handler::{read_zip_entry_to_string, rewrite_zip};
use crate::traits::{DocumentFilter, DocumentMerger, FilterOptions, FormatError, MergeOptions};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use regex::Regex;
use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use suffolk_codec::TranslatedUnit;
use suffolk_ir::{DocumentFormat, DocumentIR, DocumentMeta, UnitIR};
use zip::ZipArchive;

pub struct DocxFilter;
pub struct DocxMerger;

/// 段落の抽出時コンテキスト
struct ParagraphFrame {
    style: Option<String>,
    runs: Vec<ProcessedRun>,
}

impl DocumentFilter for DocxFilter {
    /// Word 文書 (`.docx`) の `word/document.xml` から段落テキストを抽出します。
    /// テキストボックスなどのネストした `<w:p>` もスタックで安全に抽出します。
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

        let mut p_stack: Vec<ParagraphFrame> = Vec::new();
        let mut in_r = false;
        let mut in_t = false;
        let mut in_rpr = false;
        let mut current_run_text = String::new();
        let mut current_rpr_xml = String::new();

        loop {
            match xml_reader.read_event_into(&mut buf)? {
                Event::Start(ref e) => {
                    let name = e.name();
                    if name.as_ref() == b"w:p" {
                        p_stack.push(ParagraphFrame {
                            style: None,
                            runs: Vec::new(),
                        });
                        in_r = false;
                        in_t = false;
                        in_rpr = false;
                        current_run_text.clear();
                        current_rpr_xml.clear();
                    } else if !p_stack.is_empty() && name.as_ref() == b"w:pStyle" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"w:val" {
                                if let Some(last) = p_stack.last_mut() {
                                    last.style = Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                            }
                        }
                    } else if !p_stack.is_empty() && name.as_ref() == b"w:r" {
                        in_r = true;
                        current_run_text.clear();
                        current_rpr_xml.clear();
                    } else if in_r && name.as_ref() == b"w:rPr" {
                        in_rpr = true;
                        current_rpr_xml.push_str("<w:rPr>");
                    } else if in_r && name.as_ref() == b"w:t" {
                        in_t = true;
                    } else if in_rpr {
                        let name_str = String::from_utf8_lossy(name.as_ref());
                        current_rpr_xml.push_str(&format!("<{}>", name_str));
                    }
                }
                Event::Empty(ref e) => {
                    if !p_stack.is_empty() && e.name().as_ref() == b"w:pStyle" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"w:val" {
                                if let Some(last) = p_stack.last_mut() {
                                    last.style = Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
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
                    if in_t && in_r && !p_stack.is_empty() {
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
                        if !current_run_text.is_empty() && !p_stack.is_empty() {
                            let sig = extract_style_signature(&current_rpr_xml);
                            p_stack.last_mut().unwrap().runs.push(ProcessedRun {
                                text: current_run_text.clone(),
                                style_sig: sig,
                                raw_rpr: if current_rpr_xml.is_empty() {
                                    None
                                } else {
                                    Some(current_rpr_xml.clone())
                                },
                            });
                            current_run_text.clear();
                            current_rpr_xml.clear();
                        }
                    } else if name.as_ref() == b"w:p" {
                        if let Some(frame) = p_stack.pop() {
                            let coalesced = if options.minimize_tags {
                                coalesce_runs(frame.runs)
                            } else {
                                frame.runs
                            };

                            let (normalized, placeholders) =
                                runs_to_normalized_text(&coalesced, options.minimize_tags);

                            if !normalized.trim().is_empty() {
                                let mut unit = UnitIR::new(format!("p_{}", seq), seq, normalized);
                                unit.placeholders = placeholders;
                                unit.context.style_name = frame.style;
                                unit.populate_note_from_context(None);
                                doc.add_unit(unit);
                                seq += 1;
                            }
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

/// 段落のバッファ構造体（マージ用）
struct ParagraphBuffer {
    events: Vec<Event<'static>>,
    has_text: bool,
}

/// Rewrites `word/document.xml` by replacing translatable `<w:p>` contents with translated text.
/// ネストした段落（テキストボックス等）も正確にスタック管理して XML の整合性を完全に保持します。
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
    let mut p_stack: Vec<ParagraphBuffer> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(ref e) if e.name().as_ref() == b"w:p" => {
                p_stack.push(ParagraphBuffer {
                    events: vec![Event::Start(e.clone().into_owned())],
                    has_text: false,
                });
            }
            Event::End(ref e) if e.name().as_ref() == b"w:p" && !p_stack.is_empty() => {
                let mut frame = p_stack.pop().unwrap();
                frame.events.push(Event::End(e.clone().into_owned()));

                let merged_events = if frame.has_text {
                    let res = if let Some(trans) = translations.get(&seq) {
                        build_merged_paragraph_events(&frame.events, trans)?
                    } else {
                        frame.events
                    };
                    seq += 1;
                    res
                } else {
                    frame.events
                };

                if let Some(parent) = p_stack.last_mut() {
                    // ネスト段落（テキストボックス等）は親段落のバッファにマージ結果を追加
                    parent.events.extend(merged_events);
                } else {
                    // 最上位段落は直接出力ストリームに書き込み
                    for ev in merged_events {
                        writer.write_event(ev)?;
                    }
                }
            }
            Event::Eof => break,
            other => {
                if let Some(frame) = p_stack.last_mut() {
                    if let Event::Start(ref e) = other {
                        if e.name().as_ref() == b"w:t" {
                            frame.has_text = true;
                        }
                    }
                    frame.events.push(other.into_owned());
                } else {
                    writer.write_event(other)?;
                }
            }
        }
        buf.clear();
    }

    Ok(out)
}

/// 単一の段落イベント列から、翻訳テキストを適用したマージ済みイベント列を生成します。
/// `<w:pPr>` や描画オブジェクト（`<w:pict>`, `<w:drawing>`）をそのまま保持し、
/// テキストランのみを正規の OpenXML ラン列（`<w:r>`）に置き換えます。
fn build_merged_paragraph_events(
    events: &[Event<'static>],
    trans: &TranslatedUnit,
) -> Result<Vec<Event<'static>>, FormatError> {
    let mut merged = Vec::with_capacity(events.len());
    let mut wrote_translation = false;

    // プレースホルダーを含む翻訳ランのバイト列を生成
    let mut run_bytes = Vec::new();
    {
        let mut run_writer = Writer::new(&mut run_bytes);
        write_translated_runs(&mut run_writer, &trans.target, &trans.placeholders)?;
    }

    let mut i = 0;
    while i < events.len() {
        let ev = &events[i];
        match ev {
            Event::Start(ref e) if e.name().as_ref() == b"w:p" => {
                merged.push(ev.clone());
                i += 1;
            }
            Event::Start(ref e) if e.name().as_ref() == b"w:pPr" => {
                // <w:pPr> から </w:pPr> までそのまま保持
                merged.push(ev.clone());
                i += 1;
                while i < events.len() {
                    let sub_ev = &events[i];
                    merged.push(sub_ev.clone());
                    if let Event::End(ref sub_e) = sub_ev {
                        if sub_e.name().as_ref() == b"w:pPr" {
                            i += 1;
                            break;
                        }
                    }
                    i += 1;
                }
                // <w:pPr> の直後に翻訳ランを注入
                if !wrote_translation {
                    inject_translated_run_events(&mut merged, &run_bytes)?;
                    wrote_translation = true;
                }
            }
            Event::Start(ref e) if e.name().as_ref() == b"w:r" => {
                // ラン <w:r> から </w:r> までを収集
                let mut run_events = vec![ev.clone()];
                let mut r_depth = 1;
                let mut has_graphic = false;
                i += 1;

                while i < events.len() && r_depth > 0 {
                    let sub_ev = &events[i];
                    if let Event::Start(ref sub_e) = sub_ev {
                        if sub_e.name().as_ref() == b"w:r" {
                            r_depth += 1;
                        } else if sub_e.name().as_ref() == b"w:pict" || sub_e.name().as_ref() == b"w:drawing" {
                            has_graphic = true;
                        }
                    } else if let Event::End(ref sub_e) = sub_ev {
                        if sub_e.name().as_ref() == b"w:r" {
                            r_depth -= 1;
                        }
                    }
                    run_events.push(sub_ev.clone());
                    i += 1;
                }

                if has_graphic {
                    // テキストボックスや画像を含むランはそのまま保持！
                    merged.extend(run_events);
                } else {
                    // 純粋なテキストランは除去し、翻訳ランに置き換え
                    if !wrote_translation {
                        inject_translated_run_events(&mut merged, &run_bytes)?;
                        wrote_translation = true;
                    }
                }
            }
            Event::End(ref e) if e.name().as_ref() == b"w:p" => {
                if !wrote_translation {
                    inject_translated_run_events(&mut merged, &run_bytes)?;
                    wrote_translation = true;
                }
                merged.push(ev.clone());
                i += 1;
            }
            _ => {
                // ブックマークやコメントなど、その他の要素はそのまま出力
                merged.push(ev.clone());
                i += 1;
            }
        }
    }

    Ok(merged)
}

/// 生XMLバイト列からパースしてイベント列に注入
fn inject_translated_run_events(
    target: &mut Vec<Event<'static>>,
    run_bytes: &[u8],
) -> Result<(), FormatError> {
    if run_bytes.is_empty() {
        return Ok(());
    }

    let mut reader = Reader::from_reader(run_bytes);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            ev => target.push(ev.into_owned()),
        }
        buf.clear();
    }

    Ok(())
}

/// 翻訳テキスト（プレースホルダー含む）を OpenXML の正規な `<w:r>` 列として書き出します。
fn write_translated_runs<W: Write>(
    writer: &mut Writer<W>,
    target_text: &str,
    placeholders: &HashMap<usize, String>,
) -> Result<(), FormatError> {
    if target_text.is_empty() {
        return Ok(());
    }

    let re = Regex::new(r"\{@?(\d+)\}").expect("Invalid placeholder regex");
    let mut last_end = 0;
    let mut current_rpr: Option<String> = None;

    for cap in re.captures_iter(target_text) {
        let mat = cap.get(0).unwrap();
        let chunk = &target_text[last_end..mat.start()];
        if !chunk.is_empty() {
            write_single_run(writer, chunk, current_rpr.as_deref())?;
        }

        let idx: usize = cap.get(1).unwrap().as_str().parse().unwrap_or(0);
        if let Some(rpr) = placeholders.get(&idx) {
            if rpr.trim().is_empty() {
                // {@1} 終了タグ: スタイルリセット
                current_rpr = None;
            } else {
                // {@0} 開始タグ: スタイル設定
                current_rpr = Some(rpr.clone());
            }
        }
        last_end = mat.end();
    }

    let remaining = &target_text[last_end..];
    if !remaining.is_empty() {
        write_single_run(writer, remaining, current_rpr.as_deref())?;
    }

    Ok(())
}

/// 単一のテキストラン（`<w:r>`）を書き出します。
fn write_single_run<W: Write>(
    writer: &mut Writer<W>,
    text: &str,
    rpr_xml: Option<&str>,
) -> Result<(), FormatError> {
    writer.write_event(Event::Start(BytesStart::new("w:r")))?;

    if let Some(rpr) = rpr_xml {
        if !rpr.trim().is_empty() {
            writer.get_mut().write_all(rpr.as_bytes())?;
        }
    }

    let mut t_start = BytesStart::new("w:t");
    t_start.push_attribute(("xml:space", "preserve"));
    writer.write_event(Event::Start(t_start))?;
    writer.write_event(Event::Text(BytesText::new(text)))?;
    writer.write_event(Event::End(BytesEnd::new("w:t")))?;

    writer.write_event(Event::End(BytesEnd::new("w:r")))?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Cursor;
    use std::path::Path;

    #[test]
    fn test_extract_and_merge_device_guide_docx() {
        let test_path = Path::new("../../test_data/デバイスガイド.docx");
        if !test_path.exists() {
            eprintln!("Skipping test: test_data/デバイスガイド.docx not found");
            return;
        }

        let file = File::open(test_path).expect("Failed to open デバイスガイド.docx");
        let filter = DocxFilter;
        let opts = FilterOptions {
            minimize_tags: true,
            source_lang: "ja".to_string(),
            target_lang: Some("en".to_string()),
            ..Default::default()
        };

        let ir = filter.extract(file, "デバイスガイド.docx", &opts).expect("Extract failed");
        assert!(!ir.units.is_empty());

        // マージテスト
        let skeleton_file = File::open(test_path).expect("Failed to open skeleton");
        let merger = DocxMerger;
        let mut trans_map = HashMap::new();
        for u in &ir.units {
            trans_map.insert(u.seq, TranslatedUnit {
                idx: u.seq,
                source: Some(u.source.clone()),
                target: format!("{}_translated", u.source),
                placeholders: HashMap::new(),
                note: None,
                is_sub: false,
            });
        }

        let mut out_buf = Cursor::new(Vec::new());
        merger.merge(skeleton_file, &trans_map, &mut out_buf, &MergeOptions::default())
            .expect("Merge failed");

        let out_bytes = out_buf.into_inner();
        let mut archive = ZipArchive::new(Cursor::new(&out_bytes)).expect("Invalid zip archive");
        let doc_xml = read_zip_entry_to_string(&mut archive, "word/document.xml")
            .expect("document.xml missing");

        // XML が整形式かパースして確認
        let mut reader = Reader::from_str(&doc_xml);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Eof) => break,
                Ok(_) => buf.clear(),
                Err(e) => panic!("Merged XML is not well-formed: {}", e),
            }
        }
    }
}
