use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::fs::File;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use suffolk_codec::{
    export_failsafe_csv, export_failsafe_txt, parse_xliff_12, write_xliff_12, ShWvData,
    TranslatedUnit,
};
use suffolk_formats::{
    DocxFilter, DocxMerger, DocumentFilter, DocumentMerger, FilterOptions, HtmlFilter, HtmlMerger,
    IdmlFilter, IdmlMerger, MarkdownFilter, MarkdownMerger, MergeOptions, PptxFilter, PptxMerger,
    TextFilter, TextMerger, WorksheetRule, XlsxFilter, XlsxMerger,
};
use suffolk_ir::{segment_document_ir, DocumentFormat};

#[derive(Parser)]
#[command(name = "suffolk", author, version, about = "High-performance Rust translation filter and merger for CAT tools")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Extract translatable segments from source documents into ShWvData JSON or XLIFF 1.2
    #[command(alias = "extract", alias = "-x")]
    Filter {
        /// Source document file path(s), comma-separated or space-separated list
        #[arg(required = true, num_args = 1..)]
        input: Vec<String>,

        /// Enable sentence-level segmentation (-seg / --seg)
        #[arg(long = "seg", short = 'g', alias = "segment")]
        seg: bool,

        /// Output file path (defaults to project.json / project(1).json, or <input>.xlf)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Export format: "shwv" (default) or "xliff" / "xlf"
        #[arg(long, default_value = "shwv")]
        to: String,

        /// Source language code
        #[arg(short = 's', long = "src-lang", default_value = "en")]
        src_lang: String,

        /// Target language code
        #[arg(short = 't', long = "trg-lang", default_value = "ja")]
        trg_lang: String,

        /// Disable tag minimization and preserve original raw run tags
        #[arg(long)]
        raw_tags: bool,

        /// Path to CSV configuration file for multi-language Excel
        #[arg(long)]
        xlsx_config: Option<PathBuf>,

        /// Source columns for Excel multi-language filtering (e.g. "A" or "A,D")
        #[arg(long)]
        xlsx_source: Option<String>,

        /// Target columns for Excel multi-language filtering (e.g. "B" or "B,E")
        #[arg(long)]
        xlsx_target: Option<String>,

        /// Context columns for Excel multi-language filtering (e.g. "C")
        #[arg(long)]
        xlsx_context: Option<String>,

        /// Header rows to skip for Excel (e.g. "1" or "1,2")
        #[arg(long)]
        xlsx_skip_rows: Option<String>,

        /// Sheet name pattern regex for Excel
        #[arg(long)]
        xlsx_sheet: Option<String>,
    },

    /// Merge translated segments from ShWvData JSON or XLIFF back into original document
    #[command(alias = "-m")]
    Merge {
        /// Translation file path(s) (project.json, .shwv.json, or .xlf). Defaults to project.json if omitted
        #[arg(num_args = 0..)]
        input: Vec<String>,

        /// Original document file path used as skeleton (only when single file is merged)
        #[arg(short = 's', long)]
        skeleton: Option<PathBuf>,

        /// Output translated document path (defaults to <name>.out.<ext>, or .csv / .txt for direct export)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Also export emergency failsafe CSV and TXT files alongside merged output
        #[arg(long)]
        failsafe: bool,

        /// Path to CSV configuration file for multi-language Excel
        #[arg(long)]
        xlsx_config: Option<PathBuf>,

        /// Source columns for Excel multi-language filtering (e.g. "A")
        #[arg(long)]
        xlsx_source: Option<String>,

        /// Target columns for Excel multi-language filtering (e.g. "B")
        #[arg(long)]
        xlsx_target: Option<String>,
    },

    /// Export translations to failsafe CSV (bilingual table) and TXT (target text only) for manual pasting
    #[command(alias = "failsafe", alias = "dump")]
    Export {
        /// Translation file path (.shwv.json or .xlf)
        #[arg(required = true, num_args = 1..)]
        input: Vec<String>,

        /// Export format: "all" (default: both csv & txt), "csv", or "txt"
        #[arg(long, default_value = "all")]
        to: String,

        /// Output file path (defaults to <name>.failsafe.csv or .txt)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Keep raw {@0} tags in TXT export instead of stripping them
        #[arg(long)]
        raw_tags: bool,
    },
}

fn main() -> Result<()> {
    // Tikal互換の `-seg` フラグを Clap が認識できる `--seg` へ正規化
    let args: Vec<String> = std::env::args()
        .map(|arg| {
            if arg == "-seg" {
                "--seg".to_string()
            } else {
                arg
            }
        })
        .collect();

    let cli = Cli::parse_from(args);

    match cli.command {
        Commands::Filter {
            input,
            seg,
            output,
            to,
            src_lang,
            trg_lang,
            raw_tags,
            xlsx_config,
            xlsx_source,
            xlsx_target,
            xlsx_context,
            xlsx_skip_rows,
            xlsx_sheet,
        } => {
            run_filter_orchestrator(
                input,
                seg,
                output,
                to,
                src_lang,
                trg_lang,
                raw_tags,
                xlsx_config,
                xlsx_source,
                xlsx_target,
                xlsx_context,
                xlsx_skip_rows,
                xlsx_sheet,
            )?;
        }
        Commands::Merge {
            input,
            skeleton,
            output,
            failsafe,
            xlsx_config,
            xlsx_source,
            xlsx_target,
        } => {
            run_merge_orchestrator(
                input,
                skeleton,
                output,
                failsafe,
                xlsx_config,
                xlsx_source,
                xlsx_target,
            )?;
        }
        Commands::Export {
            input,
            to,
            output,
            raw_tags,
        } => {
            run_export_orchestrator(input, output, to, raw_tags)?;
        }
    }

    Ok(())
}

/// 複数指定された引数（カンマ区切り文字列または複数引数）を展開してファイルパスリストを生成します。
fn expand_input_paths(inputs: &[String]) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for item in inputs {
        for part in item.split(',') {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                paths.push(PathBuf::from(trimmed));
            }
        }
    }
    if paths.is_empty() {
        bail!("No input files specified");
    }
    Ok(paths)
}

/// 指定ディレクトリ内に衝突しない ShWvData 保存パス（デフォルト: `project.json`）を決定します。
/// 既に存在する場合は `project(1).json`, `project(2).json` ... と番号をインクリメントして上書きを防止します。
pub fn get_unique_project_json_path(dir: &Path) -> PathBuf {
    let candidate = dir.join("project.json");
    if !candidate.exists() {
        return candidate;
    }

    let mut count = 1;
    loop {
        let candidate = dir.join(format!("project({}).json", count));
        if !candidate.exists() {
            return candidate;
        }
        count += 1;
    }
}

/// 抽出オーケストレーター（Filter Orchestrator）。
/// 単一ファイルまたは複数ファイルからテキストを抽出し、中間表現を生成。
/// `-seg` オプション指定時は文セグメンテーションを適用し、
/// XLIFF（1:1出力）または ShWvData（単一JSONに統合出力）を生成します。
fn run_filter_orchestrator(
    input_strs: Vec<String>,
    seg: bool,
    output: Option<PathBuf>,
    to: String,
    src_lang: String,
    trg_lang: String,
    raw_tags: bool,
    xlsx_config: Option<PathBuf>,
    xlsx_source: Option<String>,
    xlsx_target: Option<String>,
    xlsx_context: Option<String>,
    xlsx_skip_rows: Option<String>,
    xlsx_sheet: Option<String>,
) -> Result<()> {
    let inputs = expand_input_paths(&input_strs)?;

    let mut filter_opts = FilterOptions {
        source_lang: src_lang,
        target_lang: Some(trg_lang),
        minimize_tags: !raw_tags,
        ..Default::default()
    };

    if let Some(cfg_path) = xlsx_config {
        let rules = WorksheetRule::load_from_csv(cfg_path)?;
        if let Some(rule) = rules.first() {
            filter_opts.xlsx_sheet_pattern = Some(rule.sheet_pattern.clone());
            filter_opts.xlsx_source_cols = rule.source_cols.clone();
            filter_opts.xlsx_target_cols = rule.target_cols.clone();
            filter_opts.xlsx_context_cols = rule.context_cols.clone();
            filter_opts.xlsx_skip_rows = rule.skip_rows.clone();
        }
    }

    if let Some(s) = xlsx_source {
        filter_opts.xlsx_source_cols = parse_list(&s);
    }
    if let Some(t) = xlsx_target {
        filter_opts.xlsx_target_cols = parse_list(&t);
    }
    if let Some(c) = xlsx_context {
        filter_opts.xlsx_context_cols = parse_list(&c);
    }
    if let Some(r) = xlsx_skip_rows {
        filter_opts.xlsx_skip_rows = parse_num_list(&r);
    }
    if let Some(p) = xlsx_sheet {
        filter_opts.xlsx_sheet_pattern = Some(p);
    }

    let mut irs = Vec::new();

    for input in &inputs {
        let ext = input
            .extension()
            .and_then(|s| s.to_str())
            .context(format!("Input file has no extension: {:?}", input))?;

        let format = DocumentFormat::from_extension(ext)
            .context(format!("Unsupported file extension: .{}", ext))?;

        let file_name = input
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("document");
        let file = File::open(input).context(format!("Failed to open input file: {:?}", input))?;

        let raw_ir = match format {
            DocumentFormat::Docx => DocxFilter.extract(file, file_name, &filter_opts)?,
            DocumentFormat::Xlsx => XlsxFilter.extract(file, file_name, &filter_opts)?,
            DocumentFormat::Pptx => PptxFilter.extract(file, file_name, &filter_opts)?,
            DocumentFormat::PlainText => TextFilter.extract(file, file_name, &filter_opts)?,
            DocumentFormat::Markdown => MarkdownFilter.extract(file, file_name, &filter_opts)?,
            DocumentFormat::Html => HtmlFilter.extract(file, file_name, &filter_opts)?,
            DocumentFormat::Idml => IdmlFilter.extract(file, file_name, &filter_opts)?,
            DocumentFormat::Svg => {
                bail!("Format .{} is planned for future extension", ext);
            }
        };

        // セグメンテーション処理の適用
        let processed_ir = if seg {
            let seg_ir = segment_document_ir(&raw_ir);
            println!(
                "Segmented {:?}: {} blocks -> {} sentences",
                input,
                raw_ir.units.len(),
                seg_ir.units.len()
            );
            seg_ir
        } else {
            println!("Extracted {:?}: {} units", input, raw_ir.units.len());
            raw_ir
        };

        irs.push(processed_ir);
    }

    let is_xliff = to.eq_ignore_ascii_case("xliff") || to.eq_ignore_ascii_case("xlf");

    if is_xliff {
        // XLIFF 出力: 原稿 1 ファイルにつき 1 つの .xlf を書き出し（Tikal互換）
        for (idx, (input, ir)) in inputs.iter().zip(irs.iter()).enumerate() {
            let out_path = if inputs.len() == 1 && output.is_some() {
                output.clone().unwrap()
            } else {
                PathBuf::from(format!("{}.xlf", input.display()))
            };

            let xliff_xml = write_xliff_12(ir)?;
            std::fs::write(&out_path, xliff_xml)?;
            println!("[{}/{}] XLIFF written to: {:?}", idx + 1, inputs.len(), out_path);
        }
    } else {
        // ShWvData 出力: 複数ファイルを 1 つの JSON に統合（project.json 固定、衝突時は project(1).json）
        let out_path = if let Some(out) = output {
            out
        } else {
            let parent = inputs[0].parent().unwrap_or_else(|| Path::new("."));
            get_unique_project_json_path(parent)
        };

        let file_name_str = out_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("project.json");

        let mut shwv_data = ShWvData::from_multiple_irs(&irs, seg);
        shwv_data.meta.bilingual_path = file_name_str.to_string();

        let json_str = serde_json::to_string_pretty(&shwv_data)?;
        std::fs::write(&out_path, json_str)?;
        println!(
            "ShWvData dataset written to: {:?} (total units: {}, files: {})",
            out_path,
            shwv_data.body.units.len(),
            shwv_data.meta.files.len()
        );
    }

    Ok(())
}

/// マージオーケストレーター（Merge Orchestrator）。
/// 単一ファイルまたは複数ファイル（ShWvData / XLIFF）を受け取り、
/// 単一ファイルであっても複数ファイルであっても、常に統一された配列ループで安全に復元します。
fn run_merge_orchestrator(
    input_strs: Vec<String>,
    skeleton_opt: Option<PathBuf>,
    output_opt: Option<PathBuf>,
    failsafe: bool,
    xlsx_config: Option<PathBuf>,
    xlsx_source: Option<String>,
    xlsx_target: Option<String>,
) -> Result<()> {
    let inputs = if input_strs.is_empty() {
        let default_proj = PathBuf::from("project.json");
        if default_proj.exists() {
            vec![default_proj]
        } else {
            bail!("No translation input file specified, and 'project.json' was not found in the current directory.");
        }
    } else {
        expand_input_paths(&input_strs)?
    };

    let mut merge_opts = MergeOptions::default();
    if let Some(cfg_path) = xlsx_config {
        let rules = WorksheetRule::load_from_csv(cfg_path)?;
        if let Some(rule) = rules.first() {
            merge_opts.xlsx_source_cols = rule.source_cols.clone();
            merge_opts.xlsx_target_cols = rule.target_cols.clone();
        }
    }
    if let Some(s) = xlsx_source {
        merge_opts.xlsx_source_cols = parse_list(&s);
    }
    if let Some(t) = xlsx_target {
        merge_opts.xlsx_target_cols = parse_list(&t);
    }

    for input in &inputs {
        let input_bytes = std::fs::read(input)
            .context(format!("Failed to read translation file: {:?}", input))?;

        if input.to_string_lossy().ends_with(".json")
            || serde_json::from_slice::<serde_json::Value>(&input_bytes).is_ok()
        {
            // ShWvData からのマージ
            let shwv: ShWvData = serde_json::from_slice(&input_bytes)
                .context("Failed to parse ShWvData JSON format")?;

            // 常に shwv.meta.files の配列ループとして 1 本の統一処理を通す
            for file_info in &shwv.meta.files {
                let skeleton_path = if shwv.meta.files.len() == 1 && skeleton_opt.is_some() {
                    skeleton_opt.clone().unwrap()
                } else {
                    resolve_skeleton_path(&file_info.name, input, skeleton_opt.as_deref())?
                };

                let ext = skeleton_path
                    .extension()
                    .and_then(|s| s.to_str())
                    .context(format!("Skeleton file has no extension: {:?}", skeleton_path))?;
                let stem = skeleton_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("output");
                let parent = skeleton_path.parent().unwrap_or_else(|| Path::new("."));

                let out_path = if shwv.meta.files.len() == 1 && output_opt.is_some() {
                    output_opt.clone().unwrap()
                } else if let Some(ref out_dir) = output_opt {
                    if out_dir.is_dir() {
                        out_dir.join(format!("{}.out.{}", stem, ext))
                    } else {
                        parent.join(format!("{}.out.{}", stem, ext))
                    }
                } else {
                    parent.join(format!("{}.out.{}", stem, ext))
                };

                // このファイルのインデックス範囲でサブセグメント再結合済みのマップを取得
                let translations =
                    shwv.extract_recombined_translation_map(Some((file_info.start, file_info.end)));

                println!(
                    "Merging '{}' (units {}-{}, blocks: {}) into {:?}",
                    file_info.name,
                    file_info.start,
                    file_info.end,
                    translations.len(),
                    out_path
                );

                if failsafe {
                    let csv_path = parent.join(format!("{}.failsafe.csv", stem));
                    let txt_path = parent.join(format!("{}.failsafe.txt", stem));
                    let _ = std::fs::write(&csv_path, export_failsafe_csv(&translations, true)?);
                    let _ = std::fs::write(&txt_path, export_failsafe_txt(&translations, false));
                    println!("Failsafe files generated:\n  {:?}\n  {:?}", csv_path, txt_path);
                }

                merge_single_document(&skeleton_path, &out_path, &translations, &merge_opts)?;
            }
        } else {
            // XLIFF からのマージ
            let translations = parse_xliff_12(Cursor::new(&input_bytes))
                .map_err(|e| anyhow::anyhow!("Failed to parse XLIFF format: {}", e))?;

            let skeleton_path = if inputs.len() == 1 && skeleton_opt.is_some() {
                skeleton_opt.clone().unwrap()
            } else {
                let path_str = input.to_string_lossy();
                if path_str.ends_with(".xlf") {
                    PathBuf::from(&path_str[..path_str.len() - ".xlf".len()])
                } else if path_str.ends_with(".xliff") {
                    PathBuf::from(&path_str[..path_str.len() - ".xliff".len()])
                } else {
                    resolve_skeleton_path(&path_str, input, skeleton_opt.as_deref())?
                }
            };

            let ext = skeleton_path
                .extension()
                .and_then(|s| s.to_str())
                .context(format!("Skeleton file has no extension: {:?}", skeleton_path))?;
            let stem = skeleton_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("output");
            let parent = skeleton_path.parent().unwrap_or_else(|| Path::new("."));

            let out_path = if inputs.len() == 1 && output_opt.is_some() {
                output_opt.clone().unwrap()
            } else {
                parent.join(format!("{}.out.{}", stem, ext))
            };

            println!(
                "Merging XLIFF {:?} (blocks: {}) into {:?}",
                input,
                translations.len(),
                out_path
            );

            if failsafe {
                let csv_path = parent.join(format!("{}.failsafe.csv", stem));
                let txt_path = parent.join(format!("{}.failsafe.txt", stem));
                let _ = std::fs::write(&csv_path, export_failsafe_csv(&translations, true)?);
                let _ = std::fs::write(&txt_path, export_failsafe_txt(&translations, false));
                println!("Failsafe files generated:\n  {:?}\n  {:?}", csv_path, txt_path);
            }

            merge_single_document(&skeleton_path, &out_path, &translations, &merge_opts)?;
        }
    }

    Ok(())
}

/// スケルトンファイルの探索解決関数
fn resolve_skeleton_path(
    file_name: &str,
    translation_file: &Path,
    skeleton_hint: Option<&Path>,
) -> Result<PathBuf> {
    let direct = PathBuf::from(file_name);
    if direct.exists() {
        return Ok(direct);
    }

    if let Some(parent) = translation_file.parent() {
        let relative_to_trans = parent.join(file_name);
        if relative_to_trans.exists() {
            return Ok(relative_to_trans);
        }
    }

    if let Some(hint) = skeleton_hint {
        if hint.is_dir() {
            let in_hint = hint.join(file_name);
            if in_hint.exists() {
                return Ok(in_hint);
            }
        }
    }

    Ok(direct)
}

/// 単一ドキュメントのマージ実行および例外発生時の緊急フェイルセーフ保護
fn merge_single_document(
    skeleton_path: &Path,
    out_path: &Path,
    translations: &HashMap<usize, TranslatedUnit>,
    merge_opts: &MergeOptions,
) -> Result<()> {
    let ext = skeleton_path
        .extension()
        .and_then(|s| s.to_str())
        .context(format!("Skeleton file has no extension: {:?}", skeleton_path))?;
    let format = DocumentFormat::from_extension(ext)
        .context(format!("Unsupported skeleton file extension: .{}", ext))?;

    let parent = skeleton_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = skeleton_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    let skeleton_file = match File::open(skeleton_path) {
        Ok(f) => f,
        Err(err) => {
            let emergency_csv = parent.join(format!("{}.failsafe.csv", stem));
            let emergency_txt = parent.join(format!("{}.failsafe.txt", stem));
            let _ = std::fs::write(&emergency_csv, export_failsafe_csv(translations, true)?);
            let _ = std::fs::write(&emergency_txt, export_failsafe_txt(translations, false));
            eprintln!(
                "[FAILSAFE] Failed to open skeleton file {:?} ({:#}). Emergency files generated:\n  {:?}\n  {:?}",
                skeleton_path, err, emergency_csv, emergency_txt
            );
            return Err(err.into());
        }
    };

    let out_file = File::create(out_path)
        .context(format!("Failed to create output file: {:?}", out_path))?;

    let merge_result = match format {
        DocumentFormat::Docx => DocxMerger.merge(skeleton_file, translations, out_file, merge_opts),
        DocumentFormat::Xlsx => XlsxMerger.merge(skeleton_file, translations, out_file, merge_opts),
        DocumentFormat::Pptx => PptxMerger.merge(skeleton_file, translations, out_file, merge_opts),
        DocumentFormat::PlainText => TextMerger.merge(skeleton_file, translations, out_file, merge_opts),
        DocumentFormat::Markdown => MarkdownMerger.merge(skeleton_file, translations, out_file, merge_opts),
        DocumentFormat::Html => HtmlMerger.merge(skeleton_file, translations, out_file, merge_opts),
        DocumentFormat::Idml => IdmlMerger.merge(skeleton_file, translations, out_file, merge_opts),
        DocumentFormat::Svg => {
            bail!("Format .{} is planned for future extension", ext);
        }
    };

    if let Err(err) = merge_result {
        let emergency_csv = parent.join(format!("{}.failsafe.csv", stem));
        let emergency_txt = parent.join(format!("{}.failsafe.txt", stem));
        let _ = std::fs::write(&emergency_csv, export_failsafe_csv(translations, true)?);
        let _ = std::fs::write(&emergency_txt, export_failsafe_txt(translations, false));
        eprintln!(
            "[FAILSAFE] Merge failed for {:?} ({:#}). Emergency files generated:\n  {:?}\n  {:?}",
            skeleton_path, err, emergency_csv, emergency_txt
        );
        return Err(err.into());
    }

    println!("Successfully merged into: {:?}", out_path);
    Ok(())
}

/// 翻訳ファイル（.shwv.json または .xlf）から翻訳データを読み込みます。
fn load_translations_from_file(input: &Path) -> Result<HashMap<usize, TranslatedUnit>> {
    let input_bytes = std::fs::read(input)
        .context(format!("Failed to read translation file: {:?}", input))?;

    if input.to_string_lossy().ends_with(".json")
        || serde_json::from_slice::<serde_json::Value>(&input_bytes).is_ok()
    {
        let shwv: ShWvData = serde_json::from_slice(&input_bytes)
            .context("Failed to parse ShWvData JSON format")?;
        Ok(shwv.extract_translation_map())
    } else {
        parse_xliff_12(Cursor::new(&input_bytes))
            .map_err(|e| anyhow::anyhow!("Failed to parse XLIFF 1.2 format: {}", e))
    }
}

/// エクスポートオーケストレーター（Export Orchestrator）。
fn run_export_orchestrator(
    input_strs: Vec<String>,
    output_opt: Option<PathBuf>,
    to: String,
    raw_tags: bool,
) -> Result<()> {
    let inputs = expand_input_paths(&input_strs)?;

    for input in inputs {
        let translations = load_translations_from_file(&input)?;
        println!("Loaded {} translations from {:?}", translations.len(), input);

        let base_stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("translations");
        let clean_stem = if base_stem.ends_with(".shwv") {
            &base_stem[..base_stem.len() - ".shwv".len()]
        } else {
            base_stem
        };
        let parent = input.parent().unwrap_or_else(|| Path::new("."));

        let target = to.to_ascii_lowercase();
        if target == "csv" {
            let out_path = output_opt.clone().unwrap_or_else(|| parent.join(format!("{}.failsafe.csv", clean_stem)));
            let csv_str = export_failsafe_csv(&translations, true)?;
            std::fs::write(&out_path, csv_str)?;
            println!("Exported failsafe CSV to: {:?}", out_path);
        } else if target == "txt" {
            let out_path = output_opt.clone().unwrap_or_else(|| parent.join(format!("{}.failsafe.txt", clean_stem)));
            let txt_str = export_failsafe_txt(&translations, raw_tags);
            std::fs::write(&out_path, txt_str)?;
            println!("Exported failsafe TXT (target-only) to: {:?}", out_path);
        } else {
            let csv_path = parent.join(format!("{}.failsafe.csv", clean_stem));
            let txt_path = parent.join(format!("{}.failsafe.txt", clean_stem));

            let csv_str = export_failsafe_csv(&translations, true)?;
            std::fs::write(&csv_path, csv_str)?;

            let txt_str = export_failsafe_txt(&translations, raw_tags);
            std::fs::write(&txt_path, txt_str)?;

            println!("Exported failsafe CSV to: {:?}", csv_path);
            println!("Exported failsafe TXT (target-only) to: {:?}", txt_path);
        }
    }

    Ok(())
}

fn parse_list(s: &str) -> Vec<String> {
    s.split(|c| c == ',' || c == ';' || c == ' ')
        .map(|col| col.trim().to_ascii_uppercase())
        .filter(|col| !col.is_empty())
        .collect()
}

fn parse_num_list(s: &str) -> Vec<u32> {
    s.split(|c| c == ',' || c == ';' || c == ' ')
        .map(|num| num.trim().parse::<u32>().ok())
        .flatten()
        .collect()
}
