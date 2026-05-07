@echo off
echo "Building debug version for Windows troubleshooting..."

pushd "%~dp0..\.."

REM Clean previous builds
if exist dist rmdir /s /q dist
if exist src-tauri\target\release rmdir /s /q src-tauri\target\release

REM Set debug environment
set TAURI_DEBUG=true
set RUST_LOG=debug

REM Build with debug info
npm run build
npm run tauri build -- --debug

echo "Debug build completed. Check console output for errors."
echo "Debug executable will be in src-tauri/target/debug/"
pause
popd
