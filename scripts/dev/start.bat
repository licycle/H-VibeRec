@echo off
rem This script starts the development server for hit-vvc on Windows.

set "SCRIPT_DIR=%~dp0"
pushd "%SCRIPT_DIR%..\.."

echo.
echo Starting hit-vvc Development Server...
echo ================================================

rem --- 1. Dependency Check ---
echo.
echo Running dependency check...
call "%SCRIPT_DIR%..\setup\install-dependencies.bat"
if %errorlevel% neq 0 (
    echo.
    echo X Dependency check failed. Please resolve the issues above.
    popd
    exit /b 1
)
echo V System dependencies check passed.
echo.

rem --- 2. Node.js Dependency Check ---
if not exist "node_modules" (
    echo Installing Node.js dependencies...
    npm install
) else (
    echo V Node modules are already installed.
)

rem --- 3. Prepare Bundled ASR Runtime ---
echo.
echo Preparing bundled ASR runtime...
npm run runtime:ensure
if %errorlevel% neq 0 (
    echo X Failed to prepare bundled ASR runtime.
    exit /b 1
)
echo V Bundled ASR runtime is ready.

rem --- 4. Port Management ---
echo.
echo Checking if port 1420 is available...
for /f "tokens=5" %%a in ('netstat -aon ^| findstr ":1420" ^| findstr "LISTENING"') do (
    echo W Port 1420 is already in use by PID %%a.
    echo   This script cannot automatically free the port.
    echo   Please close the application using it and try again.
    exit /b 1
)
echo V Port 1420 is free.

rem --- 5. Start Development Server ---
echo.
echo Starting Tauri development server...
echo   - Frontend will be available at: http://localhost:1420
echo   - Tauri app will open automatically
echo.
echo To stop the server, press Ctrl+C in this terminal.
echo.

npm run tauri dev
popd
