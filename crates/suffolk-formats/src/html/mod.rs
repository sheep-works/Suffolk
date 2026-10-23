use crate::traits::{DocumentFilter, DocumentMerger, FilterOptions, FormatError, MergeOptions};
use lol_html::html_content::ContentType;
use lol_html::{element, end_tag, rewrite_str, text, RewriteStrSettings};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use std::rc::Rc;
use suffolk_codec::TranslatedUnit;
use suffolk_ir::{
    restore_placeholders, string_map_to_tags, DocumentFormat, DocumentIR, DocumentMeta, InlineTag,
    TagType, UnitIR,
};

pub struct HtmlFilter;
pub struct HtmlMerger;

impl DocumentFilter for HtmlFilter {
    /// HTML 文書から翻訳対象要素を抽出します。
    ///
    /// ### 処理概要
    /// 1. Cloudflare 製の高速ストリーミング HTML パーサー `lol_html` を使用。
    /// 2. `<script>` および `<style>` タグ内部のテキストを厳密に除外し、コード破壊を防止。
    /// 3. ブロック要素（`title`, `h1`〜`h6`, `p`, `button`, `a`, `li`, `td`, `th`）をセグメント化。
    /// 4. ブロック内のインラインタグ（`<span>`, `<b>`, `<code>` 等）を `{0}`, `{1}` プレースホルダーとして保護。
    /// 5. 要素名を `context.style_name`、通番を `note` に記録。
    fn extract<R: Read + Seek>(
        &self,
        mut reader: R,
        filename: &str,
        options: &FilterOptions,
    ) -> Result<DocumentIR, FormatError> {
        let mut raw_bytes = Vec::new();
        reader.read_to_end(&mut raw_bytes)?;
        let html_str = String::from_utf8_lossy(&raw_bytes).to_string();

        let meta = DocumentMeta {
            source_file: filename.to_string(),
            source_lang: options.source_lang.clone(),
            target_lang: options.target_lang.clone(),
            format: DocumentFormat::Html,
            encoding: "UTF-8".to_string(),
        };

        let mut doc = DocumentIR::new(meta);
        let units_cell = Rc::new(RefCell::new(Vec::new()));
        let units_clone = Rc::clone(&units_cell);

        let current_text = Rc::new(RefCell::new(String::new()));
        let current_placeholders = Rc::new(RefCell::new(HashMap::new()));
        let tag_counter = Rc::new(RefCell::new(0));

        let current_text_c = Rc::clone(&current_text);
        let current_placeholders_c = Rc::clone(&current_placeholders);
        let tag_counter_c = Rc::clone(&tag_counter);

        // Track when we are inside script or style
        let in_non_translatable = Rc::new(RefCell::new(false));
        let in_non_trans_c1 = Rc::clone(&in_non_translatable);
        let in_non_trans_c2 = Rc::clone(&in_non_translatable);

        let current_text_span = Rc::clone(&current_text);
        let current_text_text = Rc::clone(&current_text);

        let settings = RewriteStrSettings {
            element_content_handlers: vec![
                element!("script, style", move |el| {
                    *in_non_trans_c1.borrow_mut() = true;
                    let in_non_trans_end = Rc::clone(&in_non_trans_c1);
                    el.on_end_tag(end_tag!(move |_| {
                        *in_non_trans_end.borrow_mut() = false;
                        Ok(())
                    }))?;
                    Ok(())
                }),
                element!(
                    "title, h1, h2, h3, h4, h5, h6, p, button, a, li, td, th",
                    move |el| {
                        if *in_non_trans_c2.borrow() {
                            return Ok(());
                        }

                        let tag_name = el.tag_name();
                        current_text_c.borrow_mut().clear();
                        current_placeholders_c.borrow_mut().clear();
                        *tag_counter_c.borrow_mut() = 0;

                        let units_ref = Rc::clone(&units_clone);
                        let tag_name_end = tag_name.clone();
                        let text_end = Rc::clone(&current_text_c);
                        let ph_end = Rc::clone(&current_placeholders_c);

                        el.on_end_tag(end_tag!(move |_end_tag| {
                            let full_text = text_end.borrow().clone();
                            let trimmed = full_text.trim();
                            if !trimmed.is_empty() {
                                let mut u_list = units_ref.borrow_mut();
                                let seq = u_list.len() + 1;
                                let mut unit = UnitIR::new(
                                    format!("{}_{}", tag_name_end, seq),
                                    seq,
                                    trimmed,
                                );
                                unit.placeholders = ph_end.borrow().clone();
                                unit.context.style_name = Some(tag_name_end.clone());
                                unit.populate_note_from_context(None);
                                u_list.push(unit);
                            }
                            Ok(())
                        }))?;

                        Ok(())
                    }
                ),
                element!("span, strong, em, b, i, u, code", move |el| {
                    let ph_map = Rc::clone(&current_placeholders);
                    let text_ref = Rc::clone(&current_text_span);
                    let counter_ref = Rc::clone(&tag_counter);

                    let open_idx = *counter_ref.borrow();
                    let close_idx = open_idx + 1;
                    *counter_ref.borrow_mut() += 2;

                    let tag_name = el.tag_name();
                    let mut raw_open = format!("<{}", tag_name);
                    for attr in el.attributes() {
                        raw_open.push_str(&format!(" {}=\"{}\"", attr.name(), attr.value()));
                    }
                    raw_open.push('>');

                    ph_map.borrow_mut().insert(
                        open_idx,
                        InlineTag {
                            raw: raw_open,
                            tag_type: TagType::Opening,
                            name: Some(tag_name.clone()),
                        },
                    );
                    ph_map.borrow_mut().insert(
                        close_idx,
                        InlineTag {
                            raw: format!("</{}>", tag_name),
                            tag_type: TagType::Closing,
                            name: Some(tag_name.clone()),
                        },
                    );

                    text_ref.borrow_mut().push_str(&format!("{{@{}}}", open_idx));

                    let text_end = Rc::clone(&text_ref);
                    el.on_end_tag(end_tag!(move |_| {
                        text_end.borrow_mut().push_str(&format!("{{@{}}}", close_idx));
                        Ok(())
                    }))?;

                    Ok(())
                }),
                text!(
                    "title, h1, h2, h3, h4, h5, h6, p, button, a, li, td, th, span, strong, em, b, i, u, code",
                    move |t| {
                        current_text_text.borrow_mut().push_str(t.as_str());
                        Ok(())
                    }
                ),
            ],
            ..RewriteStrSettings::default()
        };

        let _ = rewrite_str(&html_str, settings).map_err(|e| FormatError::Parse(e.to_string()))?;

        let units = Rc::try_unwrap(units_cell)
            .map_err(|_| FormatError::Parse("Failed to unwrap units".to_string()))?
            .into_inner();

        for u in units {
            doc.add_unit(u);
        }

        Ok(doc)
    }
}

impl DocumentMerger for HtmlMerger {
    /// 翻訳結果を元の HTML 骨格にマージします。
    /// 各ブロック要素の内容を、インラインタグを復元した翻訳テキストで直接ストリーミング置換します。
    fn merge<R: Read + Seek, W: Write + Seek>(
        &self,
        mut skeleton: R,
        translations: &HashMap<usize, TranslatedUnit>,
        mut writer: W,
        _options: &MergeOptions,
    ) -> Result<(), FormatError> {
        let mut raw_bytes = Vec::new();
        skeleton.read_to_end(&mut raw_bytes)?;
        let html_str = String::from_utf8_lossy(&raw_bytes).to_string();

        let seq_counter = Rc::new(RefCell::new(1));
        let seq_c = Rc::clone(&seq_counter);
        let trans_map = translations.clone();

        let settings = RewriteStrSettings {
            element_content_handlers: vec![element!(
                "title, h1, h2, h3, h4, h5, h6, p, button, a, li, td, th",
                move |el| {
                    let seq = *seq_c.borrow();
                    *seq_c.borrow_mut() += 1;

                    if let Some(unit) = trans_map.get(&seq) {
                        let tags = string_map_to_tags(&unit.placeholders);
                        let restored = restore_placeholders(&unit.target, &tags);
                        el.set_inner_content(&restored, ContentType::Html);
                    }

                    Ok(())
                }
            )],
            ..RewriteStrSettings::default()
        };

        let rewritten_html =
            rewrite_str(&html_str, settings).map_err(|e| FormatError::Parse(e.to_string()))?;

        writer.write_all(rewritten_html.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Cursor;
    use std::path::Path;

    #[test]
    fn test_extract_ui_html() {
        let test_path = Path::new("../../test_data/ui.html");
        if !test_path.exists() {
            eprintln!("Skipping test: test_data/ui.html not found");
            return;
        }

        let file = File::open(test_path).expect("Failed to open ui.html");
        let filter = HtmlFilter;
        let opts = FilterOptions {
            source_lang: "ja".to_string(),
            target_lang: Some("en".to_string()),
            ..Default::default()
        };

        let doc = filter.extract(file, "ui.html", &opts).unwrap();
        assert!(!doc.units.is_empty(), "Units should not be empty");

        println!("Extracted HTML units: {}", doc.units.len());
        for u in &doc.units {
            println!("Unit {}: note={:?} src={}", u.seq, u.note, u.source);
        }

        // Verify title, buttons, p with span
        assert_eq!(doc.units[0].source, "サンプルページ");
        assert_eq!(doc.units[1].source, "Shape Creator");

        let p_unit = doc
            .units
            .iter()
            .find(|u| u.source.contains("{@0}"))
            .expect("Paragraph with span placeholder not found");
        assert!(p_unit.placeholders.contains_key(&0));
        assert!(p_unit.placeholders.get(&0).unwrap().raw.contains("class=\"bigger\""));

        // Test merge
        let mut trans = HashMap::new();
        trans.insert(1, TranslatedUnit::new(1, "Sample Page"));
        trans.insert(2, TranslatedUnit::new(2, "シェイプクリエイター"));

        let merger = HtmlMerger;
        let skeleton_file = File::open(test_path).unwrap();
        let mut merged_out = Cursor::new(Vec::new());

        merger
            .merge(skeleton_file, &trans, &mut merged_out, &MergeOptions::default())
            .unwrap();

        let merged_str = String::from_utf8(merged_out.into_inner()).unwrap();
        assert!(merged_str.contains("<title>Sample Page</title>"));
        assert!(merged_str.contains("<h2>シェイプクリエイター</h2>"));
        // Ensure script and style remain intact
        assert!(merged_str.contains("parent.postMessage"));
        assert!(merged_str.contains("width: 100vw;"));
    }
}
