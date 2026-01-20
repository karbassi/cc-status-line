#!/bin/bash
# Claude Code Status Line - Tokyo Night theme
# Reads JSON from stdin (provided by Claude Code) and outputs 4-row status

set -euo pipefail

# ═══════════════════════════════════════════════════════════════════
# Tokyo Night Dim Colors
# ═══════════════════════════════════════════════════════════════════
RESET="\033[0m"
TN_BLUE="\033[2;38;2;122;162;247m"     # project
TN_CYAN="\033[2;38;2;125;207;255m"     # cwd
TN_PURPLE="\033[2;38;2;187;154;247m"   # git branch
TN_MAGENTA="\033[2;38;2;157;124;216m"  # worktree
TN_GREEN="\033[2;38;2;158;206;106m"    # additions
TN_YELLOW="\033[2;38;2;224;175;104m"   # modifications/timers
TN_ORANGE="\033[2;38;2;255;158;100m"   # model
TN_TEAL="\033[2;38;2;42;195;222m"      # context %
TN_GRAY="\033[2;38;2;86;95;137m"       # muted/dividers
TN_RED="\033[2;38;2;247;118;142m"      # deletions

SEP="${TN_GRAY} • ${RESET}"

# ═══════════════════════════════════════════════════════════════════
# Terminal width - use CC_STATUS_WIDTH env var, or default to 50 (works for splits)
# ═══════════════════════════════════════════════════════════════════
TERM_WIDTH=${CC_STATUS_WIDTH:-50}

# ═══════════════════════════════════════════════════════════════════
# Read JSON from stdin
# ═══════════════════════════════════════════════════════════════════
INPUT=$(cat)

# ═══════════════════════════════════════════════════════════════════
# Extract Claude data (single jq call for performance)
# ═══════════════════════════════════════════════════════════════════
IFS=$'\t' read -r MODEL CONTEXT_PCT INPUT_TOKENS OUTPUT_TOKENS DURATION_MS OUTPUT_MODE PROJECT_DIR CURRENT_DIR TRANSCRIPT_PATH < <(
    echo "$INPUT" | jq -r '[
        (.model.display_name // "Unknown"),
        ((.context_window.remaining_percentage // 100) | floor),
        (.context_window.total_input_tokens // 0),
        (.context_window.total_output_tokens // 0),
        (.cost.total_duration_ms // 0),
        (.output_style.name // "default"),
        (.workspace.project_dir // ""),
        (.workspace.current_dir // ""),
        (.transcript_path // "")
    ] | @tsv'
)

# Ensure numeric values are valid integers (default to 0)
[[ ! "$CONTEXT_PCT" =~ ^[0-9]+$ ]] && CONTEXT_PCT=100
[[ ! "$INPUT_TOKENS" =~ ^[0-9]+$ ]] && INPUT_TOKENS=0
[[ ! "$OUTPUT_TOKENS" =~ ^[0-9]+$ ]] && OUTPUT_TOKENS=0
[[ ! "$DURATION_MS" =~ ^[0-9]+$ ]] && DURATION_MS=0

# Fall back to PWD if CURRENT_DIR is empty
[[ -z "$CURRENT_DIR" ]] && CURRENT_DIR="$PWD"

# ═══════════════════════════════════════════════════════════════════
# Helper Functions
# ═══════════════════════════════════════════════════════════════════

# Format tokens (e.g., 125432 -> 125K)
format_tokens() {
    local n=$1
    if [[ $n -ge 1000000 ]]; then
        printf "%.1fM" "$(echo "scale=1; $n/1000000" | bc)"
    elif [[ $n -ge 1000 ]]; then
        printf "%.0fK" "$(echo "scale=0; $n/1000" | bc)"
    else
        printf "%d" "$n"
    fi
}

# Format duration (ms -> Nm or Nh Nm)
format_duration() {
    local ms=$1
    local total_secs=$((ms / 1000))
    local mins=$((total_secs / 60))
    local hours=$((mins / 60))
    mins=$((mins % 60))

    if [[ $hours -gt 0 ]]; then
        printf "%dh %dm" "$hours" "$mins"
    else
        printf "%dm" "$mins"
    fi
}

# Smart path abbreviation based on available width
# Usage: abbreviate_path "path" max_width
# - If path fits in max_width, show full path
# - Otherwise, fish-style abbreviate until it fits (keeping last N segments full)
abbreviate_path() {
    local path="$1"
    local max_width="${2:-50}"

    # If path fits, return as-is
    if [[ ${#path} -le $max_width ]]; then
        echo "$path"
        return
    fi

    local segments
    IFS='/' read -ra segments <<< "$path"
    local count=${#segments[@]}

    # Need at least 2 segments to abbreviate
    if [[ $count -lt 2 ]]; then
        echo "$path"
        return
    fi

    # Try keeping last 2 segments full, abbreviate rest
    local result=""
    for ((i=0; i<count-2; i++)); do
        local seg="${segments[$i]}"
        if [[ -n "$seg" ]]; then
            result+="${seg:0:1}/"
        fi
    done
    result+="${segments[$count-2]}/${segments[$count-1]}"

    # If still too long, keep only last segment full
    if [[ ${#result} -gt $max_width && $count -gt 2 ]]; then
        result=""
        for ((i=0; i<count-1; i++)); do
            local seg="${segments[$i]}"
            if [[ -n "$seg" ]]; then
                result+="${seg:0:1}/"
            fi
        done
        result+="${segments[$count-1]}"
    fi

    echo "$result"
}

# ═══════════════════════════════════════════════════════════════════
# Row 1: Location (project • cwd)
# ═══════════════════════════════════════════════════════════════════
PROJECT_NAME=$(basename "$PROJECT_DIR" 2>/dev/null || echo "")

# Calculate CWD display - show path relative to home with ~ prefix
if [[ -n "$CURRENT_DIR" ]]; then
    # Replace $HOME with ~ for display
    if [[ "$CURRENT_DIR" == "$HOME"* ]]; then
        DISPLAY_CWD="~${CURRENT_DIR#$HOME}"
    else
        DISPLAY_CWD="$CURRENT_DIR"
    fi
else
    DISPLAY_CWD="."
fi

# Calculate available width for path (terminal width - project name - separator)
PATH_WIDTH=$((TERM_WIDTH - ${#PROJECT_NAME} - 3))
[[ $PATH_WIDTH -lt 10 ]] && PATH_WIDTH=10  # Minimum width
ABBREV_CWD=$(abbreviate_path "$DISPLAY_CWD" "$PATH_WIDTH")

ROW1="${TN_BLUE}${PROJECT_NAME}${RESET}${SEP}${TN_CYAN}${ABBREV_CWD}${RESET}"

# ═══════════════════════════════════════════════════════════════════
# Row 2: Git (branch • worktree • status • remote)
# ═══════════════════════════════════════════════════════════════════
GIT_BRANCH=$(git -C "$CURRENT_DIR" branch --show-current 2>/dev/null || echo "")
GIT_WORKTREE=""

# Check if in a worktree (not the main worktree)
if [[ -n "$GIT_BRANCH" ]]; then
    GIT_DIR=$(git -C "$CURRENT_DIR" rev-parse --git-dir 2>/dev/null || echo "")
    if [[ "$GIT_DIR" == *".git/worktrees/"* ]]; then
        GIT_WORKTREE=$(basename "$GIT_DIR")
    fi
fi

# Git line counts (additions/deletions) and file count
GIT_LINES_ADDED=0
GIT_LINES_DELETED=0
GIT_FILES_CHANGED=0

if [[ -n "$GIT_BRANCH" ]]; then
    # Get line counts from both staged and unstaged changes
    while IFS=$'\t' read -r added deleted _; do
        # Skip empty lines or binary files (shown as -)
        [[ -z "$added" || "$added" == "-" ]] && continue
        # Only count if we have valid numeric data
        if [[ "$added" =~ ^[0-9]+$ ]]; then
            ((GIT_LINES_ADDED += added))
            ((GIT_FILES_CHANGED++))
        fi
        [[ "$deleted" =~ ^[0-9]+$ ]] && ((GIT_LINES_DELETED += deleted))
    done < <(git -C "$CURRENT_DIR" diff --numstat HEAD 2>/dev/null || git -C "$CURRENT_DIR" diff --numstat 2>/dev/null || true)
fi

# Remote ahead/behind
GIT_AHEAD=0
GIT_BEHIND=0

if [[ -n "$GIT_BRANCH" ]]; then
    UPSTREAM=$(git -C "$CURRENT_DIR" rev-parse --abbrev-ref "@{upstream}" 2>/dev/null || echo "")
    if [[ -n "$UPSTREAM" ]]; then
        AHEAD_BEHIND=$(git -C "$CURRENT_DIR" rev-list --left-right --count HEAD..."$UPSTREAM" 2>/dev/null || echo "0 0")
        GIT_AHEAD=$(echo "$AHEAD_BEHIND" | awk '{print $1}')
        GIT_BEHIND=$(echo "$AHEAD_BEHIND" | awk '{print $2}')
    fi
fi

# Build Row 2
ROW2=""
if [[ -n "$GIT_BRANCH" ]]; then
    ROW2="${TN_PURPLE}${GIT_BRANCH}${RESET}"

    if [[ -n "$GIT_WORKTREE" ]]; then
        ROW2+="${SEP}${TN_MAGENTA}${GIT_WORKTREE}${RESET}"
    fi

    # File and line change indicators
    STATUS_PARTS=""
    [[ $GIT_FILES_CHANGED -gt 0 ]] && STATUS_PARTS+="${TN_GRAY}${GIT_FILES_CHANGED} files${RESET} "
    [[ $GIT_LINES_ADDED -gt 0 ]] && STATUS_PARTS+="${TN_GREEN}+${GIT_LINES_ADDED}${RESET} "
    [[ $GIT_LINES_DELETED -gt 0 ]] && STATUS_PARTS+="${TN_RED}-${GIT_LINES_DELETED}${RESET} "

    if [[ -n "$STATUS_PARTS" ]]; then
        ROW2+="${SEP}${STATUS_PARTS% }"
    fi

    # Remote indicators
    REMOTE_PARTS=""
    [[ $GIT_AHEAD -gt 0 ]] && REMOTE_PARTS+="${TN_GRAY}↑${GIT_AHEAD}${RESET} "
    [[ $GIT_BEHIND -gt 0 ]] && REMOTE_PARTS+="${TN_GRAY}↓${GIT_BEHIND}${RESET} "

    if [[ -n "$REMOTE_PARTS" ]]; then
        ROW2+="${SEP}${REMOTE_PARTS% }"
    fi
else
    ROW2="${TN_GRAY}no git${RESET}"
fi

# ═══════════════════════════════════════════════════════════════════
# Row 3: Claude (Model • Context% • Mode • Block Timer)
# ═══════════════════════════════════════════════════════════════════

# Block timer - time elapsed in current 5-hour block
BLOCK_TIMER=""
FIRST_TS=""
FIRST_EPOCH=""
if [[ -n "$TRANSCRIPT_PATH" && -f "$TRANSCRIPT_PATH" ]]; then
    # Get first timestamp from transcript (find first user/assistant message with actual timestamp)
    FIRST_TS=$(grep -m1 '"type":"user"' "$TRANSCRIPT_PATH" 2>/dev/null | jq -r '.timestamp // empty' 2>/dev/null)
    if [[ -n "$FIRST_TS" && "$FIRST_TS" != "null" ]]; then
        # Parse ISO timestamp (format: 2026-01-20T03:13:42.539Z)
        # Try macOS date first, then GNU date
        FIRST_EPOCH=$(date -j -f "%Y-%m-%dT%H:%M:%S" "${FIRST_TS%%.*}" "+%s" 2>/dev/null || \
                      date -d "${FIRST_TS}" "+%s" 2>/dev/null || echo "")
        if [[ -n "$FIRST_EPOCH" ]]; then
            NOW_EPOCH=$(date "+%s")
            ELAPSED_SECS=$((NOW_EPOCH - FIRST_EPOCH))
            BLOCK_SECS=$((ELAPSED_SECS % (5 * 3600)))
            BLOCK_MINS=$((BLOCK_SECS / 60))
            BLOCK_TIMER="${BLOCK_MINS}m"
        fi
    fi
fi

# Build Row 3 with conditional segments
ROW3=""
if [[ -n "$MODEL" && "$MODEL" != "Unknown" ]]; then
    ROW3="${TN_ORANGE}${MODEL}${RESET}"
fi
if [[ $CONTEXT_PCT -lt 100 ]]; then
    [[ -n "$ROW3" ]] && ROW3+="$SEP"
    ROW3+="${TN_TEAL}${CONTEXT_PCT}%${RESET}"
fi
if [[ -n "$OUTPUT_MODE" && "$OUTPUT_MODE" != "default" ]]; then
    [[ -n "$ROW3" ]] && ROW3+="$SEP"
    ROW3+="${TN_BLUE}${OUTPUT_MODE}${RESET}"
fi
if [[ -n "$BLOCK_TIMER" ]]; then
    [[ -n "$ROW3" ]] && ROW3+="$SEP"
    ROW3+="${TN_YELLOW}◔ ${BLOCK_TIMER}${RESET}"
fi

# ═══════════════════════════════════════════════════════════════════
# Row 4: Session (Duration • Block Reset • Tokens)
# ═══════════════════════════════════════════════════════════════════
DURATION_FMT=$(format_duration "$DURATION_MS")
INPUT_FMT=$(format_tokens "$INPUT_TOKENS")
OUTPUT_FMT=$(format_tokens "$OUTPUT_TOKENS")

# Block reset countdown (time until 5h block resets) - reuses FIRST_EPOCH from above
BLOCK_RESET=""
if [[ -n "$FIRST_EPOCH" ]]; then
    NOW_EPOCH=$(date "+%s")
    ELAPSED_SECS=$((NOW_EPOCH - FIRST_EPOCH))
    BLOCK_REMAINING=$((5 * 3600 - (ELAPSED_SECS % (5 * 3600))))
    RESET_MINS=$((BLOCK_REMAINING / 60))
    BLOCK_RESET="resets ${RESET_MINS}m"
fi

# Build Row 4 with conditional segments
ROW4=""
if [[ $DURATION_MS -gt 0 ]]; then
    ROW4="${TN_GRAY}${DURATION_FMT}${RESET}"
fi
if [[ -n "$BLOCK_RESET" ]]; then
    [[ -n "$ROW4" ]] && ROW4+="$SEP"
    ROW4+="${TN_GRAY}${BLOCK_RESET}${RESET}"
fi
if [[ $INPUT_TOKENS -gt 0 || $OUTPUT_TOKENS -gt 0 ]]; then
    [[ -n "$ROW4" ]] && ROW4+="$SEP"
    ROW4+="${TN_GRAY}${INPUT_FMT}/${OUTPUT_FMT}${RESET}"
fi

# ═══════════════════════════════════════════════════════════════════
# Output - only show rows with content
# ═══════════════════════════════════════════════════════════════════
echo -e "$ROW1"
echo -e "$ROW2"
[[ -n "$ROW3" ]] && echo -e "$ROW3"
[[ -n "$ROW4" ]] && echo -e "$ROW4"
