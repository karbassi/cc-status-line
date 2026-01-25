# Claude Code Instructions

This file provides guidance to Claude Code (claude.ai/code) when working with this repository.

## Project

Lightweight, fast status line for Claude Code CLI. Bash prototype first, then Rust rewrite.

## Git

Commit after each step. Short messages. Run in background.

Never use `git add -A` or `git add .`. Always add specific files relevant to the current task. Multiple Claude sessions may run concurrently.

## Branch Protection

The `main` branch is protected by a [GitHub ruleset](https://github.com/karbassi/cc-status-line/rules/12118492):
- Direct pushes blocked (must use PR)
- Copilot auto-reviews on each push
- Must resolve all comment threads before merging

## Workflow

Use subagents (Task tool) as much as possible. Run in background when possible.
