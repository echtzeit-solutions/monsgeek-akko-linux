#!/bin/bash
# Install git hooks for development

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
HOOKS_DIR="$REPO_ROOT/.git/hooks"

echo "Installing pre-commit hook..."
ln -sf "$SCRIPT_DIR/pre-commit" "$HOOKS_DIR/pre-commit"
echo "Done! Pre-commit hook installed."
echo ""
echo "The hook will run 'cargo fmt --check' and 'cargo clippy' before each commit."
