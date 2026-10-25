use crate::shwv::{recombine_translated_units, TranslatedUnit};
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::collections::HashMap;
use std::io::BufRead;
use suffolk_ir::DocumentIR;
use thiserror::Error;

/// XLIFF 1.2 の処理に関するエラー定義
#[derive(Debug, Error)]
pub enum XliffError {
    #[error("XML parsing error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("XLIFF format error: {0}")]
    Format(String),
}

/// 中間表現 `DocumentIR` を XLIFF 1.2 形式の XML 文字列へシリアライズします。
///
/// # 構造仕様
/// - `<xliff version="1.2" xmlns="urn:oasis:names:tc:xliff:document:1.2">`
/// - `<file original="..." source-language="..." target-language="..." datatype="...">`
/// - 各セグメントは `<trans-unit id="{seq}" resname="{id}">`
/// - セグメンテーション時のサブ文は `extra="isSub"` を付与
/// - 原文は `<source xml:lang="...">`
/// - 訳文（事前翻訳または空）は `<target xml:lang="...">`
/// - メタ情報（シート名、行番号、セル番地等）は `<note>` に格納
pub fn write_xliff_12(ir: &DocumentIR) -> Result<String, XliffError> {
    let mut buffer = Vec::new();
    let mut writer = Writer::new_with_indent(&mut buffer, b' ', 2);

    // <?xml version="1.0" encoding="utf-8"?>
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    // <xliff version="1.2" xmlns="urn:oasis:names:tc:xliff:document:1.2">
    let mut xliff = BytesStart::new("xliff");
    xliff.push_attribute(("version", "1.2"));
    xliff.push_attribute(("xmlns", "urn:oasis:names:tc:xliff:document:1.2"));
    writer.write_event(Event::Start(xliff))?;

    // <file original="..." source-language="..." target-language="..." datatype="...">
    let mut file = BytesStart::new("file");
    file.push_attribute(("original", ir.meta.source_file.as_str()));
    file.push_attribute(("source-language", ir.meta.source_lang.as_str()));
    let target_lang = ir.meta.target_lang.as_deref().unwrap_or("ja");
    file.push_attribute(("target-language", target_lang));
    let datatype = match ir.meta.format {
        suffolk_ir::DocumentFormat::PlainText => "plaintext",
        suffolk_ir::DocumentFormat::Html => "html",
        suffolk_ir::DocumentFormat::Markdown => "markdown",
        _ => "xml",
    };
    file.push_attribute(("datatype", datatype));
    writer.write_event(Event::Start(file))?;

    // <header><tool tool-id="suffolk" tool-name="suffolk"/></header>
    writer.write_event(Event::Start(BytesStart::new("header")))?;
    let mut tool = BytesStart::new("tool");
    tool.push_attribute(("tool-id", "suffolk"));
    tool.push_attribute(("tool-name", "suffolk"));
    writer.write_event(Event::Empty(tool))?;
    writer.write_event(Event::End(BytesEnd::new("header")))?;

    // <body>
    writer.write_event(Event::Start(BytesStart::new("body")))?;

    for unit in &ir.units {
        // <trans-unit id="..." resname="..." [extra="isSub"]>
        let mut tu = BytesStart::new("trans-unit");
        let id_str = unit.seq.to_string();
        tu.push_attribute(("id", id_str.as_str()));
        tu.push_attribute(("resname", unit.id.as_str()));
        if unit.is_sub {
            tu.push_attribute(("extra", "isSub"));
        }
        writer.write_event(Event::Start(tu))?;

        // <source xml:lang="...">
        let mut source = BytesStart::new("source");
        source.push_attribute(("xml:lang", ir.meta.source_lang.as_str()));
        writer.write_event(Event::Start(source))?;
        writer.write_event(Event::Text(BytesText::new(&unit.source)))?;
        writer.write_event(Event::End(BytesEnd::new("source")))?;

        // <target xml:lang="...">
        let mut target = BytesStart::new("target");
        target.push_attribute(("xml:lang", target_lang));
        writer.write_event(Event::Start(target))?;
        let tgt_text = unit.target.as_deref().unwrap_or("");
        writer.write_event(Event::Text(BytesText::new(tgt_text)))?;
        writer.write_event(Event::End(BytesEnd::new("target")))?;

        // <note>...</note>
        if let Some(ref note) = unit.note {
            writer.write_event(Event::Start(BytesStart::new("note")))?;
            writer.write_event(Event::Text(BytesText::new(note)))?;
            writer.write_event(Event::End(BytesEnd::new("note")))?;
        }

        writer.write_event(Event::End(BytesEnd::new("trans-unit")))?;
    }

    writer.write_event(Event::End(BytesEnd::new("body")))?;
    writer.write_event(Event::End(BytesEnd::new("file")))?;
    writer.write_event(Event::End(BytesEnd::new("xliff")))?;

    let s = String::from_utf8(buffer)
        .map_err(|e| XliffError::Format(format!("UTF-8 encoding error: {}", e)))?;
    Ok(s)
}

/// XLIFF 1.2 XML から確定訳文をストリーミングパースし、復元用マップ（`id` -> `TranslatedUnit`）を作成します。
/// `<target>` 要素および `<note>` 要素のテキストを抽出し、サブセグメント（`isSub`）の自動再結合を行います。
pub fn parse_xliff_12<R: BufRead>(reader: R) -> Result<HashMap<usize, TranslatedUnit>, XliffError> {
    let mut xml_reader = Reader::from_reader(reader);
    xml_reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut units_list = Vec::new();
    let mut target_lang = "ja".to_string();

    let mut current_id: Option<usize> = None;
    let mut current_is_sub = false;
    let mut current_source = String::new();
    let mut current_target = String::new();
    let mut current_note = String::new();
    let mut in_source = false;
    let mut in_target = false;
    let mut in_note = false;

    loop {
        match xml_reader.read_event_into(&mut buf)? {
            Event::Start(ref e) => {
                let name = e.name();
                if name.as_ref() == b"file" {
                    for attr in e.attributes() {
                        let attr = attr.map_err(quick_xml::Error::from)?;
                        if attr.key.as_ref() == b"target-language" {
                            target_lang = String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }
                } else if name.as_ref() == b"trans-unit" {
                    current_is_sub = false;
                    for attr in e.attributes() {
                        let attr = attr.map_err(quick_xml::Error::from)?;
                        if attr.key.as_ref() == b"id" {
                            let val_str = String::from_utf8_lossy(&attr.value);
                            current_id = val_str.parse::<usize>().ok();
                        } else if attr.key.as_ref() == b"extra" && attr.value.as_ref() == b"isSub" {
                            current_is_sub = true;
                        }
                    }
                    current_source.clear();
                    current_target.clear();
                    current_note.clear();
                } else if name.as_ref() == b"source" {
                    in_source = true;
                    current_source.clear();
                } else if name.as_ref() == b"target" {
                    in_target = true;
                    current_target.clear();
                } else if name.as_ref() == b"note" {
                    in_note = true;
                    current_note.clear();
                }
            }
            Event::Text(ref e) => {
                if in_source {
                    current_source.push_str(&e.unescape()?);
                } else if in_target {
                    current_target.push_str(&e.unescape()?);
                } else if in_note {
                    current_note.push_str(&e.unescape()?);
                }
            }
            Event::End(ref e) => {
                let name = e.name();
                if name.as_ref() == b"source" {
                    in_source = false;
                } else if name.as_ref() == b"target" {
                    in_target = false;
                } else if name.as_ref() == b"note" {
                    in_note = false;
                } else if name.as_ref() == b"trans-unit" {
                    if let Some(id) = current_id.take() {
                        let effective_target = if current_target.trim().is_empty() {
                            current_source.clone()
                        } else {
                            current_target.clone()
                        };

                        units_list.push(TranslatedUnit {
                            idx: id,
                            source: if current_source.is_empty() {
                                None
                            } else {
                                Some(current_source.clone())
                            },
                            target: effective_target,
                            placeholders: HashMap::new(),
                            note: if current_note.is_empty() {
                                None
                            } else {
                                Some(current_note.clone())
                            },
                            is_sub: current_is_sub,
                        });
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(recombine_translated_units(&units_list, &target_lang))
}

#[cfg(test)]
mod tests {
    use super::*;
    use suffolk_ir::{DocumentFormat, DocumentMeta, UnitIR};

    #[test]
    fn test_xliff_roundtrip() {
        let meta = DocumentMeta {
            source_file: "test.txt".to_string(),
            source_lang: "en".to_string(),
            target_lang: Some("ja".to_string()),
            format: DocumentFormat::PlainText,
            encoding: "UTF-8".to_string(),
        };
        let mut ir = DocumentIR::new(meta);
        let mut u1 = UnitIR::new("u1", 1, "Hello world");
        u1.target = Some("こんにちは世界".to_string());
        u1.note = Some("[Line 1]".to_string());
        ir.add_unit(u1);

        let xml = write_xliff_12(&ir).unwrap();
        assert!(xml.contains("<source xml:lang=\"en\">Hello world</source>"));
        assert!(xml.contains("<target xml:lang=\"ja\">こんにちは世界</target>"));

        let trans = parse_xliff_12(xml.as_bytes()).unwrap();
        assert_eq!(trans.get(&1).unwrap().target, "こんにちは世界");
    }
}
