@echo off
rem This script checks for the necessary system dependencies for the hit-vvc Tauri application on Windows.

echo.
echo Checking for required system dependencies...
echo ================================================

set "dependencies_ok=true"

rem --- 1. Check for Rust (required by Tauri) ---
echo Checking for Rust...
where rustc >nul 2>nul
if %errorlevel% neq 0 (
    echo X Rust is not installed, but it's required for Tauri.
    echo   Please install it from: https://www.rust-lang.org/tools/install
    set "dependencies_ok=false"
) else (
    echo V Rust is already installed.
)

rem --- 2. Check for Microsoft C++ Build Tools ---
echo.
echo Checking for Microsoft C++ Build Tools...
echo   Please ensure 'Microsoft C++ Build Tools' are installed.
echo   You can get them with the Visual Studio Installer here:
echo   https://visualstudio.microsoft.com/visual-cpp-build-tools/
echo   (This script cannot automatically verify this installation.)

rem --- Final Check ---
echo.
if "%dependencies_ok%"=="false" (
    echo.
    echo X Some system dependencies are missing.
    echo   Please install them manually based on the instructions above and restart your terminal.
    exit /b 1
) else (
    echo V All required system dependencies appear to be in place.
)
