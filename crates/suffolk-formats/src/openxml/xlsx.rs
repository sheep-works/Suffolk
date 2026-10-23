use crate::openxml::zip_handler::{read_zip_entry_to_string, rewrite_zip};
use crate::traits::{DocumentFilter, DocumentMerger, FilterOptions, FormatError, MergeOptions};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Read, Seek, Write};
use suffolk_codec::TranslatedUnit;
use suffolk_ir::{
    restore_placeholders, string_map_to_tags, DocumentFormat, DocumentIR, DocumentMeta, UnitIR,
};
use zip::ZipArchive;

pub struct XlsxFilter;
pub struct XlsxMerger;

/// xl/workbook.xml およびリレーションから抽出したワークシート情報
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct SheetInfo {
    /// ワークシート名（例: "Device_Config"）
    name: String,
    /// シートID
    sheet_id: u32,
    /// リレーションシップID（例: "rId1"）
    rel_id: String,
    /// Zip内のXMLファイルパス（例: "xl/worksheets/sheet1.xml"）
    target_file: String,
}

/// Excel の列文字列（"A", "B", "AA" 等）を 1 始まりの数値インデックス（1, 2, 27 等）に変換します。
pub fn col_to_num(col: &str) -> u32 {
    let mut num = 0;
    for c in col.chars() {
        if c.is_ascii_alphabetic() {
            num = num * 26 + (c.to_ascii_uppercase() as u32 - b'A' as u32 + 1);
        }
    }
    num
}

/// セル参照文字列（例: "A10"）を列文字と行番号のタプル（`("A", 10)`）に分解します。
pub fn parse_cell_ref(r: &str) -> Option<(String, u32)> {
    let col: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let row_str: String = r.chars().skip_while(|c| c.is_ascii_alphabetic()).collect();
    let row = row_str.parse::<u32>().ok()?;
    if col.is_empty() {
        return None;
    }
    Some((col.to_ascii_uppercase(), row))
}

/// `xl/sharedStrings.xml` から文字列テーブル（SST）を順序どおりに全件抽出します。
///
/// ### 日本語ルビ・ふりがな (`<rPh>`) の除外処理
/// Excel で日本語入力時にふりがな情報が設定されている場合、`<si>` 内に
/// `<t>起動</t><rPh><t>キドウ</t></rPh>` のように `<rPh>` 要素が含まれます。
/// これを無条件にテキスト抽出すると「起動キドウ」のように読み仮名が重複結合されてしまうため、
/// `<rPh>` 要素内部のテキストはスキップして親テキストのみを忠実に抽出します。
fn parse_shared_strings<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Vec<String> {
    let Some(xml_content) = read_zip_entry_to_string(archive, "xl/sharedStrings.xml") else {
        return Vec::new();
    };

    let mut reader = Reader::from_str(&xml_content);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut strings = Vec::new();

    let mut current_string = String::new();
    let mut in_t = false;
    let mut in_si = false;
    let mut in_rph = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                if name.as_ref() == b"si" {
                    in_si = true;
                    current_string.clear();
                } else if name.as_ref() == b"rPh" {
                    in_rph = true;
                } else if in_si && !in_rph && name.as_ref() == b"t" {
                    in_t = true;
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_t && !in_rph {
                    if let Ok(unescaped) = e.unescape() {
                        current_string.push_str(&unescaped);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                if name.as_ref() == b"t" {
                    in_t = false;
                } else if name.as_ref() == b"rPh" {
                    in_rph = false;
                } else if name.as_ref() == b"si" {
                    in_si = false;
                    strings.push(current_string.clone());
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    strings
}

/// `xl/workbook.xml` および `xl/_rels/workbook.xml.rels` を解析し、
/// 各シートの表示名と対応するXMLファイルパス（例: "xl/worksheets/sheet1.xml"）のリストを取得します。
fn parse_sheets<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Vec<SheetInfo> {
    let mut rels_map: HashMap<String, String> = HashMap::new();
    if let Some(rels_xml) = read_zip_entry_to_string(archive, "xl/_rels/workbook.xml.rels") {
        let mut reader = Reader::from_str(&rels_xml);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    if e.name().as_ref() == b"Relationship" {
                        let mut id = String::new();
                        let mut target = String::new();
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"Id" {
                                id = String::from_utf8_lossy(&attr.value).to_string();
                            } else if attr.key.as_ref() == b"Target" {
                                target = String::from_utf8_lossy(&attr.value).to_string();
                            }
                        }
                        if !id.is_empty() && !target.is_empty() {
                            let path = if target.starts_with('/') {
                                target.trim_start_matches('/').to_string()
                            } else {
                                format!("xl/{}", target)
                            };
                            rels_map.insert(id, path);
                        }
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }

    let mut sheets = Vec::new();
    let Some(wb_xml) = read_zip_entry_to_string(archive, "xl/workbook.xml") else {
        return sheets;
    };

    let mut reader = Reader::from_str(&wb_xml);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                if e.name().as_ref() == b"sheet" {
                    let mut name = String::new();
                    let mut sheet_id = 0;
                    let mut rel_id = String::new();

                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"name" {
                            name = String::from_utf8_lossy(&attr.value).to_string();
                        } else if attr.key.as_ref() == b"sheetId" {
                            sheet_id = String::from_utf8_lossy(&attr.value)
                                .parse::<u32>()
                                .unwrap_or(0);
                        } else if attr.key.as_ref() == b"r:id" {
                            rel_id = String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }

                    let target_file = rels_map
                        .get(&rel_id)
                        .cloned()
                        .unwrap_or_else(|| format!("xl/worksheets/sheet{}.xml", sheet_id));

                    sheets.push(SheetInfo {
                        name,
                        sheet_id,
                        rel_id,
                        target_file,
                    });
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    sheets
}

/// Parsed cell data from sheet xml
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct CellData {
    col: String,
    row: u32,
    text: String,
}

/// Reads worksheet XML and extracts cells grouped by row: row -> col -> CellData
fn parse_worksheet_cells(
    xml_content: &str,
    shared_strings: &[String],
) -> BTreeMap<u32, BTreeMap<String, CellData>> {
    let mut rows_map: BTreeMap<u32, BTreeMap<String, CellData>> = BTreeMap::new();
    let mut reader = Reader::from_str(xml_content);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut current_cell_ref = String::new();
    let mut current_cell_type = String::new();
    let mut current_val = String::new();
    let mut in_v = false;
    let mut in_t = false;
    let mut in_inline_str = false;
    let mut inline_text = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                if name.as_ref() == b"c" {
                    current_cell_ref.clear();
                    current_cell_type.clear();
                    current_val.clear();
                    inline_text.clear();
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"r" {
                            current_cell_ref = String::from_utf8_lossy(&attr.value).to_string();
                        } else if attr.key.as_ref() == b"t" {
                            current_cell_type = String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }
                } else if name.as_ref() == b"v" {
                    in_v = true;
                    current_val.clear();
                } else if name.as_ref() == b"is" {
                    in_inline_str = true;
                } else if in_inline_str && name.as_ref() == b"t" {
                    in_t = true;
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_v {
                    if let Ok(u) = e.unescape() {
                        current_val.push_str(&u);
                    }
                } else if in_t {
                    if let Ok(u) = e.unescape() {
                        inline_text.push_str(&u);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                if name.as_ref() == b"v" {
                    in_v = false;
                } else if name.as_ref() == b"t" {
                    in_t = false;
                } else if name.as_ref() == b"is" {
                    in_inline_str = false;
                } else if name.as_ref() == b"c" {
                    if let Some((col, row)) = parse_cell_ref(&current_cell_ref) {
                        let text = if current_cell_type == "s" {
                            if let Ok(idx) = current_val.parse::<usize>() {
                                shared_strings.get(idx).cloned().unwrap_or_default()
                            } else {
                                String::new()
                            }
                        } else if current_cell_type == "inlineStr" {
                            inline_text.clone()
                        } else {
                            String::new()
                        };

                        if !text.is_empty() {
                            rows_map.entry(row).or_default().insert(
                                col.clone(),
                                CellData {
                                    col,
                                    row,
                                    text,
                                },
                            );
                        }
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    rows_map
}

impl DocumentFilter for XlsxFilter {
    fn extract<R: Read + Seek>(
        &self,
        mut reader: R,
        filename: &str,
        options: &FilterOptions,
    ) -> Result<DocumentIR, FormatError> {
        let mut archive = ZipArchive::new(&mut reader)?;
        let shared_strings = parse_shared_strings(&mut archive);
        let sheets = parse_sheets(&mut archive);

        let meta = DocumentMeta {
            source_file: filename.to_string(),
            source_lang: options.source_lang.clone(),
            target_lang: options.target_lang.clone(),
            format: DocumentFormat::Xlsx,
            encoding: "UTF-8".to_string(),
        };

        let mut doc = DocumentIR::new(meta);
        let mut seq = 1;

        let is_multilingual = !options.xlsx_source_cols.is_empty();
        let sheet_regex = options
            .xlsx_sheet_pattern
            .as_deref()
            .and_then(|p| Regex::new(p).ok());

        for sheet in sheets {
            if let Some(ref re) = sheet_regex {
                if !re.is_match(&sheet.name) {
                    continue;
                }
            }

            let Some(sheet_xml) = read_zip_entry_to_string(&mut archive, &sheet.target_file)
            else {
                continue;
            };

            let rows_map = parse_worksheet_cells(&sheet_xml, &shared_strings);

            if is_multilingual {
                // Multilingual column filtering mode
                for (row_idx, cols) in rows_map {
                    if options.xlsx_skip_rows.contains(&row_idx) {
                        continue;
                    }

                    for (col_idx, src_col) in options.xlsx_source_cols.iter().enumerate() {
                        let src_col_upper = src_col.to_ascii_uppercase();
                        if let Some(src_cell) = cols.get(&src_col_upper) {
                            let src_text = src_cell.text.clone();
                            if src_text.trim().is_empty() {
                                continue;
                            }

                            // Look up pre-existing target if target col specified
                            let tgt_text = options
                                .xlsx_target_cols
                                .get(col_idx)
                                .map(|tc| tc.to_ascii_uppercase())
                                .and_then(|tc| cols.get(&tc).map(|c| c.text.clone()));

                            // Look up context note if context col specified
                            let ctx_text = options
                                .xlsx_context_cols
                                .get(col_idx)
                                .map(|cc| cc.to_ascii_uppercase())
                                .and_then(|cc| cols.get(&cc).map(|c| c.text.clone()));

                            let cell_ref = format!("{}{}", src_col_upper, row_idx);
                            let unit_id = format!("{}!{}", sheet.name, cell_ref);

                            let mut unit = UnitIR::new(unit_id, seq, src_text);
                            unit.target = tgt_text.clone();
                            unit.pre = tgt_text;
                            unit.context.sheet_name = Some(sheet.name.clone());
                            unit.context.cell_ref = Some(cell_ref);
                            unit.context.row = Some(row_idx);
                            unit.context.col = Some(src_col_upper);

                            unit.populate_note_from_context(ctx_text.as_deref());
                            doc.add_unit(unit);
                            seq += 1;
                        }
                    }
                }
            } else {
                // Standard mode: extract all translatable string cells
                for (row_idx, cols) in rows_map {
                    for (col_letter, cell) in cols {
                        let cell_ref = format!("{}{}", col_letter, row_idx);
                        let unit_id = format!("{}!{}", sheet.name, cell_ref);

                        let mut unit = UnitIR::new(unit_id, seq, cell.text);
                        unit.context.sheet_name = Some(sheet.name.clone());
                        unit.context.cell_ref = Some(cell_ref);
                        unit.context.row = Some(row_idx);
                        unit.context.col = Some(col_letter);
                        unit.populate_note_from_context(None);

                        doc.add_unit(unit);
                        seq += 1;
                    }
                }
            }
        }

        Ok(doc)
    }
}

/// Helper that parses sheet and cell references from note like "[Sheet: Sheet1 | Cell: A2] ..."
fn parse_sheet_and_cell_from_note(note: &str) -> Option<(String, String)> {
    if !note.starts_with('[') {
        return None;
    }
    let end_bracket = note.find(']')?;
    let header_part = &note[1..end_bracket];

    let mut sheet = None;
    let mut cell = None;

    for piece in header_part.split('|') {
        let piece = piece.trim();
        if piece.starts_with("Sheet:") {
            sheet = Some(piece["Sheet:".len()..].trim().to_string());
        } else if piece.starts_with("Cell:") {
            cell = Some(piece["Cell:".len()..].trim().to_string());
        }
    }

    match (sheet, cell) {
        (Some(s), Some(c)) => Some((s, c)),
        _ => None,
    }
}

impl DocumentMerger for XlsxMerger {
    /// 翻訳結果を元の Excel ファイル（スケルトン）にマージします。
    ///
    /// ### 処理フロー
    /// 1. 各ユニットの `note`（例: `[Sheet1!A10]`）からシート名と元セル番地を取得。
    /// 2. 原文列（A）から訳文書き込み列（B）へのマッピングを行い、書き込み先セル番地（B10）を決定。
    /// 3. 該当ワークシートの XML をストリーミング再構築し、`<c r="B10" t="inlineStr">` 形式で訳文を挿入。
    /// 4. OpenXML 仕様に準拠するため、行内のセル順序（A列 -> B列 -> C列...）を厳密に保証。
    /// 5. 変更されたワークシートのみを新しい Zip ストリームへ上書き出力。
    fn merge<R: Read + Seek, W: Write + Seek>(
        &self,
        mut skeleton: R,
        translations: &HashMap<usize, TranslatedUnit>,
        writer: W,
        options: &MergeOptions,
    ) -> Result<(), FormatError> {
        let mut archive = ZipArchive::new(&mut skeleton)?;
        let sheets = parse_sheets(&mut archive);
        let mut replaced_entries: HashMap<String, Vec<u8>> = HashMap::new();

        // 書き込み対象セルマップ: (シート名, 対象セル番地) -> 確定訳文文字列
        let mut target_cells: HashMap<(String, String), String> = HashMap::new();

        let src_to_tgt_map: HashMap<String, String> = options
            .xlsx_source_cols
            .iter()
            .zip(options.xlsx_target_cols.iter())
            .map(|(s, t)| (s.to_ascii_uppercase(), t.to_ascii_uppercase()))
            .collect();

        for (_, unit) in translations {
            let tags = string_map_to_tags(&unit.placeholders);
            let restored = restore_placeholders(&unit.target, &tags);
            if restored.trim().is_empty() {
                continue;
            }

            if let Some(ref note) = unit.note {
                if let Some((sheet_name, src_cell)) = parse_sheet_and_cell_from_note(note) {
                    if let Some((src_col, row)) = parse_cell_ref(&src_cell) {
                        let tgt_col = src_to_tgt_map
                            .get(&src_col)
                            .cloned()
                            .unwrap_or_else(|| src_col.clone());
                        let tgt_cell = format!("{}{}", tgt_col, row);
                        target_cells.insert((sheet_name, tgt_cell), restored);
                    }
                }
            }
        }

        // 対象シートのXMLを再構築
        for sheet in sheets {
            let Some(sheet_xml) = read_zip_entry_to_string(&mut archive, &sheet.target_file)
            else {
                continue;
            };

            let rewritten_xml = rewrite_sheet_xml(&sheet_xml, &sheet.name, &target_cells)?;
            replaced_entries.insert(sheet.target_file, rewritten_xml);
        }

        skeleton.seek(std::io::SeekFrom::Start(0))?;
        rewrite_zip(skeleton, writer, &replaced_entries)?;

        Ok(())
    }
}

/// ワークシート XML を走査し、翻訳対象セルを `inlineStr` 形式で直接注入します。
fn rewrite_sheet_xml(
    xml_content: &str,
    sheet_name: &str,
    target_cells: &HashMap<(String, String), String>,
) -> Result<Vec<u8>, FormatError> {
    let mut reader = Reader::from_str(xml_content);
    reader.config_mut().trim_text(false);
    let mut out = Vec::with_capacity(xml_content.len());
    let mut writer = Writer::new(&mut out);

    let mut buf = Vec::new();
    let mut current_row: Option<u32> = None;
    let mut written_cols_in_row: BTreeSet<u32> = BTreeSet::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(ref e) => {
                let name = e.name();
                if name.as_ref() == b"row" {
                    written_cols_in_row.clear();
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"r" {
                            current_row = String::from_utf8_lossy(&attr.value).parse::<u32>().ok();
                        }
                    }
                    writer.write_event(Event::Start(e.clone()))?;
                } else if name.as_ref() == b"c" {
                    let mut cell_ref = String::new();
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"r" {
                            cell_ref = String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }

                    if let Some((col, _row)) = parse_cell_ref(&cell_ref) {
                        let col_num = col_to_num(&col);

                        // このセルを出力する前に、行内でこの列より前に配置されるべき未書き込みのターゲットセルを出力
                        if let Some(r) = current_row {
                            write_pending_target_cells_before(
                                &mut writer,
                                sheet_name,
                                r,
                                col_num,
                                &mut written_cols_in_row,
                                target_cells,
                            )?;
                        }

                        written_cols_in_row.insert(col_num);

                        // 既存セルに直接訳文を上書きする場合
                        let key = (sheet_name.to_string(), cell_ref.clone());
                        if let Some(trans_text) = target_cells.get(&key) {
                            write_inline_str_cell(&mut writer, &cell_ref, trans_text, e)?;
                            skip_until_end_tag(&mut reader, b"c")?;
                        } else {
                            writer.write_event(Event::Start(e.clone()))?;
                        }
                    } else {
                        writer.write_event(Event::Start(e.clone()))?;
                    }
                } else {
                    writer.write_event(Event::Start(e.clone()))?;
                }
            }
            Event::End(ref e) => {
                let name = e.name();
                if name.as_ref() == b"row" {
                    // 行の閉じタグの前に、行末に残っている未書き込みのターゲットセルをすべて出力
                    if let Some(r) = current_row {
                        write_pending_target_cells_before(
                            &mut writer,
                            sheet_name,
                            r,
                            u32::MAX,
                            &mut written_cols_in_row,
                            target_cells,
                        )?;
                    }
                    current_row = None;
                }
                writer.write_event(Event::End(e.clone()))?;
            }
            Event::Eof => break,
            other => {
                writer.write_event(other)?;
            }
        }
        buf.clear();
    }

    Ok(out)
}

/// `<c r="{cell_ref}" t="inlineStr"><is><t>{text}</t></is></c>` 形式のセル XML を出力します。
///
/// ### なぜ `inlineStr` を採用するのか？
/// OpenXML ではセル文字列の表現として `s` (SharedStrings参照) と `inlineStr` (セル内直接保持) の2種類が
/// 規格上正式にサポートされています。
/// `inlineStr` を使用することで、ファイル全体の `xl/sharedStrings.xml` を再構築・再インデックスすることなく、
/// 対象ワークシート単体で安全かつ極めて高速に翻訳テキストを注入できます。
/// Microsoft Excel および LibreOffice Calc の両方で警告なく完全に開くことができます。
fn write_inline_str_cell<W: Write>(
    writer: &mut Writer<W>,
    cell_ref: &str,
    text: &str,
    original_start: &BytesStart,
) -> Result<(), FormatError> {
    let mut new_c = BytesStart::new("c");
    let mut has_r = false;
    for attr in original_start.attributes().flatten() {
        if attr.key.as_ref() == b"r" {
            has_r = true;
            new_c.push_attribute(("r", cell_ref));
        } else if attr.key.as_ref() != b"t" {
            new_c.push_attribute((attr.key.as_ref(), attr.value.as_ref()));
        }
    }
    if !has_r {
        new_c.push_attribute(("r", cell_ref));
    }
    new_c.push_attribute(("t", "inlineStr"));

    writer.write_event(Event::Start(new_c))?;
    writer.write_event(Event::Start(BytesStart::new("is")))?;
    writer.write_event(Event::Start(BytesStart::new("t")))?;
    writer.write_event(Event::Text(BytesText::new(text)))?;
    writer.write_event(Event::End(BytesEnd::new("t")))?;
    writer.write_event(Event::End(BytesEnd::new("is")))?;
    writer.write_event(Event::End(BytesEnd::new("c")))?;
    Ok(())
}

/// 指定行 `row` において、`limit_col_num` よりも左側に位置し、まだ書き込まれていないターゲットセルを出力します。
/// これにより、元の原稿に行内の特定列が存在しなかった場合でも、正しい列順序を維持してセルが追加されます。
fn write_pending_target_cells_before<W: Write>(
    writer: &mut Writer<W>,
    sheet_name: &str,
    row: u32,
    limit_col_num: u32,
    written_cols: &mut BTreeSet<u32>,
    target_cells: &HashMap<(String, String), String>,
) -> Result<(), FormatError> {
    for ((s, cell_ref), trans_text) in target_cells {
        if s == sheet_name {
            if let Some((col, r)) = parse_cell_ref(cell_ref) {
                if r == row {
                    let col_num = col_to_num(&col);
                    if col_num < limit_col_num && !written_cols.contains(&col_num) {
                        let dummy_start = BytesStart::new("c");
                        write_inline_str_cell(writer, cell_ref, trans_text, &dummy_start)?;
                        written_cols.insert(col_num);
                    }
                }
            }
        }
    }
    Ok(())
}

/// Helper to skip events until the matching end tag is encountered.
fn skip_until_end_tag<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    tag_name: &[u8],
) -> Result<(), FormatError> {
    let mut depth = 0;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(ref e) if e.name().as_ref() == tag_name => {
                depth += 1;
            }
            Event::End(ref e) if e.name().as_ref() == tag_name => {
                if depth == 0 {
                    break;
                } else {
                    depth -= 1;
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
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
    fn test_extract_device_config_xlsx() {
        let test_path = Path::new("../../test_data/Device_Config.xlsx");
        if !test_path.exists() {
            eprintln!("Skipping test: test_data/Device_Config.xlsx not found");
            return;
        }

        let file = File::open(test_path).expect("Failed to open Device_Config.xlsx");
        let filter = XlsxFilter;
        let opts = FilterOptions {
            source_lang: "en".to_string(),
            target_lang: Some("ja".to_string()),
            xlsx_source_cols: vec!["A".to_string()],
            xlsx_target_cols: vec!["B".to_string()],
            xlsx_context_cols: vec!["C".to_string()],
            xlsx_skip_rows: vec![1],
            xlsx_sheet_pattern: Some("Sheet1".to_string()),
            ..Default::default()
        };

        let doc = filter.extract(file, "Device_Config.xlsx", &opts).unwrap();
        assert!(!doc.units.is_empty(), "Units should not be empty");

        // First extracted unit (row 2)
        let u1 = &doc.units[0];
        assert_eq!(u1.context.row, Some(2));
        assert_eq!(u1.context.col.as_deref(), Some("A"));
        assert_eq!(u1.source, "# Device Configuration Guide");
        assert_eq!(u1.target.as_deref(), Some("# デバイス設定ガイド"));

        // Unit for row 10 (which has context in col C)
        let u_row10 = doc
            .units
            .iter()
            .find(|u| u.context.row == Some(10))
            .expect("Row 10 not found");
        assert!(
            u_row10.note.as_ref().unwrap().contains("Added: Ver 2.0"),
            "Note should contain context from C10"
        );
        assert!(u_row10.note.as_ref().unwrap().contains("Cell: A10"));

        // Test Merge: provide translation for row 10
        let mut trans_map = HashMap::new();
        trans_map.insert(
            u_row10.seq,
            TranslatedUnit {
                idx: u_row10.seq,
                source: None,
                target: "ネットワーク設定画面テスト訳文".to_string(),
                placeholders: HashMap::new(),
                note: u_row10.note.clone(),
                is_sub: false,
            },
        );

        let merger = XlsxMerger;
        let skeleton_file = File::open(test_path).unwrap();
        let mut merged_out = Cursor::new(Vec::new());

        let merge_opts = MergeOptions {
            xlsx_source_cols: vec!["A".to_string()],
            xlsx_target_cols: vec!["B".to_string()],
            ..Default::default()
        };

        merger
            .merge(skeleton_file, &trans_map, &mut merged_out, &merge_opts)
            .unwrap();

        // Re-open merged xlsx and verify cell B10
        let mut merged_archive = ZipArchive::new(&mut merged_out).unwrap();
        let sheet1_xml =
            read_zip_entry_to_string(&mut merged_archive, "xl/worksheets/sheet1.xml").unwrap();
        assert!(
            sheet1_xml.contains("ネットワーク設定画面テスト訳文"),
            "Merged sheet1.xml must contain the translated text"
        );
        assert!(
            sheet1_xml.contains(r#"r="B10""#),
            "Merged sheet1.xml must have cell B10"
        );
    }
}

