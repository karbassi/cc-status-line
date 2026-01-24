# cc-status-line

A lightweight, fast status line for Claude Code CLI.

## Quick Start

```bash
# Install
brew install karbassi/tap/cc-statusline

# Configure Claude Code (~/.claude/settings.json)
{
  "statusLine": {
    "type": "command",
    "command": "cc-statusline"
  }
}
```

Restart Claude Code to see the new status line.

## Features

### Row 1: Location
- **Project name** - basename of where Claude launched
- **Current directory** - fish-style abbreviated path (e.g., `~/p/weather-app/s/components`)

### Row 2: Git
- **Branch name** - current git branch
- **Worktree name** - if using git worktrees
- **Changed files** - number of modified files
- **Remote tracking** - `↑N` commits ahead, `↓N` commits behind

### Row 3: Pull Request (optional)
- **PR number** - with state color (green=open, purple=merged, red=closed)
- **Changed files** - files changed in PR
- **Check status** - checks passed/failed/pending

### Row 4: Claude
- **Model** - Opus 4.5, Sonnet 4, Haiku
- **Context %** - remaining context window
- **Output mode** - verbose, concise, etc.

### Row 5: Session
- **Duration** - total session time
- **Tokens** - input/output token counts

## Example Output

```
weather-app • ~/projects/weather-app
feat/hourly-forecast • 3 files
#42 • 5 files • checks passed
Opus 4.5 • 47%
31m • 150K/45K
```

## Style

- **Theme**: Tokyo Night (dim)
- **Dividers**: Dot (•)
- **Colors**:
  - Blue `#7aa2f7` - project name
  - Cyan `#7dcfff` - current directory
  - Purple `#bb9af7` - git branch
  - Magenta `#9d7cd8` - worktree
  - Green `#9ece6a` - PR open, checks passed
  - Orange `#ff9e64` - model, checks pending
  - Red `#f7768e` - PR closed, checks failed
  - Teal `#2ac3de` - context %
  - Gray `#565f89` - muted text, session info

## Performance

| Version | Mean | Min |
|---------|------|-----|
| Bash | 80 ms | 59 ms |
| Rust | 3.7 ms | 3.0 ms |

**20x faster than bash.** Binary <1MB, ~1.4MB RAM.

Optimizations:
- gix (pure Rust git, minimal features)
- mmap-based caching (auto-invalidates on git changes)
- opt-level=s, LTO, panic=abort

## Installation

### Homebrew

```bash
brew install karbassi/tap/cc-statusline
```

### Build from source

```bash
cargo build --release
cp target/release/cc-statusline ~/.local/bin/
```

## License

MIT
