use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::collections::HashMap;
use suffolk_ir::{InlineTag, TagType};

/// テキスト、視覚的スタイル署名、および生のXMLプロパティを保持するラン情報
#[derive(Debug, Clone)]
pub struct ProcessedRun {
    /// ラン内のテキスト文字列
    pub text: String,
    /// 有意なスタイル署名（例: "bold;color:FF0000"）
    pub style_sig: String,
    /// 元の `<w:rPr>` または `<a:rPr>` XMLスニペット
    pub raw_rpr: Option<String>,
}

/// `<w:rPr>` や `<a:rPr>` から、翻訳上意味のある視覚的装飾（太字・斜体・下線・打消線・文字色・蛍光ペン）のみを抽出し、
/// 正規化されたスタイル署名文字列（例: `"bold;color:FF0000"`）を生成します。
///
/// ### 除外されるノイズ属性
/// - スペルチェック/校正記号 (`w:noProof`)
/// - 言語コード (`w:lang`)
/// - フォント指定 (`w:rFonts`)
/// - 東アジア言語入力ヒント (`w:hint`)
///
/// これらの一時的・内部的な属性差分を無視することで、不要なタグ分断を防ぎます。
pub fn extract_style_signature(rpr_xml: &str) -> String {
    let mut sigs = Vec::new();
    let mut reader = Reader::from_str(rpr_xml);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local_name = if let Some(pos) = name.as_ref().iter().position(|&b| b == b':') {
                    &name.as_ref()[pos + 1..]
                } else {
                    name.as_ref()
                };

                match local_name {
                    b"b" | b"bCs" => {
                        let is_on = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"w:val")
                            .map(|a| a.value.as_ref() != b"false" && a.value.as_ref() != b"0")
                            .unwrap_or(true);
                        if is_on {
                            sigs.push("bold".to_string());
                        }
                    }
                    b"i" | b"iCs" => {
                        let is_on = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"w:val")
                            .map(|a| a.value.as_ref() != b"false" && a.value.as_ref() != b"0")
                            .unwrap_or(true);
                        if is_on {
                            sigs.push("italic".to_string());
                        }
                    }
                    b"u" => {
                        let val = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"w:val")
                            .map(|a| String::from_utf8_lossy(&a.value).to_string())
                            .unwrap_or_else(|| "single".to_string());
                        if val != "none" {
                            sigs.push(format!("underline:{}", val));
                        }
                    }
                    b"strike" | b"dstrike" => {
                        sigs.push("strike".to_string());
                    }
                    b"color" => {
                        if let Some(val) = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"w:val")
                        {
                            let c = String::from_utf8_lossy(&val.value);
                            if c != "auto" {
                                sigs.push(format!("color:{}", c));
                            }
                        }
                    }
                    b"highlight" => {
                        if let Some(val) = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"w:val")
                        {
                            let h = String::from_utf8_lossy(&val.value);
                            if h != "none" {
                                sigs.push(format!("highlight:{}", h));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    sigs.sort();
    sigs.join(";")
}

/// 同一のスタイル署名を持つ隣接ランを結合（Coalesce）します。
///
/// Word 等の編集履歴やスペルチェックにより寸断された同一書式のテキストを
/// 1つのまとまった文字列に復元します。
pub fn coalesce_runs(runs: Vec<ProcessedRun>) -> Vec<ProcessedRun> {
    if runs.is_empty() {
        return runs;
    }

    let mut result = Vec::new();
    let mut current = runs[0].clone();

    for run in runs.into_iter().skip(1) {
        if run.style_sig == current.style_sig {
            current.text.push_str(&run.text);
        } else {
            result.push(current);
            current = run;
        }
    }
    result.push(current);
    result
}

/// 結合後のラン配列から、スタイル境界に `{0}`, `{1}` などのプレースホルダーを挿入した
/// 正規化文字列とプレースホルダー辞書を生成します。
///
/// 段落全体で同一の書式が適用されている場合や、プレーンテキストの場合はプレースホルダーを
/// 挿入せず、そのままのテキストを返します。
pub fn runs_to_normalized_text(
    runs: &[ProcessedRun],
    _minimize_tags: bool,
) -> (String, HashMap<usize, InlineTag>) {
    if runs.is_empty() {
        return (String::new(), HashMap::new());
    }

    // 全てのランが同一スタイル、またはランが1つだけの場合はタグ不要
    let all_same = runs.iter().all(|r| r.style_sig == runs[0].style_sig);
    if all_same || runs.len() == 1 {
        let full_text = runs.iter().map(|r| r.text.as_str()).collect::<String>();
        return (full_text, HashMap::new());
    }

    let mut result = String::new();
    let mut placeholders = HashMap::new();
    let mut tag_index = 0;

    for run in runs {
        if run.text.is_empty() {
            continue;
        }

        if run.style_sig.is_empty() {
            result.push_str(&run.text);
        } else {
            let open_idx = tag_index;
            let close_idx = tag_index + 1;
            tag_index += 2;

            placeholders.insert(
                open_idx,
                InlineTag {
                    raw: run.raw_rpr.clone().unwrap_or_default(),
                    tag_type: TagType::Opening,
                    name: Some("rPr".to_string()),
                },
            );
            placeholders.insert(
                close_idx,
                InlineTag {
                    raw: String::new(),
                    tag_type: TagType::Closing,
                    name: Some("rPr".to_string()),
                },
            );

            result.push_str(&format!("{{@{}}}", open_idx));
            result.push_str(&run.text);
            result.push_str(&format!("{{@{}}}", close_idx));
        }
    }

    (result, placeholders)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_style_signature() {
        let rpr = r#"<w:rPr><w:rFonts w:ascii="Calibri"/><w:b/><w:color w:val="FF0000"/><w:lang w:val="en-US"/></w:rPr>"#;
        let sig = extract_style_signature(rpr);
        assert_eq!(sig, "bold;color:FF0000"); // w:rFonts and w:lang are ignored!
    }

    #[test]
    fn test_coalesce_runs() {
        let runs = vec![
            ProcessedRun {
                text: "Hello ".to_string(),
                style_sig: "".to_string(),
                raw_rpr: None,
            },
            ProcessedRun {
                text: "world".to_string(),
                style_sig: "".to_string(),
                raw_rpr: None,
            },
            ProcessedRun {
                text: "!".to_string(),
                style_sig: "bold".to_string(),
                raw_rpr: Some("<w:rPr><w:b/></w:rPr>".to_string()),
            },
        ];

        let coalesced = coalesce_runs(runs);
        assert_eq!(coalesced.len(), 2);
        assert_eq!(coalesced[0].text, "Hello world");
        assert_eq!(coalesced[1].text, "!");

        let (text, tags) = runs_to_normalized_text(&coalesced, true);
        assert_eq!(text, "Hello world{@0}!{@1}");
        assert_eq!(tags.len(), 2);
    }
}
