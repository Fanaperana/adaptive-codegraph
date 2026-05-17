#!/usr/bin/env bash
set -euo pipefail

echo "🔧 Installing adaptive-codegraph..."

# Build release binaries
cargo build --release 2>&1 | tail -3

# Determine install directory
INSTALL_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"
mkdir -p "$INSTALL_DIR"

# Install binaries (rm first to avoid "text file busy" errors)
rm -f "$INSTALL_DIR/adaptive-codegraph" "$INSTALL_DIR/adaptive-codegraph-mcp" "$INSTALL_DIR/adaptive-codegraph-daemon"
cp target/release/adaptive-codegraph-cli "$INSTALL_DIR/adaptive-codegraph"
cp target/release/adaptive-codegraph-mcp "$INSTALL_DIR/adaptive-codegraph-mcp"
cp target/release/adaptive-codegraph-daemon "$INSTALL_DIR/adaptive-codegraph-daemon"

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
