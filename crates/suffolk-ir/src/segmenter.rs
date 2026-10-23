use crate::model::{DocumentIR, UnitIR};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static PLACEHOLDER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{@(\d+)\}").expect("Invalid regex for placeholder"));

static LATIN_ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "sr", "jr",
    "e.g", "i.e", "etc", "vs", "fig", "no", "al",
    "inc", "ltd", "corp", "dept", "co", "approx",
    "jan", "feb", "mar", "apr", "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec",
    "u.s", "u.k", "e.u",
];

/// 分割された単一セグメント情報
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentSpan {
    /// 文のテキスト本体（前後の不要な空白はトリム）
    pub text: String,
    /// 文末に付随していた区切り空白または改行（再結合時に使用）
    pub trailing_ws: String,
}

/// テキストを文単位（セグメント）に分割します。
///
/// ### 主な対応ルール:
/// 1. **日本語 / CJK**:
///    - `。`、`！`、`？` の直後で分割。
///    - かぎ括弧（`」`、`』`、`）`、`”` など）が直後にある場合は括弧の外側で分割。
/// 2. **英語 / 西欧言語**:
///    - ピリオド `.`、`!`、`?` 直後に空白または改行がある場合に分割。
///    - 一般略語（`Mr.`, `Dr.`, `e.g.`, `i.e.`, `etc.` 等）や数値（`1.5` 等）での誤分割を保護。
/// 3. **プレースホルダー保護**:
///    - `{@0}`, `{@1}` などのトークン内部では絶対に分割しません。
/// 4. **改行**:
///    - `\n` で段落内が改行されている場合も安全に文分割。
pub fn split_sentences(text: &str) -> Vec<SegmentSpan> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut spans = Vec::new();
    let mut start = 0;
    let mut i = 0;

    while i < n {
        let c = chars[i];

        // プレースホルダー `{@...}` のスキップ
        if c == '{' && i + 1 < n && chars[i + 1] == '@' {
            i += 2;
            while i < n && chars[i] != '}' {
                i += 1;
            }
            if i < n {
                i += 1; // skip '}'
            }
            continue;
        }

        // CJK 句点・終止符: `。`, `！`, `？`
        if c == '。' || c == '！' || c == '？' {
            let mut end_char_idx = i + 1;
            // 閉じ括弧が直後に続く場合は含める（例: 「...。」や（...？））
            while end_char_idx < n && is_cjk_closing_quote(chars[end_char_idx]) {
                end_char_idx += 1;
            }

            // 引用符の直後に引用の助詞「と」（例: 「...。」と言った）が続く場合は文の途中とみなす
            if end_char_idx < n && chars[end_char_idx] == 'と' {
                i = end_char_idx;
                continue;
            }

            // 後続の空白文字を trailing_ws として収集
            let mut ws_end = end_char_idx;
            while ws_end < n && (chars[ws_end] == ' ' || chars[ws_end] == '\t' || chars[ws_end] == '\r' || chars[ws_end] == '\n') {
                ws_end += 1;
            }

            let text_part: String = chars[start..end_char_idx].iter().collect();
            let ws_part: String = chars[end_char_idx..ws_end].iter().collect();

            if !text_part.trim().is_empty() {
                spans.push(SegmentSpan {
                    text: text_part.trim().to_string(),
                    trailing_ws: ws_part,
                });
            }

            start = ws_end;
            i = ws_end;
            continue;
        }

        // 改行文字 `\n` での分割
        if c == '\n' {
            let text_part: String = chars[start..i].iter().collect();
            let mut ws_end = i + 1;
            while ws_end < n && (chars[ws_end] == '\r' || chars[ws_end] == '\n') {
                ws_end += 1;
            }
            let ws_part: String = chars[i..ws_end].iter().collect();

            if !text_part.trim().is_empty() {
                spans.push(SegmentSpan {
                    text: text_part.trim().to_string(),
                    trailing_ws: ws_part,
                });
            }
            start = ws_end;
            i = ws_end;
            continue;
        }

        // 西欧終止符: `.`, `!`, `?`
        if c == '.' || c == '!' || c == '?' {
            let is_period = c == '.';
            let mut is_sentence_end = false;

            // 次の文字が空白・引用符・または終端かチェック
            let next_char = if i + 1 < n { Some(chars[i + 1]) } else { None };

            if next_char.is_none() {
                is_sentence_end = true;
            } else if let Some(nc) = next_char {
                if nc == ' ' || nc == '\t' || nc == '\r' || nc == '\n' || is_latin_closing_quote(nc) {
                    if is_period {
                        // 略語チェック & 数値チェック
                        if !is_abbreviation(&chars, start, i) && !is_numeric_decimal(&chars, i) {
                            is_sentence_end = true;
                        }
                    } else {
                        is_sentence_end = true;
                    }
                }
            }

            if is_sentence_end {
                let mut end_char_idx = i + 1;
                while end_char_idx < n && is_latin_closing_quote(chars[end_char_idx]) {
                    end_char_idx += 1;
                }

                let mut ws_end = end_char_idx;
                while ws_end < n && (chars[ws_end] == ' ' || chars[ws_end] == '\t' || chars[ws_end] == '\r' || chars[ws_end] == '\n') {
                    ws_end += 1;
                }

                let text_part: String = chars[start..end_char_idx].iter().collect();
                let ws_part: String = chars[end_char_idx..ws_end].iter().collect();

                if !text_part.trim().is_empty() {
                    spans.push(SegmentSpan {
                        text: text_part.trim().to_string(),
                        trailing_ws: ws_part,
                    });
                }

                start = ws_end;
                i = ws_end;
                continue;
            }
        }

        i += 1;
    }

    // 残りのテキスト
    if start < n {
        let remaining: String = chars[start..n].iter().collect();
        if !remaining.trim().is_empty() {
            spans.push(SegmentSpan {
                text: remaining.trim().to_string(),
                trailing_ws: String::new(),
            });
        }
    }

    spans
}

fn is_cjk_closing_quote(c: char) -> bool {
    matches!(c, '」' | '』' | '）' | '】' | '｝' | '”' | '"' | '’' | '\'')
}

fn is_latin_closing_quote(c: char) -> bool {
    matches!(c, '"' | '\'' | ')' | ']' | '}')
}

fn is_numeric_decimal(chars: &[char], dot_idx: usize) -> bool {
    let prev_is_digit = dot_idx > 0 && chars[dot_idx - 1].is_ascii_digit();
    let next_is_digit = dot_idx + 1 < chars.len() && chars[dot_idx + 1].is_ascii_digit();
    prev_is_digit && next_is_digit
}

fn is_abbreviation(chars: &[char], seg_start: usize, dot_idx: usize) -> bool {
    // ピリオドの直前の単語（英数字列）を遡って取得
    let mut word_start = dot_idx;
    while word_start > seg_start {
        let prev = chars[word_start - 1];
        if prev.is_alphabetic() || prev == '.' {
            word_start -= 1;
        } else {
            break;
        }
    }

    if word_start == dot_idx {
        return false;
    }

    let word: String = chars[word_start..dot_idx].iter().collect();
    let lower = word.to_lowercase();

    // 1文字の頭文字（イニシャル: "A.", "J." など）
    if word.len() == 1 && word.chars().next().map_or(false, |c| c.is_ascii_uppercase()) {
        return true;
    }

    LATIN_ABBREVIATIONS.contains(&lower.as_str())
}

/// 中間表現 `DocumentIR` 全体に対して文単位セグメンテーションを適用します。
///
/// - 各 `UnitIR` を文に細分化します。
/// - 同一ブロック内の先頭文は `is_sub: false`、2文目以降は `is_sub: true` となります。
/// - 全ての分割文は元の所属ブロック（段落）を示す共通の `group_id` を保持します。
/// - プレースホルダーは該当する文に適切に割り振られます。
pub fn segment_document_ir(ir: &DocumentIR) -> DocumentIR {
    let mut new_units = Vec::new();
    let mut global_seq = 1;

    for unit in &ir.units {
        let spans = split_sentences(&unit.source);
        if spans.is_empty() {
            let mut u = unit.clone();
            u.seq = global_seq;
            u.group_id = unit.seq;
            u.is_sub = false;
            new_units.push(u);
            global_seq += 1;
        } else if spans.len() == 1 {
            let mut u = unit.clone();
            u.seq = global_seq;
            u.group_id = unit.seq;
            u.is_sub = false;
            u.source = spans[0].text.clone();
            u.trailing_ws = spans[0].trailing_ws.clone();
            new_units.push(u);
            global_seq += 1;
        } else {
            for (sub_idx, span) in spans.into_iter().enumerate() {
                let is_sub = sub_idx > 0;
                let sub_id = if is_sub {
                    format!("{}_s{}", unit.id, sub_idx + 1)
                } else {
                    unit.id.clone()
                };

                // この文に含まれるプレースホルダーのみ抽出
                let mut sub_placeholders = HashMap::new();
                for cap in PLACEHOLDER_RE.captures_iter(&span.text) {
                    if let Ok(idx) = cap[1].parse::<usize>() {
                        if let Some(tag) = unit.placeholders.get(&idx) {
                            sub_placeholders.insert(idx, tag.clone());
                        }
                    }
                }

                let sub_unit = UnitIR {
                    id: sub_id,
                    seq: global_seq,
                    source: span.text,
                    target: if is_sub { None } else { unit.target.clone() },
                    pre: if is_sub { None } else { unit.pre.clone() },
                    note: unit.note.clone(),
                    memo: unit.memo.clone(),
                    placeholders: sub_placeholders,
                    context: unit.context.clone(),
                    is_sub,
                    group_id: unit.seq,
                    trailing_ws: span.trailing_ws,
                };

                new_units.push(sub_unit);
                global_seq += 1;
            }
        }
    }

    DocumentIR {
        meta: ir.meta.clone(),
        units: new_units,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DocumentFormat, DocumentMeta};

    #[test]
    fn test_japanese_sentence_split() {
        let text = "これはペンです。あれはノートです！それは何ですか？「はい」と言った。";
        let spans = split_sentences(text);
        assert_eq!(spans.len(), 4);
        assert_eq!(spans[0].text, "これはペンです。");
        assert_eq!(spans[1].text, "あれはノートです！");
        assert_eq!(spans[2].text, "それは何ですか？");
        assert_eq!(spans[3].text, "「はい」と言った。");
    }

    #[test]
    fn test_japanese_closing_quotes() {
        let text = "彼は「分かりました。」と言った。次の文です。";
        let spans = split_sentences(text);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "彼は「分かりました。」と言った。");
        assert_eq!(spans[1].text, "次の文です。");
    }

    #[test]
    fn test_english_sentence_split() {
        let text = "Hello world. This is a pen. How are you?";
        let spans = split_sentences(text);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "Hello world.");
        assert_eq!(spans[0].trailing_ws, " ");
        assert_eq!(spans[1].text, "This is a pen.");
        assert_eq!(spans[1].trailing_ws, " ");
        assert_eq!(spans[2].text, "How are you?");
    }

    #[test]
    fn test_english_abbreviation_protection() {
        let text = "Dr. Smith visited the U.S. in 2020. It was approx. 1.5 miles away.";
        let spans = split_sentences(text);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "Dr. Smith visited the U.S. in 2020.");
        assert_eq!(spans[1].text, "It was approx. 1.5 miles away.");
    }

    #[test]
    fn test_placeholders_preserved() {
        let text = "Text with {@0} inside. {@1}Second sentence{@2}.";
        let spans = split_sentences(text);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "Text with {@0} inside.");
        assert_eq!(spans[1].text, "{@1}Second sentence{@2}.");
    }

    #[test]
    fn test_segment_document_ir() {
        let meta = DocumentMeta {
            source_file: "test.docx".to_string(),
            source_lang: "en".to_string(),
            target_lang: Some("ja".to_string()),
            format: DocumentFormat::Docx,
            encoding: "UTF-8".to_string(),
        };

        let mut ir = DocumentIR::new(meta);
        let mut u1 = UnitIR::new("p1", 1, "Hello world. This is test.");
        u1.target = Some("こんにちは世界。テストです。".to_string());
        ir.add_unit(u1);

        let segmented = segment_document_ir(&ir);
        assert_eq!(segmented.units.len(), 2);
        assert_eq!(segmented.units[0].seq, 1);
        assert_eq!(segmented.units[0].is_sub, false);
        assert_eq!(segmented.units[0].group_id, 1);
        assert_eq!(segmented.units[0].source, "Hello world.");

        assert_eq!(segmented.units[1].seq, 2);
        assert_eq!(segmented.units[1].is_sub, true);
        assert_eq!(segmented.units[1].group_id, 1);
        assert_eq!(segmented.units[1].source, "This is test.");
    }
}
