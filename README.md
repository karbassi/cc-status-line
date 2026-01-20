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

Benchmarked with hyperfine (50 runs, same JSON input):

| Version | Time | Speedup |
|---------|------|---------|
| Bash | 75 ms | baseline |
| Rust | 4.4 ms | 17x faster |

The Rust version uses gix (pure Rust git) and mmap-based caching. Caching is automatic and invalidates when git state changes.

## Development

### Phase 1: Bash prototype
Quick iteration on design and features.

### Phase 2: Rust rewrite
For performance (<1% CPU, <10ms startup).

## Installation

TBD

## License

MIT
