# Whis - Voice-to-Text Application
#
# Run `just` to see all available commands.
# Commands follow the pattern: {action}-{component}

set shell := ["bash", "-cu"]
set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# macOS build environment for whisper.cpp C++17 support
export MACOSX_DEPLOYMENT_TARGET := "10.15"
export CMAKE_OSX_DEPLOYMENT_TARGET := "10.15"

# Show available commands
default:
    @just --list --unsorted

# ============================================================================
# PRIVATE DEPENDENCY CHECKS
# ============================================================================

[unix]
[private]
_check-cargo:
    @cargo --version >/dev/null 2>&1 || { echo "cargo not found. Run: just setup-cli"; exit 1; }

[windows]
[private]
_check-cargo:
    @cargo --version >$null 2>&1; if (-not $?) { Write-Error "cargo not found. Run: just setup-cli"; exit 1 }

[unix]
[private]
_check-npm:
    @npm --version >/dev/null 2>&1 || { echo "npm not found. Install Node.js: https://nodejs.org"; exit 1; }

[windows]
[private]
_check-npm:
    @npm --version >$null 2>&1; if (-not $?) { Write-Error "npm not found. Install Node.js: https://nodejs.org"; exit 1 }

[unix]
[private]
_check-tauri:
    @cargo tauri --version >/dev/null 2>&1 || { echo "tauri-cli not found. Run: just setup-desktop"; exit 1; }

[windows]
[private]
_check-tauri:
    @cargo tauri --version >$null 2>&1; if (-not $?) { Write-Error "tauri-cli not found. Run: just setup-desktop"; exit 1 }

[private]
_check-android:
    #!/usr/bin/env bash
    set -euo pipefail
    ERRORS=""

    # Check ANDROID_HOME
    if [ -z "${ANDROID_HOME:-}" ]; then
        echo "❌ ANDROID_HOME not set"
        ERRORS="1"
    elif [ ! -d "$ANDROID_HOME" ]; then
        echo "❌ ANDROID_HOME directory does not exist: $ANDROID_HOME"
        ERRORS="1"
    fi

    # Check adb
    if ! command -v adb >/dev/null 2>&1; then
        echo "❌ adb not found in PATH"
        ERRORS="1"
    fi

    # Check NDK (auto-detect from SDK - Tauri handles NDK_HOME automatically)
    if [ -n "${ANDROID_HOME:-}" ] && [ -d "$ANDROID_HOME/ndk" ]; then
        NDK_DIR=$(find "$ANDROID_HOME/ndk" -maxdepth 1 -type d -name "[0-9]*" | sort -V | tail -1)
        if [ -z "$NDK_DIR" ]; then
            echo "❌ NDK not found in $ANDROID_HOME/ndk"
            echo "   Install via Android Studio: SDK Manager > SDK Tools > NDK (Side by side)"
            ERRORS="1"
        fi
    fi

    # Check Rust Android targets
    for target in aarch64-linux-android armv7-linux-androideabi; do
        if ! rustup target list --installed 2>/dev/null | grep -q "$target"; then
            echo "❌ Rust target $target not installed"
            echo "   Run: rustup target add $target"
            ERRORS="1"
        fi
    done

    if [ -n "$ERRORS" ]; then
        echo ""
        echo "Run 'just setup-mobile' to fix these issues"
        exit 1
    fi

[private]
_check-android-device:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v adb >/dev/null 2>&1; then
        echo "❌ adb not found"
        exit 1
    fi

    DEVICES=$(adb devices | grep -v "List of devices" | grep -v "^$" | grep -v "unauthorized" || true)
    if [ -z "$DEVICES" ]; then
        echo "❌ No authorized Android device connected"
        echo ""
        echo "Troubleshooting:"
        echo "  1. Connect your device via USB"
        echo "  2. Enable Developer Options on your device"
        echo "  3. Enable USB Debugging in Developer Options"
        echo "  4. Set USB mode to 'File Transfer' (not 'Charging only')"
        echo "  5. Accept the 'Allow USB debugging?' prompt on your device"
        echo ""
        echo "Run 'adb devices' to check connection status"
        exit 1
    fi

[private]
_init-android: _check-tauri
    #!/usr/bin/env bash
    set -euo pipefail
    if [ ! -d "crates/whis-mobile/gen/android" ]; then
        echo "→ Initializing Android project..."
        cd crates/whis-mobile && cargo tauri android init
        echo "✓ Android project initialized"
    fi

# ============================================================================
# CLI
# ============================================================================

# Check and install CLI prerequisites
[group('cli')]
[linux]
setup-cli:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== CLI Prerequisites ==="
    echo ""

    # Check cargo
    if command -v cargo >/dev/null 2>&1; then
        echo "✓ cargo $(cargo --version | cut -d' ' -f2)"
    else
        echo "❌ cargo not found"
        echo "   Install Rust: https://rustup.rs"
        echo ""
        exit 1
    fi

    # Check ALSA development libs (using pkg-config for portability)
    if pkg-config --exists alsa 2>/dev/null; then
        echo "✓ ALSA development libraries"
    else
        echo "❌ ALSA development libraries not found"
        if command -v apt >/dev/null 2>&1; then
            echo "   Run: sudo apt install libasound2-dev"
        elif command -v dnf >/dev/null 2>&1; then
            echo "   Run: sudo dnf install alsa-lib-devel"
        elif command -v pacman >/dev/null 2>&1; then
            echo "   Run: sudo pacman -S alsa-lib"
        else
            echo "   Install ALSA development libraries using your package manager"
        fi
    fi

    echo ""
    echo "If all checks pass, run: just build-cli"

[group('cli')]
[macos]
setup-cli:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== CLI Prerequisites ==="
    echo ""

    # Check cargo
    if command -v cargo >/dev/null 2>&1; then
        echo "✓ cargo $(cargo --version | cut -d' ' -f2)"
    else
        echo "❌ cargo not found"
        echo "   Install Rust: https://rustup.rs"
        echo ""
        exit 1
    fi

    echo ""
    echo "If all checks pass, run: just build-cli"

[group('cli')]
[windows]
setup-cli:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== CLI Prerequisites ==="
    echo ""

    # Check cargo
    if command -v cargo >/dev/null 2>&1; then
        echo "✓ cargo $(cargo --version | cut -d' ' -f2)"
    else
        echo "❌ cargo not found"
        echo "   Install Rust: https://rustup.rs"
        echo ""
        exit 1
    fi

    echo ""
    echo "If all checks pass, run: just build-cli"

# Fetch CLI dependencies and install system dependencies
[group('cli')]
deps-cli: _check-cargo
    #!/usr/bin/env bash
    set -euo pipefail
    cargo fetch
    if [ "$(uname)" = "Linux" ]; then
        sudo apt-get update
        sudo apt-get install -y \
            libasound2-dev \
            libx11-dev \
            libxtst-dev \
            libpulse-dev
    fi

# Build CLI
[group('cli')]
build-cli: _check-cargo
    cargo build -p whis-cli

# Lint CLI code
[group('cli')]
lint-cli: _check-cargo
    cargo clippy -p whis-cli --all-targets --fix --allow-dirty --allow-staged

# Format CLI code
[group('cli')]
fmt-cli: _check-cargo
    cargo fmt -p whis-cli

# Install CLI to ~/.cargo/bin
[group('cli')]
install-cli: build-cli
    cargo install --path crates/whis-cli --force

# Uninstall CLI from ~/.cargo/bin
[group('cli')]
uninstall-cli:
    cargo uninstall whis || echo "CLI not installed"

# ============================================================================
# DESKTOP
# ============================================================================

# Check and install desktop prerequisites
[group('desktop')]
[linux]
setup-desktop:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Desktop Prerequisites ==="
    echo ""

    # Check cargo first
    if ! command -v cargo >/dev/null 2>&1; then
        echo "❌ cargo not found"
        echo "   Install Rust first: https://rustup.rs"
        exit 1
    fi
    echo "✓ cargo $(cargo --version | cut -d' ' -f2)"

    # Check npm
    if command -v npm >/dev/null 2>&1; then
        echo "✓ npm $(npm --version)"
    else
        echo "❌ npm not found"
        echo "   Install Node.js: https://nodejs.org"
        exit 1
    fi

    # Auto-install tauri-cli
    if command -v cargo-tauri >/dev/null 2>&1; then
        echo "✓ tauri-cli installed"
    else
        echo "→ Installing tauri-cli..."
        cargo install tauri-cli
        echo "✓ tauri-cli installed"
    fi

    # Check system libs using pkg-config (more portable across distros)
    MISSING_APT=""
    MISSING_DNF=""
    MISSING_PACMAN=""
    HAS_ERRORS=""

    # WebKit2GTK
    if pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
        echo "✓ webkit2gtk-4.1"
    else
        echo "❌ webkit2gtk-4.1 not found"
        MISSING_APT="$MISSING_APT libwebkit2gtk-4.1-dev"
        MISSING_DNF="$MISSING_DNF webkit2gtk4.1-devel"
        MISSING_PACMAN="$MISSING_PACMAN webkit2gtk-4.1"
        HAS_ERRORS="1"
    fi

    # AppIndicator
    if pkg-config --exists appindicator3-0.1 2>/dev/null; then
        echo "✓ appindicator3"
    else
        echo "❌ appindicator3 not found"
        MISSING_APT="$MISSING_APT libappindicator3-dev"
        MISSING_DNF="$MISSING_DNF libappindicator-gtk3-devel"
        MISSING_PACMAN="$MISSING_PACMAN libappindicator-gtk3"
        HAS_ERRORS="1"
    fi

    # librsvg
    if pkg-config --exists librsvg-2.0 2>/dev/null; then
        echo "✓ librsvg"
    else
        echo "❌ librsvg not found"
        MISSING_APT="$MISSING_APT librsvg2-dev"
        MISSING_DNF="$MISSING_DNF librsvg2-devel"
        MISSING_PACMAN="$MISSING_PACMAN librsvg"
        HAS_ERRORS="1"
    fi

    # patchelf (binary, not a library)
    if command -v patchelf >/dev/null 2>&1; then
        echo "✓ patchelf"
    else
        echo "❌ patchelf not found"
        MISSING_APT="$MISSING_APT patchelf"
        MISSING_DNF="$MISSING_DNF patchelf"
        MISSING_PACMAN="$MISSING_PACMAN patchelf"
        HAS_ERRORS="1"
    fi

    if [ -n "$HAS_ERRORS" ]; then
        echo ""
        echo "Install missing packages:"
        if command -v apt >/dev/null 2>&1; then
            echo "   sudo apt install$MISSING_APT"
        elif command -v dnf >/dev/null 2>&1; then
            echo "   sudo dnf install$MISSING_DNF"
        elif command -v pacman >/dev/null 2>&1; then
            echo "   sudo pacman -S$MISSING_PACMAN"
        else
            echo "   Install the missing libraries using your package manager"
        fi
    else
        echo ""
        echo "All prerequisites installed! Run: just dev-desktop"
    fi

[group('desktop')]
[macos]
setup-desktop:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Desktop Prerequisites ==="
    echo ""

    # Check cargo
    if ! command -v cargo >/dev/null 2>&1; then
        echo "❌ cargo not found"
        echo "   Install Rust: https://rustup.rs"
        exit 1
    fi
    echo "✓ cargo $(cargo --version | cut -d' ' -f2)"

    # Check npm
    if command -v npm >/dev/null 2>&1; then
        echo "✓ npm $(npm --version)"
    else
        echo "❌ npm not found"
        echo "   Install Node.js: https://nodejs.org"
        exit 1
    fi

    # Check Xcode CLI tools
    if xcode-select -p >/dev/null 2>&1; then
        echo "✓ Xcode Command Line Tools"
    else
        echo "❌ Xcode Command Line Tools not found"
        echo "   Run: xcode-select --install"
        exit 1
    fi

    # Auto-install tauri-cli
    if command -v cargo-tauri >/dev/null 2>&1; then
        echo "✓ tauri-cli installed"
    else
        echo "→ Installing tauri-cli..."
        cargo install tauri-cli
        echo "✓ tauri-cli installed"
    fi

    echo ""
    echo "All prerequisites installed! Run: just dev-desktop"

[group('desktop')]
[windows]
setup-desktop:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Desktop Prerequisites ==="
    echo ""

    # Check cargo
    if ! command -v cargo >/dev/null 2>&1; then
        echo "❌ cargo not found"
        echo "   Install Rust: https://rustup.rs"
        exit 1
    fi
    echo "✓ cargo $(cargo --version | cut -d' ' -f2)"

    # Check npm
    if command -v npm >/dev/null 2>&1; then
        echo "✓ npm $(npm --version)"
    else
        echo "❌ npm not found"
        echo "   Install Node.js: https://nodejs.org"
        exit 1
    fi

    # Auto-install tauri-cli
    if command -v cargo-tauri >/dev/null 2>&1; then
        echo "✓ tauri-cli installed"
    else
        echo "→ Installing tauri-cli..."
        cargo install tauri-cli
        echo "✓ tauri-cli installed"
    fi

    echo ""
    echo "WebView2 is included with Windows 10/11."
    echo "All prerequisites installed! Run: just dev-desktop"

# Fetch desktop dependencies
[group('desktop')]
deps-desktop: _check-npm _check-tauri
    #!/usr/bin/env bash
    set -euo pipefail
    cargo fetch
    cd crates/whis-desktop/ui && npm ci
    if [ "$(uname)" = "Linux" ]; then
        sudo apt-get update
        sudo apt-get install -y \
            libwebkit2gtk-4.1-dev \
            libappindicator3-dev \
            librsvg2-dev \
            patchelf \
            libasound2-dev
    fi

# Run desktop in dev mode
[group('desktop')]
dev-desktop: deps-desktop
    #!/usr/bin/env bash
    set -euo pipefail
    cd crates/whis-desktop && cargo tauri dev

# Build desktop for release
[group('desktop')]
build-desktop: deps-desktop
    #!/usr/bin/env bash
    set -euo pipefail
    (cd crates/whis-desktop/ui && npm run build)
    (cd crates/whis-desktop && cargo tauri build)

# Lint desktop code
[group('desktop')]
lint-desktop: _check-npm
    #!/usr/bin/env bash
    set -euo pipefail
    cd crates/whis-desktop/ui && npm run lint:fix

# Format desktop code
[group('desktop')]
fmt-desktop: _check-npm
    #!/usr/bin/env bash
    set -euo pipefail
    cd crates/whis-desktop/ui && npm run lint:fix

# Install desktop app to user directory
[group('desktop')]
[linux]
install-desktop: build-desktop
    #!/usr/bin/env bash
    set -euo pipefail
    APPIMAGE=$(find target/release/bundle/appimage -name "*.AppImage" | head -1)
    if [ -z "$APPIMAGE" ]; then
        echo "❌ No AppImage found. Run 'just build-desktop' first."
        exit 1
    fi
    mkdir -p ~/.local/bin
    DEST=~/.local/bin/Whis.AppImage
    cp "$APPIMAGE" "$DEST"
    chmod +x "$DEST"
    "$DEST" --install
    echo "✓ Installed to $DEST"
    echo "  Launch 'Whis' from your app menu"

[group('desktop')]
[macos]
install-desktop: build-desktop
    #!/usr/bin/env bash
    set -euo pipefail
    APP=$(find target/release/bundle/macos -name "*.app" -type d | head -1)
    if [ -z "$APP" ]; then
        echo "❌ No .app bundle found. Run 'just build-desktop' first."
        exit 1
    fi
    mkdir -p ~/Applications
    DEST=~/Applications/Whis.app
    rm -rf "$DEST"
    cp -R "$APP" "$DEST"
    echo "✓ Installed to $DEST"
    echo "  Launch 'Whis' from Spotlight or Applications folder"

[group('desktop')]
[windows]
install-desktop: build-desktop
    #!/usr/bin/env bash
    set -euo pipefail
    EXE=$(find target/release/bundle -name "*.exe" | head -1)
    if [ -z "$EXE" ]; then
        echo "❌ No .exe found. Run 'just build-desktop' first."
        exit 1
    fi
    DEST="${LOCALAPPDATA}/Programs/Whis"
    mkdir -p "$DEST"
    cp "$EXE" "$DEST/Whis.exe"
    echo "✓ Installed to $DEST/Whis.exe"
    echo "  You can add this to your Start Menu manually"

# Uninstall desktop app from user directory
[group('desktop')]
[linux]
uninstall-desktop:
    #!/usr/bin/env bash
    set -euo pipefail
    APPIMAGE=~/.local/bin/Whis.AppImage
    if [ -f "$APPIMAGE" ]; then
        "$APPIMAGE" --remove-appimage-desktop-integration 2>/dev/null || true
        rm -f "$APPIMAGE"
        echo "✓ Removed $APPIMAGE"
    else
        echo "Desktop app not installed"
    fi
    # Clean up desktop integration files (AppImage removal may not reliably do this)
    rm -f ~/.local/share/applications/ink.whis.Whis.desktop
    rm -f ~/.local/share/icons/hicolor/*/apps/ink.whis.Whis.png
    rm -f ~/.local/share/icons/hicolor/*/apps/ink.whis.Whis.svg
    update-desktop-database ~/.local/share/applications 2>/dev/null || true

[group('desktop')]
[macos]
uninstall-desktop:
    #!/usr/bin/env bash
    set -euo pipefail
    APP=~/Applications/Whis.app
    if [ -d "$APP" ]; then
        rm -rf "$APP"
        echo "✓ Removed $APP"
    else
        echo "Desktop app not installed"
    fi

[group('desktop')]
[windows]
uninstall-desktop:
    #!/usr/bin/env bash
    set -euo pipefail
    DEST="${LOCALAPPDATA}/Programs/Whis"
    if [ -d "$DEST" ]; then
        rm -rf "$DEST"
        echo "✓ Removed $DEST"
    else
        echo "Desktop app not installed"
    fi

# ============================================================================
# MOBILE
# ============================================================================

# Check and install mobile prerequisites
[group('mobile')]
[linux]
setup-mobile:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Mobile (Android) Prerequisites ==="
    echo ""

    # Check cargo
    if ! command -v cargo >/dev/null 2>&1; then
        echo "❌ cargo not found"
        echo "   Install Rust: https://rustup.rs"
        exit 1
    fi
    echo "✓ cargo $(cargo --version | cut -d' ' -f2)"

    # Check npm
    if command -v npm >/dev/null 2>&1; then
        echo "✓ npm $(npm --version)"
    else
        echo "❌ npm not found"
        echo "   Install Node.js: https://nodejs.org"
        exit 1
    fi

    # Auto-install tauri-cli
    if command -v cargo-tauri >/dev/null 2>&1; then
        echo "✓ tauri-cli installed"
    else
        echo "→ Installing tauri-cli..."
        cargo install tauri-cli
        echo "✓ tauri-cli installed"
    fi

    # Check ANDROID_HOME
    if [ -n "${ANDROID_HOME:-}" ] && [ -d "$ANDROID_HOME" ]; then
        echo "✓ ANDROID_HOME: $ANDROID_HOME"
    else
        echo "❌ ANDROID_HOME not set or directory doesn't exist"
        echo "   1. Install Android Studio: https://developer.android.com/studio"
        echo "   2. Add to ~/.bashrc:"
        echo '      export ANDROID_HOME="$HOME/Android/Sdk"'
        echo '      export PATH="$PATH:$ANDROID_HOME/platform-tools"'
        echo "   3. Run: source ~/.bashrc"
    fi

    # Check adb (platform-tools)
    if command -v adb >/dev/null 2>&1; then
        echo "✓ adb installed"
    else
        echo "❌ adb not found"
        echo "   Install via Android Studio: SDK Manager > SDK Tools > Android SDK Platform-Tools"
    fi

    # Check NDK
    if [ -n "${ANDROID_HOME:-}" ] && [ -d "$ANDROID_HOME/ndk" ]; then
        NDK_VERSION=$(find "$ANDROID_HOME/ndk" -maxdepth 1 -type d -name "[0-9]*" | sort -V | tail -1 | xargs basename 2>/dev/null || true)
        if [ -n "$NDK_VERSION" ]; then
            echo "✓ NDK: $NDK_VERSION"
        else
            echo "❌ NDK not installed"
            echo "   Install via Android Studio: SDK Manager > SDK Tools > NDK (Side by side)"
        fi
    else
        echo "⚠ Cannot check NDK (ANDROID_HOME not set)"
    fi

    # Check build-tools
    if [ -n "${ANDROID_HOME:-}" ] && [ -d "$ANDROID_HOME/build-tools" ]; then
        BUILD_TOOLS=$(ls "$ANDROID_HOME/build-tools" 2>/dev/null | sort -V | tail -1)
        if [ -n "$BUILD_TOOLS" ]; then
            echo "✓ build-tools: $BUILD_TOOLS"
        else
            echo "❌ build-tools not installed"
            echo "   Install via Android Studio: SDK Manager > SDK Tools > Android SDK Build-Tools"
        fi
    fi

    # Check Android targets
    echo ""
    echo "Rust Android targets:"
    for target in aarch64-linux-android armv7-linux-androideabi; do
        if rustup target list --installed | grep -q "$target"; then
            echo "✓ $target"
        else
            echo "→ Adding $target..."
            rustup target add "$target"
            echo "✓ $target"
        fi
    done

    echo ""
    echo "If all checks pass, connect a device and run: just dev-mobile"

[group('mobile')]
[macos]
setup-mobile:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Mobile (Android) Prerequisites ==="
    echo ""

    # Check cargo
    if ! command -v cargo >/dev/null 2>&1; then
        echo "❌ cargo not found"
        echo "   Install Rust: https://rustup.rs"
        exit 1
    fi
    echo "✓ cargo $(cargo --version | cut -d' ' -f2)"

    # Check npm
    if command -v npm >/dev/null 2>&1; then
        echo "✓ npm $(npm --version)"
    else
        echo "❌ npm not found"
        echo "   Install Node.js: https://nodejs.org"
        exit 1
    fi

    # Auto-install tauri-cli
    if command -v cargo-tauri >/dev/null 2>&1; then
        echo "✓ tauri-cli installed"
    else
        echo "→ Installing tauri-cli..."
        cargo install tauri-cli
        echo "✓ tauri-cli installed"
    fi

    # Check ANDROID_HOME
    if [ -n "${ANDROID_HOME:-}" ] && [ -d "$ANDROID_HOME" ]; then
        echo "✓ ANDROID_HOME: $ANDROID_HOME"
    else
        echo "❌ ANDROID_HOME not set or directory doesn't exist"
        echo "   1. Install Android Studio: https://developer.android.com/studio"
        echo "   2. Add to ~/.zshrc:"
        echo '      export ANDROID_HOME="$HOME/Library/Android/sdk"'
        echo '      export PATH="$PATH:$ANDROID_HOME/platform-tools"'
        echo "   3. Run: source ~/.zshrc"
    fi

    # Check adb (platform-tools)
    if command -v adb >/dev/null 2>&1; then
        echo "✓ adb installed"
    else
        echo "❌ adb not found"
        echo "   Install via Android Studio: SDK Manager > SDK Tools > Android SDK Platform-Tools"
    fi

    # Check NDK
    if [ -n "${ANDROID_HOME:-}" ] && [ -d "$ANDROID_HOME/ndk" ]; then
        NDK_VERSION=$(find "$ANDROID_HOME/ndk" -maxdepth 1 -type d -name "[0-9]*" | sort -V | tail -1 | xargs basename 2>/dev/null || true)
        if [ -n "$NDK_VERSION" ]; then
            echo "✓ NDK: $NDK_VERSION"
        else
            echo "❌ NDK not installed"
            echo "   Install via Android Studio: SDK Manager > SDK Tools > NDK (Side by side)"
        fi
    else
        echo "⚠ Cannot check NDK (ANDROID_HOME not set)"
    fi

    # Check Android targets
    echo ""
    echo "Rust Android targets:"
    for target in aarch64-linux-android armv7-linux-androideabi; do
        if rustup target list --installed | grep -q "$target"; then
            echo "✓ $target"
        else
            echo "→ Adding $target..."
            rustup target add "$target"
            echo "✓ $target"
        fi
    done

    echo ""
    echo "If all checks pass, connect a device and run: just dev-mobile"

[group('mobile')]
[windows]
setup-mobile:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Mobile (Android) Prerequisites ==="
    echo ""

    # Check cargo
    if ! command -v cargo >/dev/null 2>&1; then
        echo "❌ cargo not found"
        echo "   Install Rust: https://rustup.rs"
        exit 1
    fi
    echo "✓ cargo $(cargo --version | cut -d' ' -f2)"

    # Check npm
    if command -v npm >/dev/null 2>&1; then
        echo "✓ npm $(npm --version)"
    else
        echo "❌ npm not found"
        echo "   Install Node.js: https://nodejs.org"
        exit 1
    fi

    # Auto-install tauri-cli
    if command -v cargo-tauri >/dev/null 2>&1; then
        echo "✓ tauri-cli installed"
    else
        echo "→ Installing tauri-cli..."
        cargo install tauri-cli
        echo "✓ tauri-cli installed"
    fi

    # Check ANDROID_HOME
    if [ -n "${ANDROID_HOME:-}" ] && [ -d "$ANDROID_HOME" ]; then
        echo "✓ ANDROID_HOME: $ANDROID_HOME"
    else
        echo "❌ ANDROID_HOME not set or directory doesn't exist"
        echo "   1. Install Android Studio: https://developer.android.com/studio"
        echo "   2. Set environment variable:"
        echo '      ANDROID_HOME=%LOCALAPPDATA%\Android\Sdk'
        echo "   3. Add to PATH: %ANDROID_HOME%\\platform-tools"
    fi

    # Check adb (platform-tools)
    if command -v adb >/dev/null 2>&1; then
        echo "✓ adb installed"
    else
        echo "❌ adb not found"
        echo "   Install via Android Studio: SDK Manager > SDK Tools > Android SDK Platform-Tools"
    fi

    # Check Android targets
    echo ""
    echo "Rust Android targets:"
    for target in aarch64-linux-android armv7-linux-androideabi; do
        if rustup target list --installed | grep -q "$target"; then
            echo "✓ $target"
        else
            echo "→ Adding $target..."
            rustup target add "$target"
            echo "✓ $target"
        fi
    done

    echo ""
    echo "If all checks pass, connect a device and run: just dev-mobile"

# Fetch mobile dependencies
[group('mobile')]
deps-mobile: _check-npm _check-tauri _check-android _init-android
    #!/usr/bin/env bash
    set -euo pipefail
    cargo fetch
    # Build the local tauri-plugin-floating-bubble first
    (cd crates/tauri-plugin-floating-bubble && npm install && npm run build)
    # Use npm install instead of npm ci to properly handle local file: dependencies
    (cd crates/whis-mobile/ui && npm install)

# Run mobile app on connected Android device
[group('mobile')]
dev-mobile: deps-mobile _check-android-device
    #!/usr/bin/env bash
    set -euo pipefail
    # Set ANDROID_NDK_ROOT for aws-lc-sys (rustls crypto backend)
    if [ -n "${ANDROID_HOME:-}" ] && [ -d "$ANDROID_HOME/ndk" ]; then
        export ANDROID_NDK_ROOT=$(find "$ANDROID_HOME/ndk" -maxdepth 1 -type d -name "[0-9]*" | sort -V | tail -1)
    fi
    (cd crates/whis-mobile/ui && npm run build)
    # Forward port 5173 from device to host for stable dev server connection
    adb reverse tcp:5173 tcp:5173
    # Use --host 127.0.0.1 to force localhost (works via adb reverse tunnel)
    (cd crates/whis-mobile && cargo tauri android dev --host 127.0.0.1)

# Build mobile APK
[group('mobile')]
build-mobile: deps-mobile
    #!/usr/bin/env bash
    set -euo pipefail
    # Set ANDROID_NDK_ROOT for aws-lc-sys (rustls crypto backend)
    if [ -n "${ANDROID_HOME:-}" ] && [ -d "$ANDROID_HOME/ndk" ]; then
        export ANDROID_NDK_ROOT=$(find "$ANDROID_HOME/ndk" -maxdepth 1 -type d -name "[0-9]*" | sort -V | tail -1)
    fi
    (cd crates/whis-mobile/ui && npm run build)
    (cd crates/whis-mobile && cargo tauri android build)

# Lint mobile code
[group('mobile')]
lint-mobile: _check-npm
    #!/usr/bin/env bash
    set -euo pipefail
    cd crates/whis-mobile/ui && npm run lint:fix

# Format mobile code
[group('mobile')]
fmt-mobile: _check-npm
    #!/usr/bin/env bash
    set -euo pipefail
    cd crates/whis-mobile/ui && npm run lint:fix

# Install mobile app to connected device (uses debug build for signing)
[group('mobile')]
install-mobile: deps-mobile _check-android-device
    #!/usr/bin/env bash
    set -euo pipefail
    # Set ANDROID_NDK_ROOT for aws-lc-sys (rustls crypto backend)
    if [ -n "${ANDROID_HOME:-}" ] && [ -d "$ANDROID_HOME/ndk" ]; then
        export ANDROID_NDK_ROOT=$(find "$ANDROID_HOME/ndk" -maxdepth 1 -type d -name "[0-9]*" | sort -V | tail -1)
    fi
    (cd crates/whis-mobile/ui && npm run build)
    (cd crates/whis-mobile && cargo tauri android build --debug)
    APK=$(find crates/whis-mobile/gen/android/app/build -name "*.apk" -path "*debug*" | head -1)
    if [ -z "$APK" ]; then
        echo "❌ No APK found."
        exit 1
    fi
    echo "Installing $APK..."
    adb install -r "$APK"
    echo "✓ Installed to device"

# Uninstall mobile app from connected device
[group('mobile')]
uninstall-mobile: _check-android-device
    adb uninstall ink.whis.mobile || echo "App not installed"

# ============================================================================
# WEBSITE
# ============================================================================

# Check and install website prerequisites
[group('website')]
setup-website:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Website Prerequisites ==="
    echo ""

    if command -v npm >/dev/null 2>&1; then
        echo "✓ npm $(npm --version)"
        echo ""
        echo "All prerequisites installed! Run: just dev-website"
    else
        echo "❌ npm not found"
        echo "   Install Node.js: https://nodejs.org"
    fi

# Fetch website dependencies
[group('website')]
deps-website: _check-npm
    #!/usr/bin/env bash
    set -euo pipefail
    cd website && npm ci

# Run website dev server
[group('website')]
dev-website: deps-website
    #!/usr/bin/env bash
    set -euo pipefail
    cd website && npm run dev

# Build website for production
[group('website')]
build-website: deps-website
    #!/usr/bin/env bash
    set -euo pipefail
    cd website && node scripts/fetch-stats.mjs && npm run build

# Lint website code
[group('website')]
lint-website: _check-npm
    #!/usr/bin/env bash
    set -euo pipefail
    cd website && npm run lint:fix

# Format website code
[group('website')]
fmt-website: _check-npm
    #!/usr/bin/env bash
    set -euo pipefail
    cd website && npm run lint:fix

# ============================================================================
# ALL
# ============================================================================

# Check all prerequisites
[group('all')]
setup-all: setup-cli setup-desktop setup-mobile setup-website

# Fetch all dependencies
[group('all')]
deps-all: deps-cli deps-desktop deps-mobile deps-website

# Build all (frontend + Rust)
[group('all')]
build-all: deps-all
    #!/usr/bin/env bash
    set -euo pipefail
    (cd crates/whis-desktop/ui && npm run build)
    (cd crates/whis-mobile/ui && npm run build)
    (cd website && npm run build)
    cargo build -p whis-cli -p whis-desktop -p whis-mobile

# Lint all code
[group('all')]
lint-all: build-all
    #!/usr/bin/env bash
    set -euo pipefail
    cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged
    (cd crates/whis-desktop/ui && npm run lint:fix)
    (cd crates/whis-mobile/ui && npm run lint:fix)
    (cd website && npm run lint:fix)

# Format all code
[group('all')]
fmt-all:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo fmt --all
    (cd crates/whis-desktop/ui && npm run lint:fix)
    (cd crates/whis-mobile/ui && npm run lint:fix)
    (cd website && npm run lint:fix)

# Install CLI and desktop app
[group('all')]
install-all: install-cli install-desktop

# Uninstall CLI and desktop app
[group('all')]
uninstall-all: uninstall-cli uninstall-desktop

# Verify all code (format check + lint)
[group('all')]
check-all: build-all
    #!/usr/bin/env bash
    set -euo pipefail
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features
    (cd crates/whis-desktop/ui && npm run lint)
    (cd crates/whis-mobile/ui && npm run lint)
    (cd website && npm run lint)

# Clean all build artifacts
[group('all')]
clean-all:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo clean
    rm -rf crates/whis-desktop/ui/dist
    rm -rf crates/whis-desktop/ui/node_modules
    rm -rf crates/whis-mobile/ui/dist
    rm -rf crates/whis-mobile/ui/node_modules
    rm -rf crates/whis-mobile/gen/android/app/build
    rm -rf website/dist
    rm -rf website/node_modules

# ============================================================================
# RELEASE (CI/CD recipes for GitHub Actions)
# ============================================================================

# Create and push a release tag (run after version bump commit)
[group('release')]
release version:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Creating release v{{version}}..."
    git tag -a "v{{version}}" -m "v{{version}}"
    git push origin main --tags
    echo ""
    echo "Release v{{version}} pushed!"
    echo "CI will build artifacts: https://github.com/frankdierolf/whis/actions"

# --- Frank's personal recipes (hidden from `just --list`) ---

# Bump version across all files using Claude Code
[private]
[group('release')]
bump-version version:
    claude --dangerously-skip-permissions -p "Update all version references in this codebase to {{version}}. Find and update versions in: Cargo.toml (workspace version), all tauri.conf.json files, all package.json files, and any Vue files with hardcoded version strings. Update each file you find. Do not commit the changes."

# Update local Flathub checkout (for testing before automated PR)
[private]
[group('release')]
[linux]
flathub-update:
    #!/usr/bin/env bash
    set -euo pipefail
    FLATHUB_DIR="${FLATHUB_DIR:-$HOME/repos/ink.whis.Whis}"
    if [ ! -d "$FLATHUB_DIR" ]; then
        echo "Error: Flathub repo not found at $FLATHUB_DIR"
        echo "Set FLATHUB_DIR or clone: git clone https://github.com/flathub/ink.whis.Whis.git $FLATHUB_DIR"
        exit 1
    fi
    echo "Generating Flathub sources..."
    echo "  cargo-sources.json..."
    python ~/repos/flatpak-builder-tools/cargo/flatpak-cargo-generator.py \
        Cargo.lock -o "$FLATHUB_DIR/cargo-sources.json"
    echo "  node-sources.json..."
    flatpak-node-generator npm \
        crates/whis-desktop/ui/package-lock.json \
        -o "$FLATHUB_DIR/node-sources.json"
    echo ""
    echo "Done! Source files updated in $FLATHUB_DIR"
    echo "Now manually update:"
    echo "  - ink.whis.Whis.yaml (tag and commit)"
    echo "  - ink.whis.Whis.metainfo.xml (release entry)"

# Build CLI release binary (native)
[group('release')]
build-release-cli: _check-cargo
    cargo build --release -p whis-cli

# Build CLI for specific target (cross-compilation)
[group('release')]
build-release-cli-cross target: _check-cargo
    cross build --release -p whis-cli --target {{target}}

# Build CLI for macOS target
[group('release')]
build-release-cli-macos target: _check-cargo
    #!/usr/bin/env bash
    set -euo pipefail
    rustup target add {{target}}
    cargo build --release -p whis-cli --target {{target}}

# Build desktop release (outputs platform-appropriate bundles)
[group('release')]
build-release-desktop: deps-desktop
    #!/usr/bin/env bash
    set -euo pipefail
    (cd crates/whis-desktop/ui && npm run build)
    (cd crates/whis-desktop && cargo tauri build)

# Build desktop release for macOS Intel (cross-compile from Apple Silicon)
[group('release')]
build-release-desktop-macos-intel: _check-npm _check-tauri
    #!/usr/bin/env bash
    set -euo pipefail
    (cd crates/whis-desktop/ui && npm ci && npm run build)
    (cd crates/whis-desktop && cargo tauri build --target x86_64-apple-darwin)

# Build mobile release APK
[group('release')]
build-release-mobile: deps-mobile
    #!/usr/bin/env bash
    set -euo pipefail
    (cd crates/whis-mobile/ui && npm run build)
    (cd crates/whis-mobile && cargo tauri android build)

# Publish whis-core to crates.io
[group('release')]
publish-crates-core: _check-cargo
    cargo publish -p whis-core --no-verify

# Publish whis CLI to crates.io
[group('release')]
publish-crates-cli: _check-cargo
    cargo publish -p whis-cli --no-verify

# Publish all crates (core first, then CLI)
[group('release')]
publish-crates: publish-crates-core
    @echo "Waiting for crates.io index to update..."
    sleep 30
    just publish-crates-cli

# Update AUR package (requires SSH key and version)
[group('release')]
[linux]
publish-aur version:
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION_TAG="{{version}}"
    cd /tmp
    rm -rf whis-aur
    git clone ssh://aur@aur.archlinux.org/whis.git whis-aur
    cd whis-aur
    # Strip 'v' prefix from version if present
    VERSION="${VERSION_TAG#v}"
    sed -i "s/^pkgver=.*/pkgver=$VERSION/" PKGBUILD
    sed -i "s/^pkgrel=.*/pkgrel=1/" PKGBUILD
    # Calculate new checksum
    TARBALL_URL="https://github.com/frankdierolf/whis/archive/refs/tags/$VERSION_TAG.tar.gz"
    SHA256=$(curl -sL "$TARBALL_URL" | sha256sum | cut -d' ' -f1)
    sed -i "s/^sha256sums=.*/sha256sums=('$SHA256')/" PKGBUILD
    # Generate .SRCINFO
    docker run --rm -v "$(pwd)":/pkg -w /pkg archlinux:latest bash -c "pacman -Sy --noconfirm base-devel && useradd builder && chown -R builder:builder . && su builder -c 'makepkg --printsrcinfo > .SRCINFO'"
    # Fix ownership after Docker
    sudo chown -R $(id -u):$(id -g) .
    git add PKGBUILD .SRCINFO
    git commit -m "Update to $VERSION_TAG" || echo "Already up to date"
    git push
