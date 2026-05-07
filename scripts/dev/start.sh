#!/bin/bash
set -euo pipefail

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)

cd "$REPO_ROOT"

echo "🎤 Starting hit-vvc Development Server..."
echo "================================================"

"$SCRIPT_DIR/../setup/install-dependencies.sh"

echo ""
echo "✅ System dependencies check passed."

# --- Port Management ---

# Function to check if a command is available
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Function to check and free the development port
check_port() {
    local PORT=1420
    echo "🔎 Checking if port $PORT is available..."

    if command_exists lsof; then
        if lsof -Pi :$PORT -sTCP:LISTEN -t >/dev/null ; then
            echo "⚠️  Port $PORT is in use. Attempting to free it..."
            lsof -t -i:$PORT | xargs kill -9 >/dev/null 2>&1
            sleep 2

            if lsof -Pi :$PORT -sTCP:LISTEN -t >/dev/null ; then
                echo "❌ Failed to free port $PORT. Please close the application using it and try again."
                exit 1
            else
                echo "✅ Port $PORT has been freed."
            fi
        else
            echo "✅ Port $PORT is free."
        fi
    else
        # Fallback for systems without lsof (like Windows default)
        if netstat -ano | findstr ":$PORT" | findstr "LISTENING" >/dev/null; then
             echo "⚠️  Port $PORT is in use. This script cannot automatically free it on this system."
             echo "   Please find and stop the process using port $PORT and run the script again."
             exit 1
        else
            echo "✅ Port $PORT appears to be free."
        fi
    fi
}

# --- Main Execution ---

# Check for Node.js dependencies
if [ ! -d "node_modules" ]; then
    echo "📦 Node modules not found. Running 'npm install'..."
    npm install
else
    echo "✅ Node modules are already installed."
fi

echo "🎙️  Preparing bundled ASR runtime..."
npm run runtime:ensure

check_port

echo ""
echo "🚀 Starting Tauri development server..."
echo "   - Frontend will be available at: http://localhost:1420"
echo "   - Tauri app will open automatically"
echo ""
echo "💡 To stop the server, press Ctrl+C in this terminal."
echo ""

# Start the Tauri development server
npm run tauri dev
