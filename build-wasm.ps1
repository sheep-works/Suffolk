# Suffolk WASM Build Script
Write-Host "Building Suffolk WASM for Web (Browser)..." -ForegroundColor Cyan
wasm-pack build crates/suffolk-wasm --target web --out-dir ../../pkg/web

Write-Host "Building Suffolk WASM for Node.js (VS Code Extension)..." -ForegroundColor Cyan
wasm-pack build crates/suffolk-wasm --target nodejs --out-dir ../../pkg/node

Write-Host "Running WASM Test Suite with Node.js..." -ForegroundColor Green
node test_wasm.js

Write-Host "`nAll WASM targets successfully built and verified!" -ForegroundColor Green
