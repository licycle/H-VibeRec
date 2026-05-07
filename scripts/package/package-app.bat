@echo off
setlocal enabledelayedexpansion

set "SCRIPT_DIR=%~dp0"
pushd "%SCRIPT_DIR%..\.."

echo.
echo Building hit-vvc Application...
echo ========================================

if /I not "%PROCESSOR_ARCHITECTURE%"=="AMD64" (
    echo ERROR: bundled offline ASR runtime packaging currently supports Windows x64 only.
    exit /b 1
)

echo.
echo Checking prerequisites...
where npm >nul 2>nul
if %errorlevel% neq 0 (
    echo ERROR: npm is not installed. Please install Node.js first.
    exit /b 1
)
echo OK: npm is available.

where cargo >nul 2>nul
if %errorlevel% neq 0 (
    echo ERROR: cargo is not installed. Please install Rust first.
    exit /b 1
)
echo OK: cargo is available.

where rustc >nul 2>nul
if %errorlevel% neq 0 (
    echo ERROR: rustc is not installed. Please install Rust first.
    exit /b 1
)
echo OK: rustc is available.

echo.
echo Installing frontend dependencies...
call npm install
if %errorlevel% neq 0 (
    echo ERROR: failed to install frontend dependencies.
    exit /b 1
)

echo.
echo Preparing bundled ASR runtime...
call npm run runtime:ensure
if %errorlevel% neq 0 (
    echo ERROR: failed to prepare bundled ASR runtime.
    exit /b 1
)
call npm run runtime:check
if %errorlevel% neq 0 (
    echo ERROR: bundled ASR runtime check failed.
    exit /b 1
)

echo.
echo Checking package readiness...
call npm run package:check
if %errorlevel% neq 0 (
    echo ERROR: package readiness check failed.
    exit /b 1
)

echo.
echo Cleaning previous bundle output...
if exist src-tauri\target\release\bundle rmdir /s /q src-tauri\target\release\bundle
if exist src-tauri\target\release\_up_ rmdir /s /q src-tauri\target\release\_up_

echo.
echo Building Tauri application...
call npm run tauri build
if %errorlevel% neq 0 (
    echo ERROR: failed to build Tauri application.
    exit /b 1
)

echo.
echo Build completed.
echo Built applications can be found in:
echo   - src-tauri\target\release\bundle\
echo.
dir src-tauri\target\release\bundle\
popd
