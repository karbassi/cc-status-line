#!/bin/bash
# Install Claude Code status line
# Adds statusline config to ~/.claude/settings.json

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STATUSLINE_PATH="$SCRIPT_DIR/statusline.sh"
SETTINGS_FILE="$HOME/.claude/settings.json"

# Check dependencies
check_deps() {
    local missing=()
    command -v jq >/dev/null 2>&1 || missing+=("jq")
    command -v git >/dev/null 2>&1 || missing+=("git")
    command -v bc >/dev/null 2>&1 || missing+=("bc")

    if [[ ${#missing[@]} -gt 0 ]]; then
        echo "Missing dependencies: ${missing[*]}"
        echo "Install with: brew install ${missing[*]}"
        exit 1
    fi
}

# Ensure settings directory exists
ensure_settings_dir() {
    mkdir -p "$(dirname "$SETTINGS_FILE")"
}

# Update or create settings.json
update_settings() {
    local statusline_config
    statusline_config=$(cat <<EOF
{
  "type": "command",
  "command": "$STATUSLINE_PATH",
  "padding": 0
}
EOF
)

    if [[ -f "$SETTINGS_FILE" ]]; then
        # Backup existing
        cp "$SETTINGS_FILE" "${SETTINGS_FILE}.backup"

        # Update statusLine key
        local updated
        updated=$(jq --argjson sl "$statusline_config" '.statusLine = $sl' "$SETTINGS_FILE")
        echo "$updated" > "$SETTINGS_FILE"
        echo "Updated $SETTINGS_FILE (backup at ${SETTINGS_FILE}.backup)"
    else
        # Create new settings file
        echo "{\"statusLine\": $statusline_config}" | jq '.' > "$SETTINGS_FILE"
        echo "Created $SETTINGS_FILE"
    fi
}

main() {
    echo "Installing Claude Code status line..."
    echo ""

    check_deps
    ensure_settings_dir
    update_settings

    echo ""
    echo "Done! Restart Claude Code to see the new status line."
    echo ""
    echo "To uninstall, remove the 'statusLine' key from $SETTINGS_FILE"
    echo "or restore from ${SETTINGS_FILE}.backup"
}

main "$@"
