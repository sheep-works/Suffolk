// ShWvData type definitions for the SheepCombWeb SPA
// Ported from sample/converter/type.d.ts

/**
 * Basic translation pair item representing a single segment.
 */
export interface TranslationPair {
  /** 1-based segment index number */
  idx: number
  /** Source language text segment */
  src: string
  /** Target language translation text segment. Primary field for editing */
  tgt: string
  /** Optional translator note or annotation */
  note?: string
  /** Flag indicating whether this is a subordinate / joined segment */
  isSub?: boolean
  /** Translation status code (e.g. 0: untranslated, 1: draft, 2: completed) */
  status?: number
  /** Map of placeholder token indices to original tags (e.g., { 0: "{0}" }) */
  placeholders?: Record<number, string>
}

/**
 * Translation pair item associated with a specific file.
 */
export interface TranslationPairWithFile extends TranslationPair {
  /** Source file name or path */
  file: string
}

/**
 * Format definition and version header for ShWv data.
 */
export interface ShWvDefine {
  /** Fixed dataset header name */
  name: 'SHWV_DATA'
  /** ShWv format schema version */
  version: '1.5' | '1.4' | '1.3' | '1.2' | '1.1' | '1.0'
}

/**
 * File range information within a multi-file ShWv dataset.
 */
export interface ShWvFileInfo {
  /** Name of the contained file */
  name: string
  /** Starting segment index (inclusive) */
  start: number
  /** Ending segment index (inclusive) */
  end: number
}

/**
 * Workflow configuration details for ShWv dataset.
 */
export interface ShWvWorkflow {
  /** Workflow step index (e.g., 0: Extract in SheepComb Web, 1: Translation in SheepWeave, 2: Review, etc.) */
  index: number
  /** Role performing this workflow step (e.g., "Extract", "Translation", "Review") */
  role: string
  /** Tool or system name executing the workflow (e.g., "SheepComb Web", "Sheep") */
  name: string
  /**
   * Segmentation mode used for extraction:
   * 
   * | 値 | 説明・挙動 |
   * | :--- | :--- |
   * | `line` (デフォルト) | **行単位**。<br>テキストパース時に改行でセグメント分割します（`splitByNewline = true`）。Tikal 抽出時は `-seg` フラグを付けません。 |
   * | `seg` | **センテンス単位（文分割）**。<br>Tikal による XLIFF 抽出時に `-seg` フラグを付与し、SRX等のルールに基づいた文単位のセグメンテーションを行います。 |
   * | `raw` | **改行分割なし（生データ保持）**。<br>パース時に改行による自動分割を行いません（`splitByNewline = false`）。 |
   */
  segmentation?: 'line' | 'seg' | 'raw' | string
  /**
   * Scope: line / idx range specification (e.g. "100-500", "1-100, 201-300").
   * If omitted, all units in dataset are scoped.
   */
  range?: string
  /**
   * Scope: target file name(s) (e.g. "Chapter1.xlsx, Chapter2.xlsx" or array of file names).
   * If specified, units belonging to these files are scoped.
   */
  files?: string[] | string
}

/**
 * Metadata for the ShWv dataset including languages, files, and workflow.
 */
export interface ShWvMeta {
  /** Path to bilingual file or repository */
  bilingualPath: string
  /** Array of contained file info objects */
  files: ShWvFileInfo[]
  /** Source language BCP-47 / ISO code (e.g., "ja", "en") */
  sourceLang: string
  /** Target language BCP-47 / ISO code (e.g., "en", "zh") */
  targetLang: string
  /** Optional project name */
  projectName?: string
  /** Paths to Translation Memory (TM) files used */
  tmFiles?: string[]
  /** Paths to Termbase (TB) files used */
  tbFiles?: string[]
  /** Workflow configuration details */
  workflow?: ShWvWorkflow
}

/**
 * Main dataset body containing segments and extracted glossary terms.
 */
export interface ShWvBody {
  /** List of core translation units (segments) */
  units: ShWvUnit[]
  /** List of extracted term pairs (source -> target) */
  terms: { src: string; tgt: string }[]
}

/**
 * Core translation unit representing a single segment in a document.
 */
export interface ShWvUnit {
  /** 1-based segment index */
  idx: number
  /** Source language text */
  src: string
  /** Pre-translation / Machine Translation initial text */
  pre: string
  /** Active target translation text. Main field for batch editing and LLM operations */
  tgt: string
  /** Optional translator note */
  note?: string
  /** Optional user-defined memo or remark */
  memo?: string
  /** Flag indicating if segment is subordinate (e.g., part of a split segment) */
  isSub?: boolean
  /** Segment status code */
  status?: number
  /** Flag indicating if Post-Editing (PE) reference was used */
  isPeRef?: boolean
  /** Reference data (TM matches, TB matches, quoted ranges) */
  ref: ShWvRef
  /** Map of placeholder index to placeholder tag string */
  placeholders?: Record<number, string>
}

/**
 * Container for reference matches (TM and TB) and quoting information.
 */
export interface ShWvRef {
  /** List of Translation Memory (TM) match results */
  tms: ShWvRefTm[]
  /** List of Termbase (TB) match results */
  tb: ShWvRefTb[]
  /** Quoted text ranges as [start, end] tuple arrays */
  quoted: [number, number][]
  /** Indices of 100% exact matches in quoted ranges */
  quoted100: number[]
}

/**
 * Individual Translation Memory (TM) match result for a segment.
 */
export interface ShWvRefTm {
  /** TM entry index */
  idx: number
  /** Source text stored in TM */
  src: string
  /** Difference diff markup between unit source and TM source */
  diff?: string
  /** Target translation text stored in TM (Do NOT confuse with unit's active tgt) */
  tgt: string
  /** Match similarity ratio (0 to 100) */
  ratio: number
  /** Optional freeze flag preventing auto-updates */
  freeze?: boolean
  /** Source file name of the TM entry */
  file?: string
}

/**
 * Individual Termbase (TB) match result for a segment.
 */
export interface ShWvRefTb {
  /** Matched source term */
  src: string
  /** Candidate target translations for the term */
  tgts: string[]
  /** Forbidden target translations for the term (do-not-use terms) */
  forbiddens?: string[]
  /** Usage note or term definition */
  note?: string
  /** Source TB file name */
  file?: string
}

/**
 * Status tracking for individual project files in pipeline processing.
 */
export interface ProjectFileStatus {
  /** Source file path */
  source: string
  /** Intermediate XLIFF file path (if generated) */
  xliff: string | null
  /** Pipeline processing state */
  status: 'extracted' | 'translated' | 'merged' | 'error'
  /** Error message if status is 'error' */
  errorMsg?: string
}

/**
 * Logical group of files in a project.
 */
export interface ProjectGroup {
  /** Filter rule string for file group */
  filter: string
  /** List of file statuses in this group */
  files: ProjectFileStatus[]
}

/**
 * Summary statistics for a translation project.
 */
export interface ProjectStats {
  /** Total segment count */
  segments: number
  /** Untranslated segment count */
  untranslated: number
  /** Count of QA warnings */
  qaWarnings: number
  /** Count of matched terms */
  termsMatched: number
}

/**
 * Overall project information and pipeline metadata.
 */
export interface ProjectInfo {
  /** Project metadata version */
  version: number
  /** Name of the project */
  projectName: string
  /** Source language code */
  sourceLanguage: string
  /** Target language code */
  targetLanguage: string
  /** Array of source file paths */
  sourceFiles: string[]
  /** File group list for Okapi pipeline processing */
  okapi: ProjectGroup[]
  /** ISO timestamp of last preparation step */
  lastPreparedAt?: string
  /** Project progress statistics */
  stats?: ProjectStats
}

/**
 * Complete root structure of a SheepComb / ShWv JSON dataset file.
 */
export interface ShWvData {
  /** Data format header and version */
  define: ShWvDefine
  /** Project and document metadata */
  meta: ShWvMeta
  /** Content body containing units and terms */
  body: ShWvBody
  /** Optional project pipeline info and stats */
  projectInfo?: ProjectInfo
}

/**
 * Simple source/target pair for export data.
 */
export interface ExportPair {
  /** Source text */
  src: string
  /** Target text */
  tgt: string
}

/**
 * JSONL export item structure with edit history.
 */
export interface ChunkedJsonlItem {
  /** Segment index */
  index: number
  /** Source text */
  src: string
  /** Target text */
  tgt: string
  /** Edit history of source/target pairs */
  history: ExportPair[]
}

/**
 * Supported data export/management types.
 */
export type ManagedDataType = 'UNITS' | 'TMS' | 'TBS' | 'JSONL' | 'JSONL_CHUNKED' | 'CSV' | 'SPLIT_BY_FILE' | 'SPLIT_BY_LENGTH'

/**
 * Filter mode for Do-Not-Translate (DNT) items.
 */
export type DntFilterType = 'digit' | 'eng' | 'digit eng' | null

/**
 * Processing configuration options.
 */
export interface ProcessorOptions {
  /** Filter duplicate entries */
  toFilterDuplicate?: boolean
  /** Level of duplication filtering */
  filterLevel?: "SRC" | "SRC_TGT" | "SRC_TGT_NOTE"
  /** Do-Not-Translate filtering mode */
  toFilterDnt?: DntFilterType
  /** Filter locked segments */
  toFilterLock?: boolean
}

/**
 * Options selecting which fields to include in export chunks.
 */
export interface ChunkOptions {
  /** Include source text */
  src?: boolean
  /** Include target text */
  tgt?: boolean
  /** Include notes */
  note?: boolean
  /** Include user-defined memo */
  memo?: boolean
  /** Include edit history */
  history?: boolean
  /** Include terms */
  terms?: boolean
  /** Format of chunks ('jsonl' or 'toon') */
  format?: 'jsonl' | 'toon'
  /** Use TOON format (experimental) */
  useToon?: boolean
}

/**
 * Helper function to instantiate ChunkOptions with defaults.
 */
export function createChunkOptions(options?: ChunkOptions): Required<ChunkOptions> {
  const useToon = options?.useToon ?? (options?.format === 'toon')
  return {
    src: options?.src ?? false,
    tgt: options?.tgt ?? false,
    note: options?.note ?? false,
    memo: options?.memo ?? false,
    history: options?.history ?? false,
    terms: options?.terms ?? false,
    format: useToon ? 'toon' : (options?.format ?? 'jsonl'),
    useToon,
  }
}

/**
 * Quality Assurance (QA) issue category.
 */
export type QaIssueType = 'Number' | 'Tag' | 'Term' | 'Consistency' | 'UnmodifiedPe'

/**
 * Individual QA issue detected during segment checking.
 */
export interface QaIssue {
  /** Type of issue detected */
  issue_type: QaIssueType
  /** 1-based segment index */
  idx: number
  /** Description message of the issue */
  message: string
}

/**
 * Configuration options for QA checks.
 */
export interface QaConfig {
  /** Check number mismatch between source and target */
  check_numbers?: boolean
  /** Check inline tags/placeholders mismatch */
  check_tags?: boolean
  /** Check target glossary/termbase usage */
  check_terms?: boolean
  /** Check translation consistency for 100% matching segments */
  check_consistency?: boolean
  /** Check unmodified pre-translation / PE leakage on referenced segments */
  check_unmodified_pe?: boolean
}

/**
 * Options for rebuilding bilingual files (XLIFF/MXLIFF/MQXLIFF).
 */
export interface BuildOptions {
  /**
   * If true, lock duplicate or high-similarity segments.
   */
  lockDuplicates?: boolean
  /**
   * Match similarity ratio threshold (0-100) to apply lock. Defaults to 100.
   */
  lockThreshold?: number
}

/**
 * Options for batch applying terms/glossaries.
 */
export interface ApplyTermsOptions {
  /**
   * Target field to apply replacements ('tgt' or 'pre'). Defaults to 'tgt'.
   */
  targetField?: 'tgt' | 'pre'
  /**
   * If the target field is empty, whether to use source text as base. Defaults to true.
   */
  fallbackToSrc?: boolean
  /**
   * Whether to enforce word boundaries on alphanumeric terms to avoid partial matching (e.g. 'admin' -> 'administrator'). Defaults to true.
   */
  matchWholeWord?: boolean
}

