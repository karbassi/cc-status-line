# cc-status-line

A lightweight, fast status line for Claude Code CLI.

![Full status line example](docs/screenshots/16-full.png)

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

See all screenshot variations in [docs/screenshots/](docs/screenshots/).

### Row 1: Location
- Project name (basename of where Claude launched)
- CWD relative to project (fish-style abbreviated if >20 chars OR >3 segments)

### Row 2: Git
- Branch name
- Worktree name (if active)
- Changed files count
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

## JSON Input

Claude Code passes session data via stdin as JSON. All fields are optional:

```json
{
  "model": {
    "display_name": "Claude Opus 4.5"
  },
  "context_window": {
    "remaining_percentage": 75.5,
    "total_input_tokens": 50000,
    "total_output_tokens": 25000
  },
  "cost": {
    "total_duration_ms": 125000
  },
  "output_style": {
    "name": "verbose"
  },
  "workspace": {
    "project_dir": "/path/to/project",
    "current_dir": "/path/to/project/src"
  },
  "git": {
    "branch": "feature-branch",
    "worktree": "my-worktree",
    "changed_files": 5,
    "ahead": 2,
    "behind": 1
  },
  "pr": {
    "number": 42,
    "state": "open",
    "url": "https://github.com/owner/repo/pull/42",
    "comments": 3,
    "changed_files": 10,
    "check_status": "passed"
  }
}
```

When `git` or `pr` fields are provided in JSON, filesystem detection is skipped for those sections. This is useful for screenshots or testing.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `GITHUB_TOKEN` | GitHub API token for PR info (preferred) |
| `GH_TOKEN` | Alternative GitHub token (used by gh CLI) |
| `XDG_CACHE_HOME` | Cache directory base (default: `~/.cache`) |
| `HOME` | User home directory for `~` expansion |

Cache files are stored in `$XDG_CACHE_HOME/cc-statusline/` (or `~/.cache/cc-statusline/`).

## Performance

| Version | Mean | Min |
|---------|------|-----|
| Bash | 80 ms | 59 ms |
| Rust | 3.7 ms | 3.0 ms |

**20x faster than bash.** Binary ~2MB, ~1.4MB RAM.

### Optimizations

- **gix**: Pure Rust git library with minimal features
- **mmap caching**: Auto-invalidates on git index/HEAD changes
- **Native TLS**: Uses OS-provided TLS (no ring/rustls overhead)
- **Release profile**: `opt-level=s`, LTO, `panic=abort`

### Running Benchmarks

```bash
cargo bench
```

Benchmarks include:
- **startup_minimal**: Empty JSON input (~3.2ms)
- **startup_full_json**: Full JSON input (~3.2ms)
- **Pure functions**: hash_path, shell_escape, percent_encode, parse_github_url, abbreviate_path

Results are saved to `target/criterion/` with HTML reports.

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

## Development

```bash
# Run tests
cargo test

# Run benchmarks
cargo bench

# Build release binary
cargo build --release

# Check formatting and lints
cargo fmt --check
cargo clippy -- -D warnings
```

## License

MIT
