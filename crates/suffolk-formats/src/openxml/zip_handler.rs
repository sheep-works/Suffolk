use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// Zip アーカイブから特定のエントリを UTF-8 文字列として読み込みます。
pub fn read_zip_entry_to_string<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Option<String> {
    if let Ok(mut file) = archive.by_name(name) {
        let mut s = String::new();
        if file.read_to_string(&mut s).is_ok() {
            return Some(s);
        }
    }
    None
}

/// Zip アーカイブから特定のエントリを生バイト列として読み込みます。
pub fn read_zip_entry_bytes<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Option<Vec<u8>> {
    if let Ok(mut file) = archive.by_name(name) {
        let mut bytes = Vec::new();
        if file.read_to_end(&mut bytes).is_ok() {
            return Some(bytes);
        }
    }
    None
}

/// 元の Zip アーカイブ（スケルトン）を読み込み、指定されたエントリ（`replaced_entries`）を
/// 新しいバイト列に差し替えて新しい Zip アーカイブを出力します。
///
/// 変更のない画像・埋め込みオブジェクト・スタイル・リレーションファイル等は
/// 元の圧縮形式およびパーミッションを維持したまま高速にストリーミングコピーされます。
pub fn rewrite_zip<R: Read + Seek, W: Write + Seek>(
    skeleton: R,
    mut target: W,
    replaced_entries: &HashMap<String, Vec<u8>>,
) -> Result<(), zip::result::ZipError> {
    let mut zip_in = ZipArchive::new(skeleton)?;
    let mut zip_out = ZipWriter::new(&mut target);

    for i in 0..zip_in.len() {
        let mut entry = zip_in.by_index(i)?;
        let name = entry.name().to_string();

        let options = SimpleFileOptions::default()
            .compression_method(entry.compression())
            .unix_permissions(entry.unix_mode().unwrap_or(0o644));

        zip_out.start_file(&name, options)?;

        if let Some(new_bytes) = replaced_entries.get(&name) {
            zip_out.write_all(new_bytes)?;
        } else {
            std::io::copy(&mut entry, &mut zip_out)?;
        }
    }

    zip_out.finish()?;
    Ok(())
}
