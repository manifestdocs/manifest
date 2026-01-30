#!/bin/bash
# Release script - bumps version in all files, commits, tags, and pushes
#
# Usage: ./scripts/release.sh <version>
# Example: ./scripts/release.sh 0.1.14

set -e

VERSION="$1"

if [ -z "$VERSION" ]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 0.1.14"
    exit 1
fi

# Validate version format (semver)
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
    echo "Error: Invalid version format. Expected semver (e.g., 0.1.14 or 0.1.14-beta.1)"
    exit 1
fi

# Check for uncommitted changes
if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "Error: Working directory has uncommitted changes. Please commit or stash them first."
    exit 1
fi

# Check we're on main branch
BRANCH=$(git branch --show-current)
if [ "$BRANCH" != "main" ]; then
    echo "Warning: Not on main branch (currently on $BRANCH)"
    read -p "Continue anyway? [y/N] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi

# Get current version from Cargo.toml
CURRENT_VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
echo "Current version: $CURRENT_VERSION"
echo "New version: $VERSION"
echo

# ── Pre-release checks ──────────────────────────────────────────────
echo "Running pre-release checks..."
echo

WORKSPACE_ROOT="$(dirname "$PROJECT_ROOT")"
CHECKS_FAILED=0

# 1. Server: cargo test
echo "[1/5] Running server tests..."
if ! cargo test --all --quiet 2>&1; then
    echo "FAIL: cargo test"
    CHECKS_FAILED=1
fi

# 2. Server: cargo clippy
echo "[2/5] Running clippy..."
if ! cargo clippy --all --quiet -- -D warnings 2>&1; then
    echo "FAIL: cargo clippy"
    CHECKS_FAILED=1
fi

# 3. Server: release build
echo "[3/5] Building server (release)..."
if ! cargo build --release --quiet 2>&1; then
    echo "FAIL: cargo build --release"
    CHECKS_FAILED=1
fi

# 4. Web: build
WEB_DIR="$WORKSPACE_ROOT/manifest-web"
if [ -d "$WEB_DIR" ]; then
    echo "[4/5] Building manifest-web..."
    if ! (cd "$WORKSPACE_ROOT" && pnpm --filter app build) 2>&1; then
        echo "FAIL: manifest-web build"
        CHECKS_FAILED=1
    fi

    # 5. Web: tests
    echo "[5/5] Running manifest-web tests..."
    if ! (cd "$WORKSPACE_ROOT" && pnpm --filter app test:run) 2>&1; then
        echo "FAIL: manifest-web tests"
        CHECKS_FAILED=1
    fi
else
    echo "[4/5] Skipping manifest-web build (not found at $WEB_DIR)"
    echo "[5/5] Skipping manifest-web tests"
fi

if [ "$CHECKS_FAILED" -ne 0 ]; then
    echo
    echo "Pre-release checks FAILED. Fix issues before releasing."
    exit 1
fi

echo
echo "All pre-release checks passed."
echo "────────────────────────────────────────────────────────────────────"
echo

# Confirm
read -p "Release v$VERSION? [y/N] " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Aborted."
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Update Cargo.toml
echo "Updating Cargo.toml..."
sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" "$PROJECT_ROOT/Cargo.toml"

# Update plugin.json in claude-plugins marketplace
echo "Updating plugin.json in claude-plugins..."
CLAUDE_PLUGINS_DIR="$(dirname "$PROJECT_ROOT")/claude-plugins"
PLUGIN_JSON="$CLAUDE_PLUGINS_DIR/plugins/manifest/.claude-plugin/plugin.json"
if [ -f "$PLUGIN_JSON" ]; then
    sed -i '' "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" "$PLUGIN_JSON"
else
    echo "Warning: $PLUGIN_JSON not found, skipping plugin.json update"
fi

# Update manifest-wrapper.sh in claude-plugins marketplace
echo "Updating manifest-wrapper.sh in claude-plugins..."
CLAUDE_PLUGINS_DIR="$(dirname "$PROJECT_ROOT")/claude-plugins"
WRAPPER_SH="$CLAUDE_PLUGINS_DIR/plugins/manifest/bin/manifest-wrapper.sh"
if [ -f "$WRAPPER_SH" ]; then
    sed -i '' "s/^VERSION=\".*\"/VERSION=\"$VERSION\"/" "$WRAPPER_SH"
else
    echo "Warning: $WRAPPER_SH not found, skipping wrapper update"
fi

# Verify changes in Manifest
echo
echo "Changes in Manifest:"
git diff --stat

# Commit Manifest
echo
echo "Committing Manifest..."
git add "$PROJECT_ROOT/Cargo.toml"
git commit -m "Bump version to $VERSION"

# Tag
echo "Creating tag v$VERSION..."
git tag "v$VERSION"

# Push Manifest
echo "Pushing Manifest to origin..."
git push origin main
git push origin "v$VERSION"

# Commit and push claude-plugins if files were updated
if [ -d "$CLAUDE_PLUGINS_DIR/.git" ]; then
    echo
    echo "Committing claude-plugins..."
    git -C "$CLAUDE_PLUGINS_DIR" add plugins/manifest/
    if git -C "$CLAUDE_PLUGINS_DIR" diff --cached --quiet; then
        echo "No changes to commit in claude-plugins"
    else
        git -C "$CLAUDE_PLUGINS_DIR" commit -m "Update manifest to $VERSION"
        echo "Pushing claude-plugins to origin..."
        git -C "$CLAUDE_PLUGINS_DIR" push origin main
    fi
fi

echo
echo "Release v$VERSION initiated!"
echo "Monitor the release workflow at:"
echo "  gh run list --workflow=release.yml --limit=1"
echo
echo "Or watch it:"
echo "  gh run watch"
