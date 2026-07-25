#!/bin/bash

set -e  # Exit on any error

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

# Map architecture names
case "$ARCH" in
  x86_64)
    ARCH="x64"
    ;;
  arm64|aarch64)
    ARCH="arm64"
    ;;
  *)
    echo "⚠️  Warning: Unknown architecture $ARCH, using as-is"
    ;;
esac

# Map OS names
case "$OS" in
  linux)
    OS="linux"
    ;;
  darwin)
    OS="macos"
    ;;
  *)
    echo "⚠️  Warning: Unknown OS $OS, using as-is"
    ;;
esac

PLATFORM="${OS}-${ARCH}"

# Set CARGO_TARGET_DIR if not defined
if [ -z "$CARGO_TARGET_DIR" ]; then
  CARGO_TARGET_DIR="target"
fi

echo "🔍 Detected platform: $PLATFORM"
echo "🔧 Using target directory: $CARGO_TARGET_DIR"

# Set API base URL for remote features
export VK_SHARED_API_BASE=""
export VITE_VK_SHARED_API_BASE=""

echo "🧹 Cleaning previous builds..."
rm -rf npx-cli/dist
mkdir -p npx-cli/dist/$PLATFORM

echo "🔨 Building frontend..."
(cd frontend && npm run build)

echo "🔨 Building Rust binaries..."
cargo build --release --manifest-path Cargo.toml
cargo build --release --bin mcp_task_server --manifest-path Cargo.toml

echo "📦 Creating distribution package..."

# Copy the main binary
cp ${CARGO_TARGET_DIR}/release/server agentic-kanban
zip -q agentic-kanban.zip agentic-kanban
rm -f agentic-kanban 
mv agentic-kanban.zip npx-cli/dist/$PLATFORM/agentic-kanban.zip

# Copy the MCP binary
cp ${CARGO_TARGET_DIR}/release/mcp_task_server agentic-kanban-mcp
zip -q agentic-kanban-mcp.zip agentic-kanban-mcp
rm -f agentic-kanban-mcp
mv agentic-kanban-mcp.zip npx-cli/dist/$PLATFORM/agentic-kanban-mcp.zip

# Copy the Review CLI binary
cp ${CARGO_TARGET_DIR}/release/review agentic-kanban-review
zip -q agentic-kanban-review.zip agentic-kanban-review
rm -f agentic-kanban-review
mv agentic-kanban-review.zip npx-cli/dist/$PLATFORM/agentic-kanban-review.zip

echo "✅ Build complete!"
echo "📁 Files created:"
echo "   - npx-cli/dist/$PLATFORM/agentic-kanban.zip"
echo "   - npx-cli/dist/$PLATFORM/agentic-kanban-mcp.zip"
echo "   - npx-cli/dist/$PLATFORM/agentic-kanban-review.zip"
echo ""
echo "🚀 To test locally, run:"
echo "   cd npx-cli && node bin/cli.js"
