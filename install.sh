#!/usr/bin/env bash
set -euo pipefail

echo "🔧 Installing adaptive-codegraph..."

# Build release binaries (add --no-default-features to skip the embedding model)
cargo build --release -p adaptive-codegraph-cli -p adaptive-codegraph-mcp -p adaptive-codegraph-daemon "$@" 2>&1 | tail -3

# Windows builds produce .exe files
EXE=""
if [ -f target/release/adaptive-codegraph-cli.exe ]; then EXE=".exe"; fi

# Determine install directory
INSTALL_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"
mkdir -p "$INSTALL_DIR"

# Install binaries (rm first to avoid "text file busy" errors)
rm -f "$INSTALL_DIR/adaptive-codegraph$EXE" "$INSTALL_DIR/adaptive-codegraph-mcp$EXE" "$INSTALL_DIR/adaptive-codegraph-daemon$EXE"
cp "target/release/adaptive-codegraph-cli$EXE" "$INSTALL_DIR/adaptive-codegraph$EXE"
cp "target/release/adaptive-codegraph-mcp$EXE" "$INSTALL_DIR/adaptive-codegraph-mcp$EXE"
cp "target/release/adaptive-codegraph-daemon$EXE" "$INSTALL_DIR/adaptive-codegraph-daemon$EXE"

echo ""
echo "✅ Installed to $INSTALL_DIR/"
echo "   adaptive-codegraph        (CLI)"
echo "   adaptive-codegraph-mcp    (MCP server)"
echo "   adaptive-codegraph-daemon (file watcher)"
echo ""

# Verify PATH
if command -v adaptive-codegraph &>/dev/null; then
    echo "🎉 Ready! cd into any project and run:"
    echo "   adaptive-codegraph init"
    echo "   adaptive-codegraph index"
else
    echo "⚠️  $INSTALL_DIR is not in your PATH. Add it:"
    echo "   export PATH=\"$INSTALL_DIR:\$PATH\""
fi
