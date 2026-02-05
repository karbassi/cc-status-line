# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.6] - 2026-02-05

### Added

- Pre-commit hook now blocks direct commits to main branch with worktree setup instructions

### Fixed

- Pre-push hook clears `GIT_DIR`/`GIT_WORK_TREE` env vars that interfered with integration tests

## [0.1.5] - 2026-02-05

### Added

- **Config file support**: Customize display via `~/.config/claude/cc-statusline.json`
  - Choose which components to show and their order
  - Organize components into rows
  - Respects `XDG_CONFIG_HOME` environment variable
- New `--config-init` flag to create default config file
- New `--config-init --force` flag to overwrite existing config
- 17 configurable components: `hostname`, `project`, `path`, `no_git`, `branch`, `worktree`, `files`, `ahead_behind`, `pr_number`, `pr_state`, `pr_comments`, `pr_files`, `pr_checks`, `model`, `context`, `style`, `duration`, `tokens`

### Changed

- Path abbreviation now uses conservative fixed width (60% of terminal) for config flexibility

## [0.1.4] - 2026-02-04

### Added

- SSH hostname detection: display hostname in green on Row 1 when connected via SSH
- Shared git hooks in `.githooks/` (run `make setup` to enable):
  - `pre-commit`: auto-format with `cargo fmt`, lint with `cargo clippy`
  - `pre-push`: run `cargo test` before pushing
  - `commit-msg`: enforce conventional commit prefixes

## [0.1.3] - 2026-01-28

### Added

- `--version` and `--help` CLI flags
- Official JSON input fixture test from docs

## [0.1.2] - 2025-01-20

### Fixed

- Fixed git path construction missing trailing slash, causing cache invalidation to fail and stale branch names to be displayed

## [0.1.1] - 2025-01-20

### Added

- Initial release with git branch, diff stats, and worktree support
- Tokyo Night Dim color theme
- Memory-mapped caching for performance
