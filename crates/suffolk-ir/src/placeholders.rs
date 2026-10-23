use crate::model::InlineTag;
use std::collections::HashMap;

/// 翻訳後テキスト内のプレースホルダー `{@0}`, `{@1}` などを
/// 元のインラインタグ（OpenXMLの書式タグやHTMLタグなど）に復元します。
/// CAT ツール内の競合を回避する `{@idx}` 形式を標準とし、後方互換性のため `{idx}` 形式も認識します。
///
/// # 引数
/// * `text` - プレースホルダーを含む翻訳済み文字列（例: "こんにちは{@0}世界{@1}"）
/// * `placeholders` - タグのインデックスと `InlineTag` のマッピング辞書
///
/// # 挙動
/// - 辞書が空の場合は入力文字列をそのまま返します。
/// - `{@数字}` または `{数字}` 形式を検出し、辞書に存在するインデックスであれば `InlineTag.raw` に置換します。
/// - 辞書に存在しないプレースホルダーや不正な形式の場合は、文字列を変更せずそのまま保持します。
pub fn restore_placeholders(text: &str, placeholders: &HashMap<usize, InlineTag>) -> String {
    if placeholders.is_empty() {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' {
            let mut is_at = false;
            if let Some(&'@') = chars.peek() {
                chars.next(); // '@' を消費
                is_at = true;
            }

            let mut num_str = String::new();
            let mut is_placeholder = false;

            while let Some(&next_c) = chars.peek() {
                if next_c.is_ascii_digit() {
                    num_str.push(next_c);
                    chars.next();
                } else if next_c == '}' && !num_str.is_empty() {
                    chars.next(); // '}' を消費
                    is_placeholder = true;
                    break;
                } else {
                    break;
                }
            }

            if is_placeholder {
                if let Ok(idx) = num_str.parse::<usize>() {
                    if let Some(tag) = placeholders.get(&idx) {
                        result.push_str(&tag.raw);
                    } else {
                        // 辞書に該当インデックスが存在しない場合は元のトークンをそのまま出力
                        result.push('{');
                        if is_at {
                            result.push('@');
                        }
                        result.push_str(&num_str);
                        result.push('}');
                    }
                }
            } else {
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

/// `ShWvUnit.placeholders`（`HashMap<usize, String>`）を内部IR用の `HashMap<usize, InlineTag>` に変換します。
/// タグ文字列の先頭・末尾文字からタグ種別（開始/終了/単独タグ）を簡易判定します。
pub fn string_map_to_tags(map: &HashMap<usize, String>) -> HashMap<usize, InlineTag> {
    use crate::model::TagType;
    map.iter()
        .map(|(&k, v)| {
            let tag_type = if v.starts_with("</") {
                TagType::Closing
            } else if v.ends_with("/>") {
                TagType::Standalone
            } else {
                TagType::Opening
            };
            (
                k,
                InlineTag {
                    raw: v.clone(),
                    tag_type,
                    name: None,
                },
            )
        })
        .collect()
}

/// 内部IR用の `HashMap<usize, InlineTag>` を JSON出力用の `HashMap<usize, String>` に変換します。
pub fn tags_to_string_map(tags: &HashMap<usize, InlineTag>) -> HashMap<usize, String> {
    tags.iter().map(|(&k, v)| (k, v.raw.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TagType;

    #[test]
    fn test_restore_placeholders() {
        let mut tags = HashMap::new();
        tags.insert(
            0,
            InlineTag {
                raw: "<b>".to_string(),
                tag_type: TagType::Opening,
                name: Some("b".to_string()),
            },
        );
        tags.insert(
            1,
            InlineTag {
                raw: "</b>".to_string(),
                tag_type: TagType::Closing,
                name: Some("b".to_string()),
            },
        );

        // Test standard {@idx} format
        let text_at = "Hello {@0}world{@1}!";
        let restored_at = restore_placeholders(text_at, &tags);
        assert_eq!(restored_at, "Hello <b>world</b>!");

        // Test backwards compatibility with {idx} format
        let text_legacy = "Hello {0}world{1}!";
        let restored_legacy = restore_placeholders(text_legacy, &tags);
        assert_eq!(restored_legacy, "Hello <b>world</b>!");
    }
}
