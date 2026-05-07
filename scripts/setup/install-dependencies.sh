#!/bin/bash

# This script checks for and helps install the necessary system dependencies
# for the hit-vvc Tauri application across different platforms.

echo "🔎 Checking for required system dependencies..."
echo "================================================"

# --- Helper Functions ---

# Function to check if a command is available in the system's PATH
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# --- Dependency Checks ---

dependencies_ok=true

# 1. Check for Rust (required by Tauri)
if ! command_exists rustc; then
    echo "❌ Rust is not installed, but it's required for Tauri."
    echo "   This script will attempt to install it using rustup."
    # Attempt to install Rust via rustup
    if command_exists curl; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        # Source the cargo environment to make it available in the current session
        source "$HOME/.cargo/env"
        if ! command_exists rustc; then
            echo "   Rust installation failed. Please install it manually from: https://www.rust-lang.org/tools/install"
            dependencies_ok=false
        else
            echo "✅ Rust has been installed successfully."
        fi
    else
        echo "   'curl' is not available. Please install Rust manually from: https://www.rust-lang.org/tools/install"
        dependencies_ok=false
    fi
else
    echo "✅ Rust is already installed."
fi

# 2. Platform-specific build tool checks
case "$(uname -s)" in
    Darwin) # macOS
        echo "🍎 Detected macOS."
        if ! xcode-select -p &>/dev/null; then
            echo "❌ Xcode Command Line Tools not found."
            echo "   Please install them by running the following command in your terminal:"
            echo "   xcode-select --install"
            # Note: This command requires user interaction, so we can't automate it here.
            dependencies_ok=false
        else
            echo "✅ Xcode Command Line Tools are installed."
        fi
        ;;
    MINGW*|CYGWIN*|MSYS*) # Windows (Git Bash, etc.)
        echo "💻 Detected Windows."
        echo "   Please ensure 'Microsoft C++ Build Tools' are installed."
        echo "   You can get them with the Visual Studio Installer here:"
        echo "   https://visualstudio.microsoft.com/visual-cpp-build-tools/"
        # We cannot automate this check or installation from a bash script.
        ;;
    Linux) # Linux
        echo "🐧 Detected Linux."
        echo "   Please ensure you have the necessary libraries for Tauri."
        echo "   For Debian/Ubuntu, you can install them with:"
        echo "   sudo apt update"
        echo "   sudo apt install libwebkit2gtk-4.0-dev build-essential curl wget file libssl-dev appmenu-gtk2-module librsvg2-dev"
        echo "   For other distributions, please consult the Tauri documentation for prerequisites."
        # We don't automate this to avoid making assumptions about the package manager.
        ;;
    *)
        echo "🤔 Unknown OS. Please ensure you have the necessary build tools for a Rust + Webview application."
        ;;
esac

# --- Final Check ---

echo ""
if [ "$dependencies_ok" = false ]; then
    echo "🚫 Some system dependencies are missing or could not be installed automatically."
    echo "   Please install them manually based on the instructions above and restart your terminal."
    exit 1
else
    echo "✅ All required system dependencies appear to be in place."
    echo "   You can now run the development server."
fi
