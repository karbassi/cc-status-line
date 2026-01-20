# cc-status-line

A lightweight, fast status line for Claude Code CLI.

## Design

4-row status line with Tokyo Night dim colors and dot (•) dividers.

```
 trips • d/api/endpoints
 main • feature-auth • +3 ~2 • ↑1
 Opus • 84% • verbose • ◔ 35m
 47m • resets 12m • 125K/42K
```

### Row 1: Location
- Project name (basename of where Claude launched)
- CWD relative to project (fish-style abbreviated if >20 chars OR >3 segments)

### Row 2: Git
- Branch name
- Worktree name (if active)
- Status: `+N` added, `~N` modified, `-N` deleted, `?N` untracked
- Remote: `↑N` ahead, `↓N` behind

### Row 3: Claude
- Model (Opus/Sonnet/Haiku)
- Context % remaining
- Output mode
- Block timer

### Row 4: Session
- Session duration
- Block reset countdown
- Tokens (in/out)

## Style

- **Theme**: Tokyo Night (dim)
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

### Download binary

Download the latest release for your platform from [Releases](https://github.com/karbassi/cc-status-line/releases):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/karbassi/cc-status-line/releases/latest/download/cc-statusline-macos-arm64.tar.gz | tar xz
mv cc-statusline ~/.local/bin/

# macOS (Intel)
curl -L https://github.com/karbassi/cc-status-line/releases/latest/download/cc-statusline-macos-x86_64.tar.gz | tar xz
mv cc-statusline ~/.local/bin/

# Linux (x86_64)
curl -L https://github.com/karbassi/cc-status-line/releases/latest/download/cc-statusline-linux-x86_64.tar.gz | tar xz
mv cc-statusline ~/.local/bin/

# Linux (ARM64)
curl -L https://github.com/karbassi/cc-status-line/releases/latest/download/cc-statusline-linux-arm64.tar.gz | tar xz
mv cc-statusline ~/.local/bin/
```

### Build from source

```bash
cd rust
cargo build --release
cp target/release/cc-statusline ~/.local/bin/
```

## License

MIT
