const fs = require('fs');
const path = require('path');
const wasm = require('./pkg/node/suffolk_wasm.js');

console.log('--- Suffolk WASM Test ---');
console.log('Version:', wasm.suffolk_version());

// 1. サポートフォーマット一覧
const formats = wasm.suffolk_vscode_get_supported_formats();
console.log('Supported Formats:', formats.map(f => f.name).join(', '));

// 2. 文分割テスト (VS Code 便利機能)
const testText = "これは最初の文です。これは二番目の文です！本当に？はい。";
const sentences = wasm.suffolk_vscode_segment_text(testText);
console.log('Segmented sentences:', sentences);

// 3. タグ整合性検証テスト
const tagCheck = wasm.suffolk_vscode_validate_tags("Hello {@0}world{@1}!", "こんにちは {@0}世界{@1}!");
console.log('Tag validation (valid):', tagCheck);
const tagCheckBroken = wasm.suffolk_vscode_validate_tags("Hello {@0}world{@1}!", "こんにちは 世界!");
console.log('Tag validation (broken):', tagCheckBroken);

// 4. DOCX 抽出 & マージテスト
const docxPath = path.join(__dirname, 'test_data', 'デバイスガイド.docx');
if (fs.existsSync(docxPath)) {
    const docxBytes = fs.readFileSync(docxPath);
    console.log('\n--- Testing DOCX Filter ---');
    const options = {
        seg: true,
        srcLang: 'ja',
        trgLang: 'en'
    };
    const shwv = wasm.suffolk_browser_extract(docxBytes, 'デバイスガイド.docx', options);
    console.log('DOCX Extracted units:', shwv.body.units.length);
    console.log('Sample unit 0:', shwv.body.units[0]);

    // マージテスト
    console.log('\n--- Testing DOCX Merge ---');
    // 最初のユニットの訳文を入れてみる
    shwv.body.units[0].tgt = 'Device Guide (Translated by WASM)';
    const mergedDocxBytes = wasm.suffolk_browser_merge(docxBytes, 'デバイスガイド.docx', shwv, {});
    console.log('Merged DOCX byte length:', mergedDocxBytes.length);
    console.log('DOCX Merge successful!');
}

// 5. Excel (XLSX) 多言語抽出 & マージテスト
const xlsxPath = path.join(__dirname, 'test_data', 'Device_Config.xlsx');
if (fs.existsSync(xlsxPath)) {
    const xlsxBytes = fs.readFileSync(xlsxPath);
    console.log('\n--- Testing XLSX Filter (Multi-language) ---');
    const xlsxOptions = {
        xlsxSource: 'A',
        xlsxTarget: 'B',
        xlsxContext: 'C',
        xlsxSkipRows: 1
    };
    const shwvXlsx = wasm.suffolk_vscode_extract(xlsxBytes, 'Device_Config.xlsx', xlsxOptions);
    console.log('XLSX Extracted units:', shwvXlsx.body.units.length);
    console.log('Sample unit 0:', shwvXlsx.body.units[0]);

    // フェイルセーフ救出テスト
    console.log('\n--- Testing Failsafe Export ---');
    const failsafe = wasm.suffolk_vscode_export_failsafe(shwvXlsx);
    console.log('Failsafe CSV length:', failsafe.csv.length);
    console.log('Failsafe TXT length:', failsafe.txt.length);
}

// 6. 複数ファイル一括バッチ抽出テスト
console.log('\n--- Testing Batch Extract ---');
const batchFiles = [
    { name: 'test1.txt', data: Buffer.from('Hello world.\nThis is a test.') },
    { name: 'test2.md', data: Buffer.from('# Heading\nMarkdown paragraph.') }
];
const batchResult = wasm.suffolk_browser_extract_batch(batchFiles, { seg: true });
console.log('Batch Extracted files:', batchResult.meta.files.length);
console.log('Batch Extracted total units:', batchResult.body.units.length);

console.log('\n>>> All WASM tests completed successfully! <<<');
