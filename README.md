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

## Design

4-5 row status line with Tokyo Night colors and dot (•) dividers. PR row appears when a PR exists for the current branch.

```
 trips • d/api/endpoints
 main • feature-auth • 3 files +45 -12 • ↑1
 #42 • open • 2 comments • 5 files • checks passed
 Opus • 84% • verbose • ◔ 35m
 47m • resets 12m • 125K/42K
```

### Row 1: Location
- Project name (basename of where Claude launched)
- CWD relative to project (fish-style abbreviated if >20 chars OR >3 segments)

### Row 2: Git
- Branch name
- Worktree name (if active)
- Changed files count, lines added (`+N`), lines deleted (`-N`)
- Remote: `↑N` ahead, `↓N` behind

### Row 3: PR (optional, GitHub only)
- PR number with clickable link (OSC 8)
- State: open/merged/closed
- Comments count
- Changed files count
- Check status: passed/failed/pending (clickable link to checks page)

**Requirements for PR row:**
- GitHub repository with origin remote
- Authentication via one of:
  - `GITHUB_TOKEN` or `GH_TOKEN` environment variable (all platforms)
  - GitHub CLI (`gh auth login`) - macOS/Linux only
  - Git credential helper with GitHub credentials (all platforms)

If no authentication is available, the PR row will not appear. On Windows, use an environment variable or git credential helper since `gh auth login` is not used by the native HTTP path.

### Row 4: Claude
- Model (Opus/Sonnet/Haiku)
- Context % remaining
- Output mode
- Block timer

### Row 5: Session
- Session duration
- Block reset countdown
- Tokens (in/out)

## Style

- **Theme**: Tokyo Night
- **Dividers**: Dot (•)
- **Colors by segment**:
  - Blue `#7aa2f7` - project
  - Cyan `#7dcfff` - cwd
  - Purple `#bb9af7` - git branch
  - Magenta `#9d7cd8` - worktree
  - Green `#9ece6a` - additions/clean
  - Yellow `#e0af68` - modifications/timers
  - Orange `#ff9e64` - model
  - Teal `#2ac3de` - context %
  - Gray `#565f89` - muted/session

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
